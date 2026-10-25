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
/// main-window, design-gallery (menu bar item states in a window),
/// no-path-repair, menubar (no extra window; for the timers below).
public struct Automation: Equatable, Sendable {
    public enum Appearance: String, Sendable { case light, dark }
    public var dumpTick = false
    public var panelWindow = false
    public var mainWindow = false
    public var designGallery = false
    public var noPathRepair = false
    public var appearance: Appearance?
    /// Owner id to open the close confirmation for, or "first" for the first row.
    /// In main-window mode (R16) "selected" means the selected owner, opened at `closeAt`.
    public var confirm: String?
    /// main-window only (R16): when to open the confirmation (seconds since launch);
    /// `closeStep` "force" then shows the second step with every pid still running,
    /// built from the confirmation, no signal sent; `escAfter` and `returnAfter` press
    /// those keys in the sheet. With a main-window confirmation the app sends no signal at all.
    public var closeAt: Double?
    public var closeStep: String?
    public var escAfter: Double?
    public var returnAfter: Double?
    /// main-window only (review round 8 A:B1): the window's content width, to check the
    /// toolbar at the narrowest width the window allows.
    public var mainWidth: Double?
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
    /// Second (re)show of the main window, for close → reopen → regular (P08-REOPEN-POLICY).
    public var showMainAgainAfter: Double?
    /// panel-window only: press the panel's own "show main window" action at this time.
    public var panelShowMainAfter: Double?
    /// Main window selection once data arrives: owner id, kind:orphan|agent|browser, or overview.
    public var select: String?
    /// A second selection through the same path at WHOATE_UI_SELECT_AGAIN_AFTER seconds,
    /// with the window already open: the R9 scroll check (a pane's session clicked).
    public var selectAgain: String?
    public var selectAgainAfter: Double?
    /// main-window screenshots: scroll the details to this card (relation|cpu) once shown.
    public var detailScroll: String?
    /// panel-window and main-window: start in this sort (memory|cpu).
    public var sort: SortKey?
    /// Template PDF for the menu bar logo when the app bundle has none (launch.py wrappers).
    public var menuBarIcon: String?
    /// panel-window only (R14): real double-clicks sent into the panel window, from
    /// WHOATE_UI_PANEL_DOUBLE_CLICKS = "<seconds> <row|pill> <row id>" items joined by ";".
    public struct DoubleClick: Equatable, Sendable {
        public var after: Double
        public var part: String
        public var rowID: String
    }
    public var panelDoubleClicks: [DoubleClick] = []

    public static let off = Automation()

    public init() {}
    public init(environment: [String: String]) {
        guard let modes = environment["WHOATE_UI_AUTOMATION"], !modes.isEmpty else { return }
        let set = Set(modes.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) })
        dumpTick = set.contains("dump-tick")
        panelWindow = set.contains("panel-window")
        mainWindow = set.contains("main-window")
        designGallery = set.contains("design-gallery")
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
        closeAt = seconds("WHOATE_UI_CLOSE_AT")
        closeStep = environment["WHOATE_UI_CLOSE_STEP"]
        escAfter = seconds("WHOATE_UI_ESC_AFTER")
        returnAfter = seconds("WHOATE_UI_RETURN_AFTER")
        mainWidth = seconds("WHOATE_UI_MAIN_WIDTH")
        showMainAfter = seconds("WHOATE_UI_SHOW_MAIN_AFTER")
        panelOpenAfter = seconds("WHOATE_UI_PANEL_OPEN_AFTER")
        showMainAgainAfter = seconds("WHOATE_UI_SHOW_MAIN_AGAIN_AFTER")
        panelShowMainAfter = seconds("WHOATE_UI_PANEL_SHOW_MAIN_AFTER")
        select = environment["WHOATE_UI_SELECT"]
        selectAgain = environment["WHOATE_UI_SELECT_AGAIN"]
        selectAgainAfter = seconds("WHOATE_UI_SELECT_AGAIN_AFTER")
        detailScroll = environment["WHOATE_UI_DETAIL_SCROLL"]
        sort = environment["WHOATE_UI_SORT"].flatMap(SortKey.init(rawValue:))
        menuBarIcon = environment["WHOATE_UI_MENUBAR_ICON"]
        panelDoubleClicks = (environment["WHOATE_UI_PANEL_DOUBLE_CLICKS"] ?? "").split(separator: ";").compactMap { item in
            let fields = item.trimmingCharacters(in: .whitespaces).split(separator: " ", maxSplits: 2).map(String.init)
            guard fields.count == 3, let after = Double(fields[0]), after.isFinite, after >= 0,
                  ["row", "pill"].contains(fields[1]) else { return nil }
            return DoubleClick(after: after, part: fields[1], rowID: fields[2])
        }
    }
    public var isActive: Bool { self != .off }
}
