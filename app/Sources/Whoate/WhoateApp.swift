// Entry point. PATH is repaired before the engine exists; the automation
// entry (WHOATE_UI_AUTOMATION) is inert unless that variable is set.
import AppKit
import SwiftUI
import WhoateKit

@main
enum Main {
    /// Process start on the uptime clock: the automation timers count from here.
    nonisolated(unsafe) static var launchedAt = ProcessInfo.processInfo.systemUptime

    static func main() {
        launchedAt = ProcessInfo.processInfo.systemUptime
        let automation = Automation(environment: ProcessInfo.processInfo.environment)
        if !automation.noPathRepair { repairProcessPath() }
        if automation.dumpTick { dumpTick(automation) }
        // Before anything else touches AppKit: even a bare executable without
        // Info.plist must never register as a foreground app with a Dock icon.
        NSApplication.shared.setActivationPolicy(.accessory)
        WhoateApp.automation = automation
        WhoateApp.main()
    }

    /// Prints one tick JSON and exits: 0 for a report, 1 for an error envelope.
    static func dumpTick(_ automation: Automation) -> Never {
        guard let engine = makeEngine(automation) else {
            FileHandle.standardError.write(Data("engine_unavailable\n".utf8))
            exit(1)
        }
        let data = engine.tickData()
        FileHandle.standardOutput.write(data + Data("\n".utf8))
        if case .value = decode(Report.self, from: data) { exit(0) }
        exit(1)
    }

    static func makeEngine(_ automation: Automation) -> Engine? {
        #if WHOATE_FIXTURE
        if let fixture = automation.fixture { return Engine.fixture(path: fixture) }
        #endif
        return Engine.live()
    }
}

struct WhoateApp: App {
    nonisolated(unsafe) static var automation = Automation.off
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        MenuBarExtra {
            PanelView(model: delegate.model, mainWindows: delegate.mainWindows,
                      openMain: { PanelActions.showMain($0, open: open, dismiss: delegate.dismissPanel) },
                      quit: delegate.quit, foundWindow: { delegate.panelWindow = $0 })
        } label: {
            // Owner design gate 2026-10-21: style D (a system symbol, alerts as triangles).
            MenuBarLabel(label: MenuBarItem.label(delegate.model.host, style: .systemSymbol, language: delegate.model.language))
                .task { await delegate.runAutomation(open: open) }
        }
        .menuBarExtraStyle(.window)

        Window(L10n.text(.mainWindowTitle, delegate.model.language), id: "main") {
            MainWindowView(model: delegate.model, mainWindows: delegate.mainWindows)
        }
        .defaultSize(width: 1120, height: 720)
        .commands {
            // The regular-policy app menu's ⌘Q goes through the same QuitAction.
            CommandGroup(replacing: .appTermination) {
                Button(L10n.text(.quit, delegate.model.language)) { delegate.quit() }.keyboardShortcut("q")
            }
        }
    }

    /// An existing main window is raised; otherwise SwiftUI creates it. A panel
    /// row's owner goes through AppModel.select: its category, scrolled into view (R9).
    private func open(_ ownerID: String?) {
        if let ownerID { delegate.model.select(ownerID) }
        switch WindowPolicy.open(mainWindowExists: delegate.mainWindows.exists) {
        case .bringToFront: delegate.mainWindows.bringToFront()
        case .create:
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
            // A reused window gets no new reporter callback: track it again once it is shown.
            let windows = delegate.mainWindows
            for delay in AppDelegate.reattachDelays {
                DispatchQueue.main.asyncAfter(deadline: .now() + delay) { windows.reattach() }
            }
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model: AppModel
    let mainWindows = MainWindows()
    let quit = QuitAction()
    /// After openWindow on a reused main window: it is shown within the first
    /// run-loop turns (0.1 s measured in the QA22 reopen run); 0.5 s covers a slow tick.
    static let reattachDelays = [0.1, 0.5]
    /// Automation waits this long for the first report (fixture replay takes < 1 s).
    static let reportWait = 10.0
    /// The panel's window: the MenuBarExtra one, or the automation panel window.
    weak var panelWindow: NSWindow?
    /// SwiftUI's main-window opener, captured from the menu bar label (automation panel window).
    private var openMain: ((String?) -> Void)?
    private var automationWindow: NSWindow?
    private var automationStarted = false

    override init() {
        let automation = WhoateApp.automation
        model = AppModel(language: .system(), makeEngine: { Main.makeEngine(automation) }, trace: { mode in
            guard let path = automation.scheduleLog else { return }
            // Opt-in safe event log; no process, argv or session source content.
            appendLine("\(Date().timeIntervalSince1970)\t\(mode)\n", to: path, failure: "schedule_log_failed")
        })
        super.init()
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        let automation = WhoateApp.automation
        switch automation.appearance {
        case .light: NSApp.appearance = NSAppearance(named: .aqua)
        case .dark: NSApp.appearance = NSAppearance(named: .darkAqua)
        case nil: break
        }
        if let sort = automation.sort { model.sort = sort }
        model.start()
        if automation.panelWindow { Task { await showPanelWindow(automation) } }
        if automation.designGallery { showGalleryWindow(automation) }
    }

    /// Puts the panel away after it opened the main window. A MenuBarExtra panel
    /// closes (SwiftUI recreates it on the next click); the automation window only hides.
    func dismissPanel() {
        guard let window = panelWindow else { return }
        if window === automationWindow { window.orderOut(nil) } else { window.close() }
    }

    /// Design gate: every menu bar item state in an ordinary window, so the
    /// screenshot is of whoate's own window, never the system menu bar.
    private func showGalleryWindow(_ automation: Automation) {
        let window = showAutomationWindow("whoate menu bar item", MenuBarGallery(language: model.language))
        if let path = automation.windowFile {
            let size = window.frame.size
            appendLine("gallery\t\(window.windowNumber)\t\(Int(size.width))\t\(Int(size.height))\t"
                + "\(window.backingScaleFactor)\t\(ProcessInfo.processInfo.processIdentifier)\t-\n",
                to: path, failure: "window_file_failed")
        }
    }

    /// Automation timeline, in seconds since the process started. Runs once,
    /// from the menu bar label (the one view that always exists and can open
    /// SwiftUI windows).
    func runAutomation(open: @escaping (String?) -> Void) async {
        let automation = WhoateApp.automation
        guard automation.isActive, !automationStarted else { return }
        automationStarted = true
        openMain = open
        let began = Main.launchedAt
        func at(_ seconds: Double, _ action: @escaping @MainActor () -> Void) {
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(max(0, seconds - (ProcessInfo.processInfo.systemUptime - began))))
                action()
            }
        }
        if automation.mainWindow { at(0.5) { open(nil) } }
        if let select = automation.select { Task { await applySelection(select) } }
        if let again = automation.selectAgain, let after = automation.selectAgainAfter {
            at(after) { [model] in model.select(again) }
        }
        if let after = automation.showMainAfter { at(after) { open(nil) } }
        if let after = automation.showMainAgainAfter { at(after) { open(nil) } }
        if let after = automation.panelShowMainAfter {
            at(after) { [weak self] in
                PanelActions.showMain(nil, open: open, dismiss: { self?.dismissPanel() })
            }
        }
        if automation.mainWindow, let confirm = automation.confirm {
            // Keys are pressed into this confirmation: nothing may reach a process.
            model.kill = { pids, _ in
                Self.killAttempts.add(pids.count)
                let error = try? JSONDecoder().decode(FFIError.self, from: Data(#"{"code":1,"message":"automation: no signal sent"}"#.utf8))
                return (1, .failure(error!))
            }
            at(automation.closeAt ?? 4) { [weak self] in self?.openCloseSheet(confirm, step: automation.closeStep) }
        }
        if let after = automation.escAfter { at(after) { [mainWindows] in Self.press(key: 53, "\u{1b}", in: mainWindows.all.first) } }
        if let after = automation.returnAfter { at(after) { [mainWindows] in Self.press(key: 36, "\r", in: mainWindows.all.first) } }
        if let after = automation.mainCloseAfter { at(after) { [mainWindows] in mainWindows.closeAll() } }
        if let after = automation.quitAfter { at(after) { [quit] in quit() } }
        if let path = automation.stateFile { await recordState(to: path) }
    }

    /// Automation only: a titled window around `root`, in front, sized to fit.
    private func showAutomationWindow(_ title: String, _ root: some View) -> NSWindow {
        let window = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: false)
        window.title = title
        window.contentView = NSHostingView(rootView: root)
        window.setContentSize(window.contentView!.fittingSize)
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        automationWindow = window
        return window
    }

    /// Automation only: waits up to `reportWait` seconds for the first report.
    private func waitForReport() async {
        let deadline = ProcessInfo.processInfo.systemUptime + Self.reportWait
        while model.report == nil, ProcessInfo.processInfo.systemUptime < deadline {
            try? await Task.sleep(for: .milliseconds(100))
        }
    }

    /// WHOATE_UI_SELECT, once the first report is in.
    private func applySelection(_ value: String) async {
        await waitForReport()
        guard model.report != nil else { return }
        model.select(value)
    }

    /// One line whenever the observable window/refresh state changes.
    private func recordState(to path: String) async {
        var last = ""
        while !Task.isCancelled {
            let main = mainWindows.all.first
            let policy = switch NSApp.activationPolicy() {
            case .regular: "regular"
            case .accessory: "accessory"
            default: "prohibited"
            }
            var fields = [
                "policy=\(policy)", "active=\(NSApp.isActive)", "main_windows=\(mainWindows.count)",
                "mode=\(model.fullRefresh ? "full" : "host")",
                "panel_visible=\(panelWindow?.isVisible ?? false)",
                "selected=\(model.selectedOwnerID ?? "-")", "category=\(model.category.rawValue)",
                // Which app has focus: checks tell another app taking it from a key-window bug.
                "frontmost=\(Self.frontmost())",
            ]
            if let main {
                fields += [
                    "key=\(main.isKeyWindow)", "visible=\(main.isVisible)",
                    "occlusion_visible=\(main.occlusionState.contains(.visible))",
                    "hides_on_deactivate=\(main.hidesOnDeactivate)", "level=\(main.level.rawValue)",
                    "behavior=\(main.collectionBehavior.rawValue)", "window=\(main.windowNumber)",
                    // R15: the window's name is the wordmark (Window menu, Mission Control, the drawn title).
                    "title=\(main.title)", "title_shown=\(main.titleVisibility == .visible)",
                ]
                fields += Self.listSelection(main) + Self.toolbarGeometry(main) + Self.closeSheet(main, close: model.close)
            }
            let line = fields.joined(separator: "\t")
            if line != last {
                last = line
                // since_launch lets a checker align its clock with the timers above.
                let since = String(format: "%.3f", ProcessInfo.processInfo.systemUptime - Main.launchedAt)
                appendLine("\(Date().timeIntervalSince1970)\t\(line)\tsince_launch=\(since)\n", to: path,
                           failure: "state_file_failed")
            }
            try? await Task.sleep(for: .milliseconds(200))
        }
    }

    /// Automation only: "self" when whoate is the frontmost app, else "other:<bundle id>".
    static func frontmost() -> String {
        let front = NSWorkspace.shared.frontmostApplication
        if front?.processIdentifier == ProcessInfo.processInfo.processIdentifier { return "self" }
        return "other:" + (front?.bundleIdentifier ?? "-")
    }

    /// Automation only (R9 check): the owner list's selected row, its row count, and
    /// whether the row lies wholly inside the list's visible part. The list is the
    /// second table from the left in the main window; the sidebar is the first.
    static func listSelection(_ window: NSWindow) -> [String] {
        guard let list = listSelectionTable(window) else { return ["list=missing"] }
        let row = list.selectedRow
        let inside = row >= 0 && list.visibleRect.insetBy(dx: -1, dy: -1).contains(list.rect(ofRow: row))
        return ["list_rows=\(list.numberOfRows)", "list_row=\(row)", "list_row_visible=\(inside)"]
    }

    static func listSelectionTable(_ window: NSWindow) -> NSTableView? {
        func tables(_ view: NSView) -> [NSTableView] {
            (view as? NSTableView).map { [$0] } ?? view.subviews.flatMap(tables)
        }
        let found = window.contentView.map(tables)?.sorted { $0.convert($0.bounds, to: nil).minX < $1.convert($1.bounds, to: nil).minX }
        return found.flatMap { $0.count >= 2 ? $0[1] : nil }
    }

    /// Automation only (R13/R15 check): the toolbar's height, the sort control's distance
    /// from the window's right edge (hiding the system title turns the toolbar compact and
    /// pulls that control left), and the title the system draws: whether it is there, how
    /// far it starts from the list column's left edge (an item in front of it pushes it
    /// right) and its vertical offset from the traffic lights.
    static func toolbarGeometry(_ window: NSWindow) -> [String] {
        func first<T: NSView>(_ view: NSView, _ match: (T) -> Bool) -> T? {
            if let found = view as? T, match(found) { return found }
            for sub in view.subviews { if let found = first(sub, match) { return found } }
            return nil
        }
        func frame(_ view: NSView?) -> CGRect? { view.map { $0.convert($0.bounds, to: nil) } }
        guard let frameView = window.contentView?.superview else { return [] }
        let sort = frame(first(frameView) { (_: NSSegmentedControl) in true })
        let title = frame(first(frameView) { (field: NSTextField) in !field.isHidden && field.stringValue == window.title })
        let list = listSelectionTable(window).flatMap { frame($0.enclosingScrollView) }
        let close = frame(window.standardWindowButton(.closeButton))
        return ["toolbar_h=\(Int(window.frame.height - window.contentLayoutRect.height))",
                "sort_right_gap=\(sort.map { "\(Int(window.frame.width - $0.maxX))" } ?? "-")",
                // Review round 8 A:B1: the Memory/CPU control whole and inside the window.
                "sort_w=\(sort.map { "\(Int($0.width))" } ?? "-")", "sort_left=\(sort.map { "\(Int($0.minX))" } ?? "-")",
                "window_w=\(Int(window.frame.width))",
                "title_drawn=\(title != nil)",
                "title_lead=\(title.flatMap { t in list.map { "\(Int(t.minX - $0.minX))" } } ?? "-")",
                "title_dy=\(title.flatMap { t in close.map { "\(Int(abs(t.midY - $0.midY)))" } } ?? "-")"]
    }

    /// Screenshot entry: the same PanelView in an ordinary titled window,
    /// because a MenuBarExtra panel cannot be opened programmatically.
    private func showPanelWindow(_ automation: Automation) async {
        if let after = automation.panelOpenAfter {
            try? await Task.sleep(for: .seconds(max(0, after - (ProcessInfo.processInfo.systemUptime - Main.launchedAt))))
        }
        let window = showAutomationWindow("whoate panel", PanelView(
            model: model, mainWindows: mainWindows,
            openMain: { [weak self] id in
                PanelActions.showMain(id, open: { self?.openMain?($0) }, dismiss: { self?.dismissPanel() })
            },
            quit: quit))
        panelWindow = window
        await waitForReport()
        if let confirm = automation.confirm {
            // "first": the first panel row that can be closed, else the first unprotected owner row.
            let rows = model.rows.filter { $0.source == .owner }
            let id = confirm != "first" ? confirm
                : (rows.first(where: \.closable) ?? rows.first(where: { model.owner($0.id).map { !Panel.isProtected($0) } ?? false }))?.id
            if let id { model.requestClose(id, from: .panel) }
        }
        try? await Task.sleep(for: .milliseconds(600))
        window.setContentSize(window.contentView!.fittingSize)
        if let path = automation.windowFile {
            let size = window.frame.size
            // Last column: the owner whose close confirmation is open, "-" when none.
            let target = if case .confirm(let c) = model.close { c.ownerID } else { "-" }
            let line = "panel\t\(window.windowNumber)\t\(Int(size.width))\t\(Int(size.height))\t"
                + "\(window.backingScaleFactor)\t\(ProcessInfo.processInfo.processIdentifier)\t\(target)\n"
            try? line.write(toFile: path, atomically: true, encoding: .utf8)
        }
        for click in automation.panelDoubleClicks {
            Task { @MainActor [weak self] in
                try? await Task.sleep(for: .seconds(max(0, click.after - (ProcessInfo.processInfo.systemUptime - Main.launchedAt))))
                self?.doubleClick(click, in: window)
            }
        }
        if let after = automation.panelCloseAfter {
            try? await Task.sleep(for: .seconds(after))
            window.orderOut(nil)
        }
    }
}

extension AppDelegate {
    /// Automation only (R14 check): two real clicks posted to the panel window, so
    /// SwiftUI's own double-click recognition decides, aimed at the row's title line
    /// or the pill's centre. A target not laid out is reported, never guessed.
    func doubleClick(_ click: Automation.DoubleClick, in window: NSWindow) {
        guard let view = window.contentView, let frame = RowFrames.panel["\(click.rowID)\t\(click.part)"] else {
            FileHandle.standardError.write(Data("double_click_target_missing\t\(click.rowID)\t\(click.part)\n".utf8))
            return
        }
        // A row is aimed at its title line: past the row's 16 pt side and 12 pt top padding.
        let point = click.part == "pill" ? CGPoint(x: frame.midX, y: frame.midY) : CGPoint(x: frame.minX + 24, y: frame.minY + 20)
        let location = view.convert(point, to: nil)
        for count in 1...2 {
            for type in [NSEvent.EventType.leftMouseDown, .leftMouseUp] {
                guard let event = NSEvent.mouseEvent(
                    with: type, location: location, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                    windowNumber: window.windowNumber, context: nil, eventNumber: 0, clickCount: count,
                    pressure: type == .leftMouseDown ? 1 : 0) else { continue }
                NSApp.postEvent(event, atStart: false)
            }
        }
    }
}

extension AppDelegate {
    /// Automation only (R16): the main window's close confirmation for `id` ("selected":
    /// the selected owner). Step "force" swaps in the second step with every pid still
    /// running; it is made here from the confirmation, and no signal is ever sent.
    func openCloseSheet(_ id: String, step: String?) {
        guard let target = id == "selected" ? model.selectedOwnerID : id else { return }
        model.requestClose(target, from: .main)
        guard step == "force", case .confirm(let c) = model.close else { return }
        let none: [String: Any] = ["unknown_count": 0]
        let json: [String: Any] = [
            "executed": true, "missing_targets": [String](),
            "results": c.pids.map { ["pid": $0, "status": "still_alive"] },
            "memory": ["before": none, "basis": "automation"],
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: json),
              let result = try? JSONDecoder().decode(KillResult.self, from: data) else { return }
        model.close = .result(KillOutcome(code: 0, result: result, language: model.language))
    }

    /// Automation only (R16): kill calls the dry-run sender received (must stay 0).
    nonisolated static let killAttempts = Counter()
    final class Counter: @unchecked Sendable {
        private let lock = NSLock()
        private var value = 0
        func add(_ n: Int) { lock.withLock { value += n } }
        var count: Int { lock.withLock { value } }
    }

    /// Automation only (R16): one key press (Esc, Return) to the main window's sheet.
    static func press(key: UInt16, _ characters: String, in window: NSWindow?) {
        guard let target = window?.attachedSheet ?? window else { return }
        for type in [NSEvent.EventType.keyDown, .keyUp] {
            guard let event = NSEvent.keyEvent(
                with: type, location: .zero, modifierFlags: [], timestamp: ProcessInfo.processInfo.systemUptime,
                windowNumber: target.windowNumber, context: nil, characters: characters,
                charactersIgnoringModifiers: characters, isARepeat: false, keyCode: key) else { continue }
            NSApp.postEvent(event, atStart: false)
        }
    }

    /// Automation only (R16 check): the close step, the dry-run kill count, the dim
    /// layer's frame, and the main window's sheet: its centre's offset from the
    /// window's centre, size, and its buttons left to right (layout ids; SwiftUI
    /// buttons are no NSButtons and expose no accessibility tree without a client).
    static func closeSheet(_ window: NSWindow, close: AppModel.CloseFlow) -> [String] {
        let step = switch close {
        case .idle: "idle"
        case .confirm: "confirm"
        case .running: "running"
        case .result(let o): o.stillAlive.isEmpty ? "result" : "result_still_alive"
        case .forcing: "forcing"
        case .failed: "failed"
        }
        let dim = RowFrames.main["close\tdim"].map { "\(Int($0.minX)),\(Int($0.minY)),\(Int($0.width)),\(Int($0.height))" }
        let common = ["close=\(step)", "kill_attempts=\(killAttempts.count)", "dim=\(dim ?? "-")",
                      "titlebar_dim=\(TitlebarDim.height(window))"]
        guard let sheet = window.attachedSheet else { return common + ["sheet=none"] }
        // Left to right: "esc" is the button bound to Esc, "destructive" the red one.
        let buttons = RowFrames.sheet.filter { $0.key.hasPrefix("closeButton\t") }.sorted { $0.value.minX < $1.value.minX }
            .map { String($0.key.dropFirst("closeButton\t".count)) }
        let (s, w) = (sheet.frame, window.frame)
        return common + ["sheet=\(Int(s.width))x\(Int(s.height))",
                         "sheet_dx=\(Int(s.midX - w.midX))", "sheet_dy=\(Int(s.midY - w.midY))",
                         "sheet_buttons=\(buttons.joined(separator: ","))"]
    }
}

/// Appends to an opt-in automation file, creating it on first use.
func appendLine(_ line: String, to path: String, failure: String) {
    if !FileManager.default.fileExists(atPath: path) { FileManager.default.createFile(atPath: path, contents: nil) }
    do {
        let file = try FileHandle(forWritingTo: URL(fileURLWithPath: path))
        defer { try? file.close() }
        try file.seekToEnd()
        try file.write(contentsOf: Data(line.utf8))
    } catch { FileHandle.standardError.write(Data((failure + "\n").utf8)) }
}

/// Template image plus system-font text; the system colours both for the
/// menu bar's appearance. Nothing here is ever tinted (feedback 5).
struct MenuBarLabel: View {
    let label: MenuBarItem.Label
    var body: some View {
        HStack(spacing: 4) {
            Image(nsImage: MenuBarItem.image(label.image, logo: Self.logo))
            Text(label.text).monospacedDigit()
        }
    }

    /// The bundled template PDF (gallery styles A–C). launch.py wrapper bundles
    /// have no resources; automation may name the PDF. `swift run`: nil.
    static let logo: NSImage? = {
        let bundled = Bundle.main.url(forResource: "menubar-template", withExtension: "pdf")
        let url = bundled ?? WhoateApp.automation.menuBarIcon.map { URL(fileURLWithPath: $0) }
        return url.flatMap(NSImage.init(contentsOf:))
    }()
}

/// Automation only (design-gallery): each alert style against each pressure,
/// on a 24 pt strip in the bar material, as the owner compares them.
struct MenuBarGallery: View {
    let language: Language
    private static let pressures: [(String, Pressure?)] = [
        ("normal", .normal), ("warn", .warn), ("critical", .critical), ("no data", nil),
    ]

    var body: some View {
        Grid(alignment: .leading, horizontalSpacing: 24, verticalSpacing: 12) {
            GridRow {
                Text("")
                ForEach(MenuBarItem.AlertStyle.allCases, id: \.self) { style in
                    Text(Self.title(style)).font(.headline)
                }
            }
            Divider().gridCellColumns(MenuBarItem.AlertStyle.allCases.count + 1)
            ForEach(Array(Self.pressures.enumerated()), id: \.offset) { _, item in
                let (name, pressure) = item
                GridRow {
                    Text(name).font(.callout).foregroundStyle(.secondary)
                    ForEach(MenuBarItem.AlertStyle.allCases, id: \.self) { style in
                        let host = pressure.map(Self.host)
                        MenuBarLabel(label: MenuBarItem.label(host, style: style, language: language))
                            .font(.body)
                            .padding(.horizontal, 8)
                            .frame(height: 24)
                            .background(.bar, in: RoundedRectangle(cornerRadius: 4))
                    }
                }
            }
        }
        .padding(24)
        .fixedSize()
    }

    static func title(_ style: MenuBarItem.AlertStyle) -> String {
        switch style {
        case .symbol: "A · symbol"
        case .prefix: "B · ! prefix"
        case .word: "C · word"
        case .systemSymbol: "D · pie logo"
        }
    }

    /// 41.2G used of 48G, as a decoded host (Host has no public memberwise init).
    static func host(_ pressure: Pressure) -> WhoateKit.Host {
        let json = #"{"mem_total_bytes":51539607552,"mem_used_bytes":44237000000,"compressed_bytes":9663676416,"#
            + #""wired_bytes":5368709120,"swap_used_bytes":6442450944,"pressure":"\#(pressure.rawValue)"}"#
        return try! JSONDecoder().decode(WhoateKit.Host.self, from: Data(json.utf8))
    }
}
