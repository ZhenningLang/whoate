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

    /// Owner kinds a category lists; swarms appear under "all" and the two filters only.
    /// Reads the row's own kind and rule: no owner lookup per row.
    public static func matches(_ row: PanelRow, _ category: MainCategory) -> Bool {
        switch category {
        case .all: return true
        case .needsAction: return row.level == .watch || row.level == .act
        case .growing: return row.rule == "fast-growth" || row.rule == "leak-suspect"
        case .tmux: return row.kind == .tmuxPane
        case .agents: return row.kind == .agentSession
        case .apps: return row.kind == .app
        case .orphans: return row.kind == .orphan
        case .system: return row.kind == .system || row.kind == .service
        }
    }

    /// Sidebar total: bytes for kinds, row count for filters. Process groups are an
    /// overlay on owners (overview: a swarm never changes ownership), so their bytes
    /// are already in the owners' and never counted twice.
    public static func total(_ rows: [PanelRow], _ category: MainCategory) -> String {
        let shown = rows.filter { matches($0, category) }
        guard !category.isFilter else { return "\(shown.count)" }
        return Format.bytes(shown.filter { $0.source == .owner }.reduce(0) { $0 + $1.memBytes })
    }

    /// WHOATE_UI_SELECT: an owner id, kind:orphan|agent|browser, or overview.
    public static func select(_ value: String, rows: [PanelRow]) -> (MainCategory, String?) {
        switch value {
        case "overview": return (.all, nil)
        case "kind:orphan": return (.orphans, rows.first { $0.kind == .orphan }?.id)
        case "kind:agent": return (.agents, rows.first { $0.kind == .agentSession }?.id)
        case "kind:browser": return (.apps, rows.first(where: isBrowser)?.id)
        default: return (.all, value)
        }
    }
}
