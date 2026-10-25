// R19 (owner 2026-10-25): a process group spread over many owners, such as the
// WebKit content processes every app opens for its pages, is normal and gives
// nothing to do. The GUI names it, lists who uses it and says what would help.
// The core's level and text for it (verdict/, frozen) are left as they are.
import Foundation

public enum Spread {
    public struct User: Equatable, Sendable {
        /// The owner a click opens; nil for the merged "not recognised" line.
        public var id: String?
        public var name: String
        public var count: Int
        public var memBytes: UInt64? = nil
        public var cpuPct: Double? = nil
    }

    /// The panel and main window rows treat it as normal: not in "needs action", grey pill.
    public static func isSpread(_ s: Swarm) -> Bool { s.shape == .spread }

    /// "com.apple.WebKit" → "网页内容进程（WebKit）"; another bundle-id style label → its last part.
    public static func name(_ s: Swarm, _ language: Language) -> String {
        if s.label == "com.apple.WebKit" { return L10n.text(.webContent, language) }
        return s.label.hasPrefix("com.") ? Detail.plainName(s.label) : s.label
    }

    /// An owner says which app uses the group only when it is in the report, is not a
    /// background service or system process, and is not named after the group itself
    /// (a launchd-adopted WebKit process). Everything else is "not recognised" (spec R19).
    static func recognised(_ o: Owner?, _ s: Swarm) -> Bool {
        guard let o, o.kind != .service, o.kind != .system else { return false }
        return o.label != s.label && !o.label.hasPrefix(s.label + ".")
    }

    /// Who runs the group's processes, most memory first (then most processes). A
    /// recognised owner is its own line and opens; all the others are one line.
    public static func users(_ s: Swarm, owners: [String: Owner], language: Language) -> [User] {
        var named: [User] = []
        var unknown = User(id: nil, name: L10n.text(.spreadUnknown, language), count: 0)
        for member in s.owners {
            if let owner = owners[member.id], recognised(owner, s) {
                named.append(User(id: owner.id, name: RowNames.title(owner, language), count: member.count,
                                  memBytes: member.memBytes, cpuPct: member.cpuPct))
            } else {
                unknown.count += member.count
                if let mem = member.memBytes { unknown.memBytes = (unknown.memBytes ?? 0) + mem }
                if let cpu = member.cpuPct { unknown.cpuPct = (unknown.cpuPct ?? 0) + cpu }
            }
        }
        return (named + (unknown.count > 0 ? [unknown] : [])).sorted {
            ($0.memBytes ?? 0, $0.count, $1.name) > ($1.memBytes ?? 0, $1.count, $0.name)
        }
    }

    /// "Mail、WPS 等 7 个应用各自用 macOS 的网页组件显示内容（另有 3 个进程没认出是哪个
    /// 应用），每个页面或窗口一个进程，一共 16 个，占 5.8G。这是正常现象。" The app count is
    /// the recognised lines of the list; the rest is said apart (review round 7).
    public static func summary(_ s: Swarm, users: [User], language: Language) -> String {
        let apps = users.filter { $0.id != nil }
        let names = apps.prefix(4).map(\.name).joined(separator: language == .zh ? "、" : ", ")
        var who = apps.isEmpty ? L10n.text(.spreadSomeApps, language)
            : apps.count > 4 ? names + L10n.format(.spreadAndMore, language, "\(apps.count)") : names
        // A Latin name before Chinese text gets a space ("Brave Browser 各自用").
        if language == .zh, who.last?.isASCII == true { who += " " }
        // One app "uses"; several "each use"; none recognised says so instead of "more" (review round 7 B).
        let verb = L10n.text(apps.count == 1 ? .spreadVerbOne : .spreadVerbEach, language)
        let unknown = users.first { $0.id == nil }.map {
            apps.isEmpty ? L10n.text(.spreadNoneRecognised, language) : L10n.format(.spreadUnknownClause, language, "\($0.count)")
        } ?? ""
        let memory = Format.bytes(s.memBytes)
        return s.label == "com.apple.WebKit"
            ? L10n.format(.spreadWebKit, language, who, verb, unknown, "\(s.count)", memory)
            : L10n.format(.spreadGeneric, language, who, verb, name(s, language), unknown, "\(s.count)", memory)
    }

    /// The app using the most memory as the example; without memory per app, the general advice.
    public static func advice(_ users: [User], owners: [String: Owner], language: Language) -> String {
        let apps = users.filter { user in user.id.flatMap { owners[$0] }?.kind == .app && (user.memBytes ?? 0) > 0 }
        guard let top = apps.first, let bytes = top.memBytes else { return L10n.text(.spreadAdviceGeneric, language) }
        return L10n.format(.spreadAdvice, language, top.name, Format.bytes(bytes))
    }

    /// Row subtitle in processes, the same unit as the list: "16 个进程：Mail 4、WPS 3、Raycast 3…".
    /// One user only: "74 个进程 · 没认出是哪个应用" (its count would repeat the total).
    public static func subtitle(_ s: Swarm, owners: [String: Owner], language: Language) -> String {
        let users = users(s, owners: owners, language: language)
        if users.count == 1, let only = users.first {
            return L10n.format(.spreadSubtitleOne, language, "\(s.count)", only.name)
        }
        var parts = users.prefix(3).map { "\($0.name) \($0.count)" }
        if users.count > 3, !parts.isEmpty { parts[parts.count - 1] += "…" }
        return L10n.format(.spreadSubtitle, language, "\(s.count)", parts.joined(separator: language == .zh ? "、" : ", "))
    }
}
