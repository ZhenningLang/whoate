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
            PanelView(model: delegate.model, mainWindows: delegate.mainWindows, openMain: open, quit: delegate.quit)
        } label: {
            MenuBarLabel(host: delegate.model.host)
                .task { await delegate.runAutomation(open: open) }
        }
        .menuBarExtraStyle(.window)

        Window(L10n.text(.mainWindowTitle, delegate.model.language), id: "main") {
            MainWindowView(model: delegate.model, mainWindows: delegate.mainWindows)
        }
        .defaultSize(width: 720, height: 560)
        .commands {
            // The regular-policy app menu's ⌘Q goes through the same QuitAction.
            CommandGroup(replacing: .appTermination) {
                Button(L10n.text(.quit, delegate.model.language)) { delegate.quit() }.keyboardShortcut("q")
            }
        }
    }

    /// An existing main window is raised; otherwise SwiftUI creates it.
    private func open(_ ownerID: String?) {
        if let ownerID { delegate.model.selectedOwnerID = ownerID }
        switch WindowPolicy.open(mainWindowExists: delegate.mainWindows.exists) {
        case .bringToFront: delegate.mainWindows.bringToFront()
        case .create:
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
        }
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model: AppModel
    let mainWindows = MainWindows()
    let quit = QuitAction()
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
        model.start()
        if automation.panelWindow { Task { await showPanelWindow(automation) } }
    }

    /// Automation timeline, in seconds since the process started. Runs once,
    /// from the menu bar label (the one view that always exists and can open
    /// SwiftUI windows).
    func runAutomation(open: @escaping (String?) -> Void) async {
        let automation = WhoateApp.automation
        guard automation.isActive, !automationStarted else { return }
        automationStarted = true
        let began = Main.launchedAt
        func at(_ seconds: Double, _ action: @escaping @MainActor () -> Void) {
            Task { @MainActor in
                try? await Task.sleep(for: .seconds(max(0, seconds - (ProcessInfo.processInfo.systemUptime - began))))
                action()
            }
        }
        if automation.mainWindow { at(0.5) { open(nil) } }
        if let after = automation.showMainAfter { at(after) { open(nil) } }
        if let after = automation.mainCloseAfter { at(after) { [mainWindows] in mainWindows.closeAll() } }
        if let after = automation.quitAfter { at(after) { [quit] in quit() } }
        if let path = automation.stateFile { await recordState(to: path) }
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
            ]
            if let main {
                fields += [
                    "key=\(main.isKeyWindow)", "visible=\(main.isVisible)",
                    "occlusion_visible=\(main.occlusionState.contains(.visible))",
                    "hides_on_deactivate=\(main.hidesOnDeactivate)", "level=\(main.level.rawValue)",
                    "behavior=\(main.collectionBehavior.rawValue)", "window=\(main.windowNumber)",
                ]
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

    /// Screenshot entry: the same PanelView in an ordinary titled window,
    /// because a MenuBarExtra panel cannot be opened programmatically.
    private func showPanelWindow(_ automation: Automation) async {
        if let after = automation.panelOpenAfter {
            try? await Task.sleep(for: .seconds(max(0, after - (ProcessInfo.processInfo.systemUptime - Main.launchedAt))))
        }
        let window = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: false)
        window.title = "whoate panel"
        window.contentView = NSHostingView(rootView: PanelView(
            model: model, mainWindows: mainWindows, openMain: { _ in }, quit: quit))
        window.setContentSize(window.contentView!.fittingSize)
        window.center()
        window.makeKeyAndOrderFront(nil)
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        automationWindow = window
        for _ in 0..<100 where model.report == nil { try? await Task.sleep(for: .milliseconds(100)) }
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

/// Template icon plus used memory; tinted only when pressure is not normal.
struct MenuBarLabel: View {
    let host: WhoateKit.Host?
    var body: some View {
        let text = host.map { Format.bytes($0.memUsedBytes) } ?? "—"
        if let color = Palette.pressure(host?.pressure) {
            Image(nsImage: Self.render(text: text, color: color))
        } else {
            HStack(spacing: 4) {
                Image(nsImage: Self.icon)
                Text(text).monospacedDigit()
            }
        }
    }

    static let icon: NSImage = {
        if let url = Bundle.main.url(forResource: "menubar-template", withExtension: "pdf"),
           let image = NSImage(contentsOf: url) {
            image.size = NSSize(width: 18, height: 14)
            image.isTemplate = true
            return image
        }
        // `swift run` has no bundle resources.
        let image = NSImage(systemSymbolName: "memorychip", accessibilityDescription: "whoate")!
        image.isTemplate = true
        return image
    }()

    /// Menu bar labels are drawn as templates; colour needs a non-template image.
    static func render(text: String, color: NSColor) -> NSImage {
        let font = NSFont.monospacedDigitSystemFont(ofSize: 13, weight: .semibold)
        let label = NSAttributedString(string: text, attributes: [.font: font, .foregroundColor: color])
        let iconSize = NSSize(width: 18, height: 14)
        let size = NSSize(width: iconSize.width + 4 + ceil(label.size().width), height: 18)
        let image = NSImage(size: size, flipped: false) { _ in
            let tinted = icon.copy() as! NSImage
            tinted.lockFocus()
            color.set()
            NSRect(origin: .zero, size: tinted.size).fill(using: .sourceAtop)
            tinted.unlockFocus()
            tinted.draw(in: NSRect(x: 0, y: (18 - iconSize.height) / 2, width: iconSize.width, height: iconSize.height))
            label.draw(at: NSPoint(x: iconSize.width + 4, y: (18 - label.size().height) / 2))
            return true
        }
        image.isTemplate = false
        return image
    }
}
