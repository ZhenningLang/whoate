// Process environment for the app: PATH repair and the UI automation entry.
import Foundation

/// Finder/launchd starts apps with /usr/bin:/bin:/usr/sbin:/sbin, so the
/// engine's bare `tmux` and `docker` lookups fail. Append (never prepend or
/// replace) the standard Homebrew locations that are missing.
public func completedPath(_ path: String?) -> String {
    var parts = (path ?? "").split(separator: ":", omittingEmptySubsequences: true).map(String.init)
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] where !parts.contains(dir) {
        parts.append(dir)
    }
    return parts.joined(separator: ":")
}

/// Must run before the engine is created.
public func repairProcessPath() {
    setenv("PATH", completedPath(ProcessInfo.processInfo.environment["PATH"]), 1)
}

/// Only active when WHOATE_UI_AUTOMATION is set; otherwise every field is nil.
/// WHOATE_UI_AUTOMATION is a comma list of: dump-tick, panel-window,
/// main-window, no-path-repair, menubar (no extra window; for the timers below).
public struct Automation: Equatable, Sendable {
    public enum Appearance: String, Sendable { case light, dark }
    public var dumpTick = false
    public var panelWindow = false
    public var mainWindow = false
    public var noPathRepair = false
    public var appearance: Appearance?
    /// Owner id to open the close confirmation for, or "first" for the first row.
    public var confirm: String?
    /// File receiving "panel\t<windowNumber>\t<w>\t<h>\t<scale>\t<pid>\t<confirm owner id or ->"
    /// once the panel window is shown.
    public var windowFile: String?
    /// Debug Rust library only: fixture replayed instead of the live engine.
    public var fixture: String?
    public var scheduleLog: String?
    public var panelCloseAfter: Double?
    /// One line per change of activation policy, main windows, key/visible/occlusion or refresh mode.
    public var stateFile: String?
    /// Panel row frames: "<row id>\t<buttons|trend>\t<x>\t<y>\t<w>\t<h>", and
    /// "<row id>\tcurve\t<line|insufficient>\t<epoch written>".
    public var layoutFile: String?
    /// Seconds after the process started: quit through QuitAction, close or (re)show the main window.
    public var quitAfter: Double?
    public var mainCloseAfter: Double?
    public var showMainAfter: Double?
    /// panel-window only: stay hidden this many seconds after start, then open the panel window.
    public var panelOpenAfter: Double?

    public static let off = Automation()

    public init() {}
    public init(environment: [String: String]) {
        guard let modes = environment["WHOATE_UI_AUTOMATION"], !modes.isEmpty else { return }
        let set = Set(modes.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) })
        dumpTick = set.contains("dump-tick")
        panelWindow = set.contains("panel-window")
        mainWindow = set.contains("main-window")
        noPathRepair = set.contains("no-path-repair")
        appearance = environment["WHOATE_UI_APPEARANCE"].flatMap(Appearance.init(rawValue:))
        confirm = environment["WHOATE_UI_CONFIRM"]
        windowFile = environment["WHOATE_UI_WINDOW_FILE"]
        fixture = environment["WHOATE_UI_FIXTURE"]
        scheduleLog = environment["WHOATE_UI_SCHEDULE_LOG"]
        let seconds = { (key: String) in
            environment[key].flatMap(Double.init).flatMap { $0.isFinite && $0 >= 0 ? $0 : nil }
        }
        panelCloseAfter = seconds("WHOATE_UI_PANEL_CLOSE_AFTER")
        stateFile = environment["WHOATE_UI_STATE_FILE"]
        layoutFile = environment["WHOATE_UI_LAYOUT_FILE"]
        quitAfter = seconds("WHOATE_UI_QUIT_AFTER")
        mainCloseAfter = seconds("WHOATE_UI_MAIN_CLOSE_AFTER")
        showMainAfter = seconds("WHOATE_UI_SHOW_MAIN_AFTER")
        panelOpenAfter = seconds("WHOATE_UI_PANEL_OPEN_AFTER")
    }
    public var isActive: Bool { self != .off }
}
