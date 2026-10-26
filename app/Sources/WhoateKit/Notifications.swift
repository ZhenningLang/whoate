// Notifications (Phase 11): memory pressure reaching tight or critical, and a row
// starting to grow (the engine's fast-growth / leak-suspect, as the main window's
// "growing" filter). Each kind and object at most once per 30 minutes. Nothing
// leaves the machine; the text names the row and its memory only.
import AppKit
import Foundation
import UserNotifications

public struct AppNotification: Equatable, Sendable {
    public enum Kind: String, Sendable { case pressure, growing }
    public var kind: Kind
    /// Throttle object: the pressure level, or the row id.
    public var object: String
    /// The row to select when clicked; nil for pressure.
    public var ownerID: String?
    public var title: String
    public var body: String
    public var id: String { kind.rawValue + ":" + object }
}

/// Decides what to send; keeps its own throttle and transition state. Pure.
public struct NotificationPolicy: Sendable {
    public static let throttle: Double = 30 * 60
    public var enabled = false
    public var pressure = true
    public var growing = true
    private var lastSent: [String: Double] = [:]
    private var lastPressure: Pressure?
    private var growingRows: Set<String> = []

    public init(enabled: Bool = false, pressure: Bool = true, growing: Bool = true) {
        (self.enabled, self.pressure, self.growing) = (enabled, pressure, growing)
    }

    /// Normal or unknown to tight or critical, or tight to critical (D4 = a: each
    /// level is its own object). The state moves on even when nothing is sent.
    public mutating func host(_ host: Host?, at now: Double, language: Language) -> AppNotification? {
        guard let level = host?.pressure else { return nil }
        let previous = lastPressure
        lastPressure = level
        let rose = switch level {
        case .warn: previous != .warn && previous != .critical
        case .critical: previous != .critical
        default: false
        }
        guard rose, enabled, pressure else { return nil }
        let title = L10n.text(level == .critical ? .notifyCriticalTitle : .notifyTightTitle, language)
        let body = L10n.format(.notifyPressureBody, language, host.map { Format.bytes($0.memUsedBytes) } ?? "—",
                               host.map { Format.bytes($0.memTotalBytes) } ?? "—")
        return send(AppNotification(kind: .pressure, object: level.rawValue, ownerID: nil, title: title, body: body), at: now)
    }

    /// Rows newly in the growing filter; a row that keeps growing is not sent again.
    /// The app passes no rows while this kind is off (it skips building them), so
    /// turning it on sends one for each row already growing.
    public mutating func rows(_ rows: [PanelRow], at now: Double, language: Language) -> [AppNotification] {
        let current = rows.filter { MainWindowModel.matches($0, .growing) }
        let entered = current.filter { !growingRows.contains($0.id) }
        growingRows = Set(current.map(\.id))
        guard enabled, growing else { return [] }
        return entered.compactMap { row in
            let title = L10n.format(.notifyGrowingTitle, language, Self.name(row.title))
            let body = L10n.format(.notifyGrowingBody, language, Format.bytes(row.memBytes))
            return send(AppNotification(kind: .growing, object: row.id, ownerID: row.id, title: title, body: body), at: now)
        }
    }

    private mutating func send(_ note: AppNotification, at now: Double) -> AppNotification? {
        if let last = lastSent[note.id], now - last < Self.throttle { return nil }
        // Rows come and go (panes, sessions): entries past the window throttle nothing.
        lastSent = lastSent.filter { now - $0.value < Self.throttle }
        lastSent[note.id] = now
        return note
    }

    /// Row titles are display names, but a session title is free text: it ends at the
    /// first option or assignment (`--token x`, `KEY=value`), and a path or URL keeps
    /// only its last component (review A: no argv, path or URL in a notification).
    /// A URL is dropped whole: its last component can still carry `user:password@host`
    /// (review A, round 2). Words split on any whitespace, tabs included.
    static func name(_ title: String) -> String {
        let words = title.components(separatedBy: .whitespacesAndNewlines).filter { !$0.isEmpty }
        let kept = words.prefix { !$0.hasPrefix("-") && !$0.contains("=") }.filter { !$0.contains("://") && !$0.contains("@") }
        let name = kept.map { $0.contains("/") ? ($0 as NSString).lastPathComponent : $0 }.joined(separator: " ")
        return name.isEmpty ? "whoate" : name
    }
}

/// Where a clicked notification leads: the main window, with its row selected
/// when that row is still there.
public enum NotificationClick {
    /// Opens the main window once; `rows` are the rows listed now.
    @MainActor public static func open(ownerID: String?, rows: [PanelRow], open: (String?) -> Void) {
        open(ownerID.flatMap { id in rows.contains { $0.id == id } ? id : nil })
    }
}

public enum NotificationAuthorization: String, Sendable { case notDetermined, denied, authorized }

@MainActor public protocol Notifier: AnyObject {
    func authorization() async -> NotificationAuthorization
    func requestAuthorization() async -> NotificationAuthorization
    func post(_ note: AppNotification)
    /// Runs with the clicked notification's owner id (nil for pressure).
    var onClick: ((String?) -> Void)? { get set }
}

/// The real centre, only for a bundled .app with a bundle id: a bare executable
/// (`swift run`) has no centre to talk to (research F3).
@MainActor public final class SystemNotifier: NSObject, Notifier, UNUserNotificationCenterDelegate {
    public var onClick: ((String?) -> Void)?
    private let center: UNUserNotificationCenter

    public static func make(bundle: Bundle = .main) -> SystemNotifier? {
        guard bundle.bundleURL.pathExtension == "app", bundle.bundleIdentifier != nil else { return nil }
        return SystemNotifier(center: .current())
    }

    private init(center: UNUserNotificationCenter) {
        self.center = center
        super.init()
        center.delegate = self
    }

    public func authorization() async -> NotificationAuthorization {
        Self.map(await center.notificationSettings().authorizationStatus)
    }

    public func requestAuthorization() async -> NotificationAuthorization {
        _ = try? await center.requestAuthorization(options: [.alert, .sound])
        return await authorization()
    }

    public func post(_ note: AppNotification) {
        let content = UNMutableNotificationContent()
        content.title = note.title
        content.body = note.body
        content.userInfo = ["owner": note.ownerID ?? ""]
        center.add(UNNotificationRequest(identifier: note.id + ":" + UUID().uuidString, content: content, trigger: nil))
    }

    static func map(_ status: UNAuthorizationStatus) -> NotificationAuthorization {
        switch status {
        case .authorized, .provisional, .ephemeral: .authorized
        case .denied: .denied
        default: .notDetermined
        }
    }

    public nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        let owner = response.notification.request.content.userInfo["owner"] as? String
        await MainActor.run { onClick?(owner.flatMap { $0.isEmpty ? nil : $0 }) }
    }

    public nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter,
                                                   willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .sound]
    }
}

/// Automation and tests: every call is a line in `log` (WHOATE_UI_NOTIFY_LOG);
/// authorization comes from WHOATE_UI_NOTIFY_AUTH (authorized|denied, default authorized).
@MainActor public final class FakeNotifier: Notifier {
    public var onClick: ((String?) -> Void)?
    public private(set) var posted: [AppNotification] = []
    public private(set) var requests = 0
    private var state: NotificationAuthorization
    private let grant: NotificationAuthorization
    private let log: String?

    public init(grant: NotificationAuthorization = .authorized, log: String? = nil) {
        state = .notDetermined
        self.grant = grant
        self.log = log
    }

    public func authorization() async -> NotificationAuthorization { state }
    public func requestAuthorization() async -> NotificationAuthorization {
        requests += 1
        state = grant
        write("request_authorization\t\(grant.rawValue)")
        return state
    }
    public func post(_ note: AppNotification) {
        posted.append(note)
        write("post\t\(note.kind.rawValue)\t\(note.ownerID ?? "-")\t\(note.title)\t\(note.body)")
    }
    /// The real click path, for the latest posted notification of `kind`.
    public func click(latest kind: AppNotification.Kind) {
        guard let note = posted.last(where: { $0.kind == kind }) else { return }
        write("click\t\(note.kind.rawValue)\t\(note.ownerID ?? "-")")
        onClick?(note.ownerID)
    }

    private func write(_ line: String) {
        if let log { appendLine(line + "\n", to: log, failure: "notify_log_failed") }
    }
}

/// The settings tab's switch: asks for permission only when turned on; refused
/// permission turns it back off and says why (spec 通知).
@MainActor public enum NotificationSwitch {
    public static func turnOn(_ notifier: any Notifier) async -> (enabled: Bool, denied: Bool) {
        var status = await notifier.authorization()
        if status == .notDetermined { status = await notifier.requestAuthorization() }
        return status == .authorized ? (true, false) : (false, true)
    }

    /// System Settings' notification page for this app.
    public static func settingsURL(bundleID: String?) -> URL? {
        URL(string: "x-apple.systempreferences:com.apple.Notifications-Settings.extension" + (bundleID.map { "?id=" + $0 } ?? ""))
    }
}
