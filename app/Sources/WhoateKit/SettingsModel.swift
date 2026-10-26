// The settings window's state and actions (Phase 11). View state lives here,
// not in @State (no SwiftUI macros under Command Line Tools).
import AppKit
import Foundation
import Observation

public enum SettingsTab: String, CaseIterable, Sendable {
    case general, menubar, notifications, cli, about
    public var title: L10nKey {
        switch self {
        case .general: .tabGeneral
        case .menubar: .tabMenuBar
        case .notifications: .tabNotifications
        case .cli: .tabCLI
        case .about: .tabAbout
        }
    }
    public var symbol: String {
        switch self {
        case .general: "gearshape"
        case .menubar: "menubar.rectangle"
        case .notifications: "bell"
        case .cli: "terminal"
        case .about: "info.circle"
        }
    }
}

@MainActor @Observable
public final class SettingsModel {
    public let settings: AppSettings
    public var tab: SettingsTab = .general
    @ObservationIgnored public let login: any LoginItemService
    @ObservationIgnored public let notifier: any Notifier
    @ObservationIgnored let bundlePath: String
    @ObservationIgnored let info: [String: Any]?
    public private(set) var loginStatus: LoginItemStatus
    public private(set) var loginError: String?
    /// Permission was refused: the reason and a System Settings button show.
    public private(set) var notificationsDenied = false
    /// About: the support QR code shown enlarged, by resource name.
    public var enlargedSupport: String?

    // Command line tab: where to look, filled when the tab is first shown.
    public struct CLIEnvironment: Sendable {
        public var bundled: String?
        public var candidates: [String]
        /// nil: read the login shell's PATH (3 s at most) the first time.
        public var pathDirs: [String]?
        public init(bundled: String?, candidates: [String], pathDirs: [String]?) {
            (self.bundled, self.candidates, self.pathDirs) = (bundled, candidates, pathDirs)
        }
    }
    @ObservationIgnored let cliEnvironment: CLIEnvironment
    public private(set) var cliStatus: CommandLineTool.Status?
    public private(set) var cliPathDirs: [String]?
    /// The login shell's PATH could not be read: the process PATH stands in.
    public private(set) var cliShellFailed = false
    public private(set) var cliInstall: CommandLineTool.InstallResult?

    public init(settings: AppSettings, login: any LoginItemService, notifier: any Notifier,
                bundlePath: String = Bundle.main.bundlePath, info: [String: Any]? = Bundle.main.infoDictionary,
                cli: CLIEnvironment) {
        self.settings = settings
        self.login = login
        self.notifier = notifier
        self.bundlePath = bundlePath
        self.info = info
        cliEnvironment = cli
        loginStatus = login.status
    }

    // MARK: General

    public func loginPresentation(_ language: Language) -> LoginItemPresentation {
        .make(status: loginStatus, bundlePath: bundlePath, error: loginError, language: language)
    }

    public func setLogin(_ on: Bool) {
        loginError = LoginItemAction.set(on, service: login)
        loginStatus = login.status
    }

    /// The window became key again: System Settings may have changed it (research F10).
    public func reload() {
        loginStatus = login.status
        Task { await refreshNotificationAuthorization() }
    }

    // MARK: Notifications

    public func setNotifications(_ on: Bool) async {
        guard on else { settings.notificationsEnabled = false; notificationsDenied = false; return }
        let result = await NotificationSwitch.turnOn(notifier)
        settings.notificationsEnabled = result.enabled
        notificationsDenied = result.denied
    }

    func refreshNotificationAuthorization() async {
        guard settings.notificationsEnabled else { return }
        if await notifier.authorization() == .denied {
            settings.notificationsEnabled = false
            notificationsDenied = true
        }
    }

    // MARK: Command line

    public func refreshCLI() async {
        let env = cliEnvironment
        let path: [String]
        if let known = cliPathDirs ?? env.pathDirs {
            path = known
        } else {
            let read = await CommandLineTool.offPool { CommandLineTool.userPath() }
            cliShellFailed = read == nil
            path = read ?? completedPath(ProcessInfo.processInfo.environment["PATH"]).split(separator: ":").map(String.init)
        }
        cliPathDirs = path
        cliStatus = await CommandLineTool.offPool { CommandLineTool.detect(pathDirs: path, bundled: env.bundled) }
    }

    public func installCLI() async {
        guard let bundled = cliEnvironment.bundled else { return }
        if cliPathDirs == nil { await refreshCLI() }
        let (candidates, path) = (cliEnvironment.candidates, cliPathDirs ?? [])
        cliInstall = await CommandLineTool.offPool { CommandLineTool.install(bundled: bundled, candidates: candidates, pathDirs: path) }
        await refreshCLI()
    }

    public var canCopyGuide: Bool { cliEnvironment.bundled != nil }

    /// "复制给 AI": the bundled CLI's own `agent-guide` output.
    public func agentGuide() async -> String? {
        guard let cli = cliEnvironment.bundled else { return nil }
        return await CommandLineTool.offPool { CommandLineTool.agentGuide(cli: cli) }
    }

    // MARK: About

    public func about(_ language: Language) -> AboutInfo { .make(info: info, language: language) }
}
