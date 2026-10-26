// Phase 11 wiring: the settings window, what each setting changes, notifications,
// first launch, and their automation entries. Automation always gets fake login
// items and notifications, and never the owner's UserDefaults (research F6).
import AppKit
import SwiftUI
import WhoateKit

enum FirstLaunchHint: String { case none = "-", statusItem, banner }

extension AppDelegate {
    /// The status item's window is laid out a moment after the label appears (measured:
    /// 0 pt high at first). ceiling: 12 × 250 ms = 3 s, then the main window's banner.
    static let hintPolls = 12
    static let hintPollInterval = Duration.milliseconds(250)
    /// The popover goes away by itself after this, or at the first click elsewhere.
    static let hintLifetime = Duration.seconds(8)

    static func makeSettingsModel(_ settings: AppSettings, automation: Automation) -> SettingsModel {
        let isApp = Bundle.main.bundleURL.pathExtension == "app"
        let login: any LoginItemService = automation.isActive || !isApp
            ? FakeLoginItem(environment: automation.loginFake) : SystemLoginItem()
        let grant = automation.notifyAuth.flatMap(NotificationAuthorization.init(rawValue:)) ?? .authorized
        // No real centre for a bare executable (`swift run`): a fake that is never granted.
        let real: (any Notifier)? = automation.isActive ? nil : SystemNotifier.make()
        let notifier: any Notifier = real ?? FakeNotifier(grant: automation.isActive ? grant : .denied,
                                                         log: automation.isActive ? automation.notifyLog : nil)
        let cli = automation.isActive
            ? SettingsModel.CLIEnvironment(bundled: automation.cliBundled ?? CommandLineTool.bundledCLI(),
                                           candidates: automation.cliDirs ?? [], pathDirs: automation.cliPath ?? [])
            : SettingsModel.CLIEnvironment(bundled: CommandLineTool.bundledCLI(),
                                           candidates: CommandLineTool.candidates(home: NSHomeDirectory()), pathDirs: nil)
        return SettingsModel(settings: settings, login: login, notifier: notifier, cli: cli)
    }

    /// What each setting changes, at once (spec: 改了立即生效).
    func wireSettings() {
        syncNotificationSwitches()
        settings.changed = { [weak self] key in
            guard let self else { return }
            switch key {
            case .appearance: applyAppearance()
            case .language: model.language = settings.resolvedLanguage()
            case .refreshProfile: model.profile = settings.refreshProfile
            case .notificationsEnabled, .notificationsPressure, .notificationsGrowing: syncNotificationSwitches()
            case .menuBarDisplay, .firstLaunchDone: break
            }
        }
        model.onHost = { [weak self] host in
            guard let self else { return }
            let now = ProcessInfo.processInfo.systemUptime
            if let note = notifications.host(host, at: now, language: model.language) { settingsModel.notifier.post(note) }
        }
        model.onReport = { [weak self] report in
            // Rows only while the growing kind is on, and only built when some owner grows.
            // Off, the growing set is emptied, so turning it on sends one per row growing then.
            guard let self else { return }
            guard notifications.enabled, notifications.growing else { _ = notifications.rows([], at: 0, language: model.language); return }
            let growing = report.owners.contains { MainWindowModel.growingRules.contains($0.verdict.rule) }
            let now = ProcessInfo.processInfo.systemUptime
            for note in notifications.rows(growing ? model.rows(limit: .max) : [], at: now, language: model.language) {
                settingsModel.notifier.post(note)
            }
        }
        settingsModel.notifier.onClick = { [weak self] owner in self?.openFromNotification(owner) }
    }

    private func syncNotificationSwitches() {
        notifications.enabled = settings.notificationsEnabled
        notifications.pressure = settings.notifyPressure
        notifications.growing = settings.notifyGrowing
    }

    /// The main window, with the notification's row selected when it is still listed.
    func openFromNotification(_ ownerID: String?) {
        NotificationClick.open(ownerID: ownerID, rows: model.rows(limit: .max)) { openMainAction?($0) }
    }

    func applyAppearance() {
        let name = AppearanceApplier.appearanceName(setting: settings.appearance, automation: WhoateApp.automation.appearance)
        NSApp.appearance = name.flatMap(NSAppearance.init(named:))
    }

    /// Called by the menu bar label once SwiftUI's openers exist; runs the first launch.
    func connect(open: @escaping (String?) -> Void, openSettings: @escaping () -> Void) {
        openMainAction = open
        openSettingsAction = openSettings
        guard firstLaunch else { return }
        firstLaunch = false
        FirstLaunch.markDone(settings.store)
        Task { @MainActor [weak self] in
            open(nil)
            for _ in 0..<Self.hintPolls {
                try? await Task.sleep(for: Self.hintPollInterval)
                if FirstLaunch.statusItemWindow() != nil { break }
            }
            self?.showFirstLaunchHint()
        }
    }

    /// "whoate 在菜单栏这里" under the menu bar item, or a banner in the main window
    /// when the item cannot be found on screen (research F11).
    func showFirstLaunchHint() {
        guard let item = FirstLaunch.statusItemWindow(), let anchor = item.contentView else {
            model.firstLaunchBanner = true
            hint = .banner
            return
        }
        let popover = NSPopover()
        popover.behavior = .transient
        popover.contentViewController = NSHostingController(rootView:
            Text(L10n.text(.firstLaunchHint, model.language)).font(.callout).padding(12))
        popover.show(relativeTo: anchor.bounds, of: anchor, preferredEdge: .minY)
        hint = .statusItem
        hintWindow = popover.contentViewController?.view.window
        Task { @MainActor in
            try? await Task.sleep(for: Self.hintLifetime)
            popover.close()
        }
    }

    /// The gear, the app menu item and automation all open settings here.
    func showSettings(fromPanel: Bool) {
        openSettingsAction?()
        NSApp.activate(ignoringOtherApps: true)
        if fromPanel { dismissPanel() }
        // SwiftUI shows the same window again on later opens without a new reporter call.
        for delay in Self.reattachDelays {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
                guard let self, let window = settingsWindow, window.isVisible else { return }
                mainWindows.registerSettings(window)
                window.makeKeyAndOrderFront(nil)
                NSApp.activate(ignoringOtherApps: true)
            }
        }
    }

    func settingsWindowFound(_ window: NSWindow) {
        guard settingsWindow !== window else { return }
        settingsWindow = window
        mainWindows.registerSettings(window)
        // Coming back to it re-reads open-at-login and notification permission (research F10).
        settingsKeyToken.map(NotificationCenter.default.removeObserver)
        settingsKeyToken = NotificationCenter.default.addObserver(forName: NSWindow.didBecomeKeyNotification, object: window,
                                                                  queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.settingsModel.reload() }
        }
    }

    /// A page shown for the first time is laid out twice: once under the toolbar's safe
    /// area, where the grouped form measures 10.5 pt taller, then without it. The window
    /// takes the first height and kept it until some later, unrelated update (0.6 s or
    /// never, measured), so a tab opened too tall and then jumped. Asking for a
    /// constraints pass on the next turn makes it take the second height at once.
    func settingsPageResized() {
        DispatchQueue.main.async { [weak self] in self?.settingsWindow?.contentView?.needsUpdateConstraints = true }
    }

    /// WHOATE_UI_OPEN_SETTINGS_AFTER, _SETTINGS_TAB(_STEP), _SETTINGS_CLOSE_AFTER, _NOTIFY_CLICK_AFTER.
    func scheduleSettingsAutomation(_ automation: Automation, at: (Double, @escaping @MainActor () -> Void) -> Void) {
        if let tab = automation.settingsTab.flatMap(SettingsTab.init(rawValue:)) { settingsModel.tab = tab }
        if let after = automation.openSettingsAfter {
            at(after) { [weak self] in self?.showSettings(fromPanel: false) }
            if let step = automation.settingsTabStep {
                for (index, tab) in SettingsTab.allCases.enumerated() {
                    at(after + step * Double(index + 1)) { [weak self] in self?.settingsModel.tab = tab }
                }
            }
        }
        if let after = automation.settingsCloseAfter { at(after) { [weak self] in self?.settingsWindow?.close() } }
        if let after = automation.notifyClickAfter {
            at(after) { [weak self] in (self?.settingsModel.notifier as? FakeNotifier)?.click(latest: .growing) }
        }
    }

    /// Automation state file fields for the settings window and what settings changed.
    func settingsState() -> [String] {
        let window = settingsWindow.flatMap { $0.isVisible ? $0 : nil }
        let label = MenuBarItem.label(model.host, style: .systemSymbol, display: settings.menuBarDisplay, language: model.language)
        return [
            "settings_visible=\(window != nil)", "settings_key=\(window?.isKeyWindow ?? false)",
            "settings_tab=\(settingsModel.tab.rawValue)", "settings_window=\(window.map { "\($0.windowNumber)" } ?? "-")",
            "settings_size=\(window.map { Self.geometry($0, number: false) } ?? "-")",
            "settings_windows=\(mainWindows.settingsCount)",
            "appearance=\(NSApp.appearance?.name.rawValue ?? "system")", "language=\(model.language == .zh ? "zh" : "en")",
            "menubar_text=\(label.text)", "profile=\(model.profile.rawValue)", "store=\(storeKind.rawValue)",
            "login_backend=\(settingsModel.login is FakeLoginItem ? "fake" : "real")",
            "notify_backend=\(settingsModel.notifier is FakeNotifier ? "fake" : "real")",
            "first_launch=\(hint == .none ? "no" : "yes")", "hint=\(hint.rawValue)",
            "status_windows=\(Self.statusWindows())",
            "hint_window=\(hintWindow.flatMap { $0.isVisible ? Self.geometry($0) : nil } ?? "-")",
            "app_menu_settings=\(Self.appMenuSettings())",
            "main_size=\(mainWindows.all.first.map { Self.geometry($0) } ?? "-")",
        ]
    }

    /// "<number>x<width>x<height>x<scale>" (screenshots: which window, what size to expect).
    static func geometry(_ window: NSWindow, number: Bool = true) -> String {
        (number ? "\(window.windowNumber)x" : "") + "\(Int(window.frame.width))x\(Int(window.frame.height))x\(window.backingScaleFactor)"
    }

    /// Automation only (first-launch hint): the status item windows, their frames and the notch areas.
    static func statusWindows() -> String {
        func rect(_ r: NSRect) -> String { "\(Int(r.minX)),\(Int(r.minY)),\(Int(r.width)),\(Int(r.height))" }
        let windows = NSApp.windows.filter { String(describing: type(of: $0)).contains("Status") }
        let items = windows.map { w -> String in
            let aux = w.screen?.auxiliaryTopRightArea.map(rect) ?? "-"
            return "\(type(of: w))@\(rect(w.frame))v\(w.isVisible)aux\(aux)"
        }
        return items.isEmpty ? "-" : items.joined(separator: ";")
    }

    /// The app menu's settings item: "<title>|<modifiers>+<key>", "-" when missing.
    static func appMenuSettings() -> String {
        guard let items = NSApp.mainMenu?.items.first?.submenu?.items,
              let item = items.first(where: { $0.keyEquivalent == "," }) else { return "-" }
        let command = item.keyEquivalentModifierMask.contains(.command) ? "cmd" : "none"
        return "\(item.title)|\(command)+\(item.keyEquivalent)"
    }
}
