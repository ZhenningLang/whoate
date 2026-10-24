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
                ]
                fields += Self.listSelection(main)
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
        func tables(_ view: NSView) -> [NSTableView] {
            (view as? NSTableView).map { [$0] } ?? view.subviews.flatMap(tables)
        }
        guard let content = window.contentView else { return [] }
        let found = tables(content).sorted { $0.convert($0.bounds, to: nil).minX < $1.convert($1.bounds, to: nil).minX }
        guard found.count >= 2 else { return ["list=missing"] }
        let list = found[1], row = list.selectedRow
        let inside = row >= 0 && list.visibleRect.insetBy(dx: -1, dy: -1).contains(list.rect(ofRow: row))
        return ["list_rows=\(list.numberOfRows)", "list_row=\(row)", "list_row_visible=\(inside)"]
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
            if let id { model.requestClose(id) }
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
        if let after = automation.panelCloseAfter {
            try? await Task.sleep(for: .seconds(after))
            window.orderOut(nil)
        }
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
        case .systemSymbol: "D · memorychip"
        }
    }

    /// 41.2G used of 48G, as a decoded host (Host has no public memberwise init).
    static func host(_ pressure: Pressure) -> WhoateKit.Host {
        let json = #"{"mem_total_bytes":51539607552,"mem_used_bytes":44237000000,"compressed_bytes":9663676416,"#
            + #""wired_bytes":5368709120,"swap_used_bytes":6442450944,"pressure":"\#(pressure.rawValue)"}"#
        return try! JSONDecoder().decode(WhoateKit.Host.self, from: Data(json.utf8))
    }
}
