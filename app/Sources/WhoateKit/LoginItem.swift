// Open at login (Phase 11, research F1): SMAppService.mainApp. The system's
// status is the only truth (nothing kept in defaults), so a change made in
// System Settings shows here too. Automation always gets the fake.
import AppKit
import Foundation
import ServiceManagement

public enum LoginItemStatus: String, Sendable {
    case enabled, notRegistered, notFound, requiresApproval
}

@MainActor public protocol LoginItemService: AnyObject {
    var status: LoginItemStatus { get }
    func register() throws
    func unregister() throws
    func openSystemSettings()
}

@MainActor public final class SystemLoginItem: LoginItemService {
    public init() {}
    public var status: LoginItemStatus {
        switch SMAppService.mainApp.status {
        case .enabled: .enabled
        case .requiresApproval: .requiresApproval
        case .notRegistered: .notRegistered
        case .notFound: .notFound
        @unknown default: .notFound
        }
    }
    public func register() throws { try SMAppService.mainApp.register() }
    public func unregister() throws { try SMAppService.mainApp.unregister() }
    public func openSystemSettings() { SMAppService.openSystemSettingsLoginItems() }
}

/// Status from WHOATE_UI_LOGIN_FAKE (enabled|off|approval|error); records each call.
@MainActor public final class FakeLoginItem: LoginItemService {
    public struct Failure: LocalizedError {
        public var errorDescription: String? { "Operation not permitted (fake)" }
    }
    public var status: LoginItemStatus
    public var fails: Bool
    public private(set) var calls: [String] = []
    public init(status: LoginItemStatus = .notFound, fails: Bool = false) {
        self.status = status
        self.fails = fails
    }
    public convenience init(environment value: String?) {
        switch value {
        case "enabled": self.init(status: .enabled)
        case "approval": self.init(status: .requiresApproval)
        case "error": self.init(status: .notRegistered, fails: true)
        default: self.init(status: .notFound)
        }
    }
    public func register() throws {
        calls.append("register")
        if fails { throw Failure() }
        status = .enabled
    }
    public func unregister() throws {
        calls.append("unregister")
        if fails { throw Failure() }
        status = .notRegistered
    }
    public func openSystemSettings() { calls.append("openSystemSettings") }
}

/// What the General tab shows for open-at-login. Pure.
public struct LoginItemPresentation: Equatable, Sendable {
    public var isOn: Bool
    public var canToggle: Bool
    public var message: String?
    public var showsSystemSettingsButton: Bool

    /// `bundlePath` is Bundle.main's; `error` is the last register/unregister failure.
    public static func make(status: LoginItemStatus, bundlePath: String, error: String?, language: Language) -> Self {
        guard bundlePath.hasSuffix(".app") || bundlePath.hasSuffix(".app/") else {
            return Self(isOn: false, canToggle: false, message: L10n.text(.loginOnlyApp, language), showsSystemSettingsButton: false)
        }
        // An unnotarized app opened from a quarantined download can run from a random
        // read-only copy; registering that path would point at nothing after logout.
        if bundlePath.contains("/AppTranslocation/") {
            return Self(isOn: false, canToggle: false, message: L10n.text(.loginTranslocated, language), showsSystemSettingsButton: false)
        }
        var lines: [String] = []
        let isOn = status == .enabled || status == .requiresApproval
        switch status {
        case .enabled: lines.append(L10n.text(.loginOn, language))
        case .notRegistered, .notFound: lines.append(L10n.text(.loginOff, language))
        case .requiresApproval: lines.append(L10n.text(.loginNeedsApproval, language))
        }
        if let error { lines.append(L10n.format(.loginFailed, language, error)) }
        // The login item records the bundle's absolute path (research F1).
        if !bundlePath.hasPrefix("/Applications/") { lines.append(L10n.text(.loginMoveNote, language)) }
        return Self(isOn: isOn, canToggle: true, message: lines.joined(separator: "\n"),
                    showsSystemSettingsButton: status == .requiresApproval)
    }
}

/// The toggle's action: register or unregister, then the status is read back from
/// the service (never assumed), with the error text when it threw.
@MainActor public enum LoginItemAction {
    public static func set(_ on: Bool, service: any LoginItemService) -> String? {
        do {
            if on { try service.register() } else { try service.unregister() }
            return nil
        } catch {
            return error.localizedDescription
        }
    }
}
