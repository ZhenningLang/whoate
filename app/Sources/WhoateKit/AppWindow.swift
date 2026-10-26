// Activation policy, "show main window" and quit. whoate is a menu bar tool
// (LSUIElement, accessory policy); while a main window is open it becomes a
// regular app so the window is reachable from the Dock and ⌘Tab. Nothing here
// depends on what the main window shows.
import AppKit
import Observation
import SwiftUI

public enum WindowPolicy {
    public static func activation(mainWindows: Int) -> NSApplication.ActivationPolicy {
        activation(mainWindows: mainWindows, settingsWindows: 0)
    }

    /// The settings window needs the Dock and ⌘Tab too (research F10), but it is
    /// never a main window: "完整面板" still opens one.
    public static func activation(mainWindows: Int, settingsWindows: Int) -> NSApplication.ActivationPolicy {
        mainWindows > 0 || settingsWindows > 0 ? .regular : .accessory
    }

    public enum OpenAction: Equatable, Sendable { case bringToFront, create }

    /// An existing main window is raised; a second one is never created.
    public static func open(mainWindowExists: Bool) -> OpenAction {
        mainWindowExists ? .bringToFront : .create
    }

    /// R1 (owner 2026-10-21): "完整面板" whether or not the window exists yet.
    public static func openLabel(mainWindowExists: Bool, _ language: Language) -> String {
        L10n.text(mainWindowExists ? .showMain : .openWhoate, language)
    }
}

/// Panel actions that open the main window also put the panel away (feedback 2).
public enum PanelActions {
    @MainActor public static func showMain(_ ownerID: String?, open: (String?) -> Void, dismiss: () -> Void) {
        open(ownerID)
        dismiss()
    }
}

/// The one quit path for the panel button, ⌘Q and automation.
public struct QuitAction: Sendable {
    private let terminate: @MainActor @Sendable () -> Void
    public init(terminate: @escaping @MainActor @Sendable () -> Void = { QuitAction.terminateApp() }) {
        self.terminate = terminate
    }
    /// A sheet on a window (R16's close confirmation) makes NSApp.terminate do nothing
    /// (measured 2026-10-25: a test instance outlived its quit by 30 minutes), so the
    /// sheets go first; dismissing the close sheet cancels the close.
    @MainActor public static func terminateApp() {
        for window in NSApp.windows {
            if let sheet = window.attachedSheet { window.endSheet(sheet) }
        }
        NSApplication.shared.terminate(nil)
    }
    @MainActor public func callAsFunction() { terminate() }
}

/// Tracks open main windows and applies the activation policy when their count
/// changes. `apply` is injected so tests can observe it without a real app.
@MainActor @Observable
public final class MainWindows {
    public private(set) var count = 0
    /// Open settings windows (Phase 11): they count for the policy only.
    public private(set) var settingsCount = 0
    @ObservationIgnored private var windows: [ObjectIdentifier: (window: NSWindow, token: NSObjectProtocol)] = [:]
    @ObservationIgnored private var settings: [ObjectIdentifier: (window: NSWindow, tokens: [NSObjectProtocol])] = [:]
    /// SwiftUI may show the same NSWindow again after it was closed, without the
    /// reporter view moving to a window again (P08-REOPEN-POLICY, measured in Phase 10).
    @ObservationIgnored private weak var closedWindow: NSWindow?
    @ObservationIgnored private let apply: @MainActor (NSApplication.ActivationPolicy) -> Void

    public init(apply: @escaping @MainActor (NSApplication.ActivationPolicy) -> Void = MainWindows.applyToApp) {
        self.apply = apply
    }

    public static func applyToApp(_ policy: NSApplication.ActivationPolicy) {
        guard NSApp.activationPolicy() != policy else { return }
        NSApp.setActivationPolicy(policy)
        if policy == .regular { NSApp.activate(ignoringOtherApps: true) }
    }

    public var exists: Bool { count > 0 }
    public var all: [NSWindow] { windows.values.map(\.window) }

    public func register(_ window: NSWindow) {
        let id = ObjectIdentifier(window)
        guard windows[id] == nil else { return }
        // Explicit: deactivation must never hide the main window.
        window.hidesOnDeactivate = false
        let token = NotificationCenter.default.addObserver(
            forName: NSWindow.willCloseNotification, object: window, queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated { self?.closed(id) }
        }
        windows[id] = (window, token)
        changed()
    }

    public var settingsWindow: NSWindow? { settings.values.first?.window }

    /// SwiftUI's Settings window, tracked until it closes; SwiftUI shows the same
    /// window again next time, which registers it again.
    public func registerSettings(_ window: NSWindow) {
        let id = ObjectIdentifier(window)
        guard settings[id] == nil else { return }
        let gone: @Sendable (Notification) -> Void = { [weak self] _ in
            MainActor.assumeIsolated { self?.settingsGone(id) }
        }
        let tokens = [NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: window,
                                                             queue: .main, using: gone)]
        settings[id] = (window, tokens)
        changed()
    }

    func settingsGone(_ id: ObjectIdentifier) {
        guard let entry = settings.removeValue(forKey: id) else { return }
        entry.tokens.forEach(NotificationCenter.default.removeObserver)
        changed()
    }

    func closed(_ id: ObjectIdentifier) {
        guard let entry = windows.removeValue(forKey: id) else { return }
        closedWindow = entry.window
        NotificationCenter.default.removeObserver(entry.token)
        changed()
    }

    /// Raises the existing main window; false when there is none.
    @discardableResult public func bringToFront() -> Bool {
        guard let window = windows.values.first?.window else { return false }
        apply(.regular)
        if window.isMiniaturized { window.deminiaturize(nil) }
        window.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
        return true
    }

    public func closeAll() { all.forEach { $0.close() } }

    /// After an open: a closed main window that is visible again is tracked again.
    @discardableResult public func reattach() -> Bool {
        guard let window = closedWindow, window.isVisible, windows[ObjectIdentifier(window)] == nil else { return false }
        register(window)
        return true
    }

    private func changed() {
        count = windows.count
        settingsCount = settings.count
        apply(WindowPolicy.activation(mainWindows: count, settingsWindows: settingsCount))
    }
}

/// Reports the NSWindow hosting a view, once it has one.
public struct WindowReporter: NSViewRepresentable {
    private let found: @MainActor (NSWindow) -> Void
    public init(found: @escaping @MainActor (NSWindow) -> Void) { self.found = found }
    public func makeNSView(context: Context) -> ReporterView { ReporterView(found: found) }
    public func updateNSView(_ view: ReporterView, context: Context) {}

    @MainActor public final class ReporterView: NSView {
        private let found: @MainActor (NSWindow) -> Void
        init(found: @escaping @MainActor (NSWindow) -> Void) {
            self.found = found
            super.init(frame: .zero)
        }
        required init?(coder: NSCoder) { fatalError("init(coder:) unavailable") }
        public override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            if let window { found(window) }
        }
    }
}
