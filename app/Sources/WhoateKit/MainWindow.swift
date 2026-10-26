// Three-column main window: sidebar categories and filters over the same rows
// the panel shows (AppModel.rows), plus the automation selection.
import Foundation

public enum MainCategory: String, CaseIterable, Sendable, Hashable {
    case all, tmux, agents, apps, orphans, system
    case needsAction, growing

    public var isFilter: Bool { self == .needsAction || self == .growing }

    public func title(_ language: Language) -> String {
        switch self {
        case .all: L10n.text(.categoryAll, language)
        case .tmux: "tmux"
        case .agents: L10n.text(.categoryAgents, language)
        case .apps: L10n.text(.categoryApps, language)
        case .orphans: L10n.text(.categoryOrphans, language)
        case .system: L10n.text(.categorySystem, language)
        case .needsAction: L10n.text(.filterNeedsAction, language)
        case .growing: L10n.text(.filterGrowing, language)
        }
    }

    public var symbol: String {
        switch self {
        case .all: "square.stack.3d.up"
        case .tmux: "terminal"
        case .agents: "bubble.left.and.text.bubble.right"
        case .apps: "app.dashed"
        case .orphans: "exclamationmark.triangle"
        case .system: "gearshape.2"
        case .needsAction: "hand.raised"
        case .growing: "chart.line.uptrend.xyaxis"
        }
    }
}

public enum MainWindowModel {
    /// Bundle ids shown as browsers (WHOATE_UI_SELECT=kind:browser, browser details).
    public static let browsers: Set<String> = [
        "com.brave.Browser", "com.google.Chrome", "com.apple.Safari", "org.mozilla.firefox",
        "com.microsoft.edgemac", "company.thebrowser.Browser",
    ]

    public static func isBrowser(_ row: PanelRow) -> Bool {
        row.kind == .app && browsers.contains(String(row.id.dropFirst("app:".count)))
    }

    /// R24 (owner 2026-10-26): AI desktop apps, listed whole under "AI & Agent" with the
    /// sessions instead of under "Apps". The one place this list lives. Each id is the
    /// app's CFBundleIdentifier (`defaults read /Applications/<App>.app/Contents/Info
    /// CFBundleIdentifier`); to add an app, append its id. The owner's first list,
    /// 2026-10-26; ChatGPT ships as com.openai.codex on the owner's Mac.
    public static let aiApps: Set<String> = [
        "com.openai.chat", "com.openai.codex", "com.anthropic.claudefordesktop", "com.anysphere.sand",
        "com.moonshot.kimichat", "com.google.Gemini", "ai.perplexity.mac",
    ]

    public static func isAIApp(_ row: PanelRow) -> Bool {
        row.kind == .app && aiApps.contains(String(row.id.dropFirst("app:".count)))
    }

    /// Owner kinds a category lists; swarms appear under "all" and the two filters only.
    /// Reads the row's own kind and rule: no owner lookup per row.
    public static func matches(_ row: PanelRow, _ category: MainCategory) -> Bool {
        switch category {
        case .all: return true
        case .needsAction: return row.level == .watch || row.level == .act
        case .growing: return row.rule == "fast-growth" || row.rule == "leak-suspect"
        case .tmux: return row.kind == .tmuxPane
        case .agents: return row.kind == .agentSession || isAIApp(row)
        case .apps: return row.kind == .app && !isAIApp(row)
        case .orphans: return row.kind == .orphan
        case .system: return row.kind == .system || row.kind == .service
        }
    }

    /// Sidebar number: for a kind, the footprint of the rows its list shows, added up
    /// like a row's number (R23, owner 2026-10-26: the sidebar and the list use one
    /// measure, so a kind is never smaller than one of its rows); "≥" with the hover
    /// when some processes could not be read. "All" can exceed physical memory, and its
    /// hover says why. Row count for filters. Process groups are an overlay on owners
    /// and never counted twice.
    public static func total(_ rows: [PanelRow], _ category: MainCategory, _ language: Language) -> Format.Memory {
        let shown = rows.filter { matches($0, category) }
        guard !category.isFilter else { return Format.Memory(text: "\(shown.count)", help: nil) }
        let owners = shown.filter { $0.source == .owner }
        var total = Format.memory(owners.reduce(0) { $0 + $1.memBytes }, unknown: owners.reduce(0) { $0 + $1.unknownMemCount },
                                  language)
        if category == .all {
            total.help = ([L10n.text(.memAllHelp, language)] + (total.help.map { [$0] } ?? [])).joined(separator: "\n")
        }
        return total
    }

    /// WHOATE_UI_SELECT: an owner id, kind:orphan|agent|browser, or overview.
    /// An owner id opens the category of its kind (R9, owner 2026-10-24); a
    /// process group or an id not in `rows` falls back to "all".
    public static func select(_ value: String, rows: [PanelRow]) -> (MainCategory, String?) {
        switch value {
        case "overview": return (.all, nil)
        case "kind:orphan": return (.orphans, rows.first { $0.kind == .orphan }?.id)
        case "kind:agent": return (.agents, rows.first { $0.kind == .agentSession }?.id)
        case "kind:browser": return (.apps, rows.first(where: isBrowser)?.id)
        default: return (rows.first { $0.id == value }.flatMap { category(of: $0) } ?? .all, value)
        }
    }

    /// R14 (owner 2026-10-25): what a double-clicked panel row selects in the main
    /// window. The row itself when the list has it; for a process group that is gone,
    /// its member owner with the most memory in total (R14 ruling; since R19 a swarm's
    /// `owners` may also carry each owner's share, not used here); nil (window only)
    /// when neither is listed.
    public static func panelTarget(_ row: PanelRow, rows: [PanelRow]) -> String? {
        if rows.contains(where: { $0.id == row.id }) { return row.id }
        let members = Set(row.members)
        return rows.filter { $0.source == .owner && members.contains($0.id) }.max { $0.memBytes < $1.memBytes }?.id
    }

    /// The kind category that lists `row`; nil for process groups and unknown kinds.
    public static func category(of row: PanelRow) -> MainCategory? {
        [MainCategory.tmux, .agents, .apps, .orphans, .system].first { matches(row, $0) }
    }
}
