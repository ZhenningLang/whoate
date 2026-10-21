// Row names readable without context (app/DESIGN.md §10): a kind word first
// in app subtitles, a distinguishing suffix when two rows share a title, and
// who a same-name process group belongs to. Applied on top of Panel.rows.
import Foundation

public enum RowNames {
    /// Display name of an agent tool id.
    public static func tool(_ tool: String) -> String {
        switch tool {
        case "claude": "Claude Code"
        case "codex": "Codex"
        default: tool
        }
    }

    /// Panel.title, except a Codex session without a name (R7): "Codex 会话 · <folder>".
    public static func title(_ o: Owner, _ language: Language) -> String {
        guard let agent = o.agent, agent.tool == "codex", (agent.name ?? "").isEmpty else { return Panel.title(o) }
        let session = L10n.text(.codexSession, language)
        guard let cwd = Detail.folder(o), cwd != "/" else { return session }
        return session + " · " + URL(fileURLWithPath: cwd).lastPathComponent
    }

    /// "会话：A、B 等 3 个" for a pane running agent sessions (R3); nil without any.
    public static func paneSessionsLine(_ pane: Owner, among owners: [Owner], language: Language) -> String? {
        let titles = Detail.paneSessions(pane, among: owners, language: language).map(\.title)
        guard !titles.isEmpty else { return nil }
        var line = titles.prefix(2).joined(separator: language == .zh ? "、" : ", ")
        if titles.count > 2 { line += " " + L10n.format(.andMore, language, "\(titles.count)") }
        return L10n.format(.paneSessionsLine, language, line)
    }

    /// Kind word, or "tmux pane" for panes; nil for agent sessions.
    public static func kindLabel(_ kind: OwnerKind, _ language: Language) -> String? {
        kindWord(kind, language) ?? (kind == .tmuxPane ? L10n.text(.kindPane, language) : nil)
    }

    public static func kindWord(_ kind: OwnerKind, _ language: Language) -> String? {
        switch kind {
        case .app: L10n.text(.kindApp, language)
        case .orphan: L10n.text(.kindOrphan, language)
        case .service: L10n.text(.kindService, language)
        case .system: L10n.text(.kindSystem, language)
        default: nil
        }
    }

    /// Kind word in app subtitles, unique titles, swarm ownership. Order is kept.
    public static func apply(_ rows: [PanelRow], report: Report, language: Language) -> [PanelRow] {
        let owners = Dictionary(report.owners.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let swarms = Dictionary(report.swarms.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        // Sessions by pane once, not one scan of every owner per pane row.
        let byPane = Dictionary(grouping: report.owners.filter { $0.kind == .agentSession }) { $0.origin?.tmuxPane ?? "" }
        var named = rows.map { row -> PanelRow in
            var row = row
            if let o = owners[row.id] {
                if o.kind == .app, let word = kindWord(.app, language) { row.subtitle = appSubtitle(o, kind: word, language) }
                row.title = title(o, language)
                if o.kind == .tmuxPane, let pane = o.origin?.tmuxPane,
                   let sessions = paneSessionsLine(o, among: byPane[pane] ?? [], language: language) {
                    row.subtitle = row.subtitle.isEmpty ? sessions : row.subtitle + " · " + sessions
                }
            }
            // The title already says "× 74": the subtitle spends its width on the owners.
            if let s = swarms[row.id], let owned = ownership(s, owners: owners, language: language) {
                row.subtitle = owned
            }
            return row
        }
        // Pass 1 adds a readable word; pass 2 adds the id tail (the pid for orphans)
        // to whatever still collides; pass 3 the whole id. Ids are unique, so titles end unique.
        for pass in 1...3 {
            for (_, indices) in Dictionary(grouping: named.indices, by: { named[$0].title }) where indices.count > 1 {
                // The shortest id (com.openai.codex before com.openai.codex.cli) keeps the plain title.
                let sorted = indices.sorted { (named[$0].id.count, named[$0].id) < (named[$1].id.count, named[$1].id) }
                for index in sorted.dropFirst() {
                    let row = named[index]
                    let extra = switch pass {
                    case 1: suffix(row, owner: owners[row.id], language: language)
                    case 2: String(row.id.split(separator: ":").last ?? Substring(row.id))
                    default: row.id
                    }
                    named[index].title += " · " + extra
                }
            }
        }
        return named
    }

    /// Kind word · the largest part · process count: one part only, and without the
    /// app's own name in front ("Brave Browser Helper (Renderer)" → "Helper (Renderer)"),
    /// so the panel's width goes to what differs (design round 1, finding 1).
    static func appSubtitle(_ o: Owner, kind: String, _ language: Language) -> String {
        var parts = [kind]
        if let top = o.parts.max(by: { $0.memBytes < $1.memBytes }), top.name != o.label {
            var part = top
            if part.name.hasPrefix(o.label + " ") { part.name.removeFirst(o.label.count + 1) }
            parts.append(part.displayName)
        }
        if o.pids.count > 1 { parts.append(L10n.format(.processCount, language, "\(o.pids.count)")) }
        return parts.joined(separator: " · ")
    }

    /// Largest part name when it says something new, else the kind word, else the id tail.
    static func suffix(_ row: PanelRow, owner: Owner?, language: Language) -> String {
        if let owner, owner.kind == .app,
           let part = owner.parts.max(by: { $0.memBytes < $1.memBytes })?.name,
           part.lowercased() != row.title.lowercased() {
            return part
        }
        if let owner, owner.kind != .app, let word = kindWord(owner.kind, language) { return word }
        return String(row.id.split(separator: ".").last ?? Substring(row.id))
    }

    /// "分属 22 个归属：Claude Code 会话 15、opencode 会话 4、孤儿 2…" — at most three groups.
    public static func ownership(_ s: Swarm, owners: [String: Owner], language: Language) -> String? {
        guard !s.owners.isEmpty else { return nil }
        var groups: [String: Int] = [:]
        for member in s.owners { groups[group(member.id, owners: owners, language: language), default: 0] += 1 }
        let top = groups.sorted { ($0.value, $1.key) > ($1.value, $0.key) }
        var parts = top.prefix(3).map { "\($0.key) \($0.value)" }
        if top.count > 3 { parts[parts.count - 1] += "…" }
        return L10n.format(.ownedBy, language, "\(s.owners.count)", parts.joined(separator: language == .zh ? "、" : ", "))
    }

    static func group(_ id: String, owners: [String: Owner], language: Language) -> String {
        let fields = id.split(separator: ":", maxSplits: 2).map(String.init)
        switch fields.first {
        case "agent" where fields.count > 1: return L10n.format(.sessionsOf, language, tool(fields[1]))
        case "app": return owners[id]?.label ?? L10n.text(.kindApp, language)
        case "orphan": return L10n.text(.kindOrphan, language)
        case "pane": return "tmux"
        case "service": return L10n.text(.kindService, language)
        default: return L10n.text(.kindSystem, language)
        }
    }
}
