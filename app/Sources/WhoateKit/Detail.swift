// Main window right column, as data: which cards an owner gets and what they
// say. A card without data is absent. The header carries everything the panel
// row shows (feedback 4), so details never say less than the panel.
import Foundation

public struct DetailHeader: Equatable, Sendable {
    public var kindLine: String
    public var title: String
    public var subtitle: String
    public var pill: String
    public var pillLevel: Level
    public var verdict: String?
}

public struct SessionCard: Equatable, Sendable {
    public struct Field: Equatable, Sendable {
        public var label: String
        public var value: String
    }
    public var fields: [Field]
    public var prompt: String?
    public var reply: String?
    public var confidence: String
}

/// System services: how to restart by hand. Display only, never an action.
public struct SystemCard: Equatable, Sendable {
    public var command: String?
    public var permissions: String
    public var launchd: String
    public var sideEffects: String
    public var safety: String
}

public struct EvidenceLine: Equatable, Sendable {
    public var title: String
    public var detail: String
    public var confidence: String
}

public enum Card: Equatable, Sendable {
    case session(SessionCard)
    case system(SystemCard)
    case evidence([EvidenceLine])
    case curve
    case parts([Part])
    case facts([Fact], linuxNote: Bool)
    case advice([Advice.Item])
}

/// Tmux jump, Finder, close: what the action bar may offer.
public struct DetailActions: Equatable, Sendable {
    /// Sessions always show the jump button; it is greyed when `pane` is nil (U-DEC-6).
    public var jumpShown: Bool
    public var pane: String?
    public var folder: String?
    /// Processes the close button would end.
    public var close: Int
    public var closeDisabled: Bool
}

public enum Detail {
    public static func header(_ owner: Owner, row: PanelRow, language: Language) -> DetailHeader {
        DetailHeader(kindLine: kindLine(owner, language), title: row.title, subtitle: row.subtitle, pill: row.pill,
                     pillLevel: row.pillLevel, verdict: owner.verdict.level == .ok ? nil : owner.verdict.text)
    }

    public static func header(_ swarm: Swarm, row: PanelRow) -> DetailHeader {
        DetailHeader(kindLine: swarm.signature, title: row.title, subtitle: row.subtitle, pill: row.pill,
                     pillLevel: row.pillLevel, verdict: swarm.verdict.level == .ok ? nil : swarm.verdict.text)
    }

    static func kindLine(_ owner: Owner, _ language: Language) -> String {
        if let agent = owner.agent {
            var parts = [L10n.format(.sessionsOf, language, RowNames.tool(agent.tool))]
            parts.append(agent.sessionId.map { String($0.prefix(8)) } ?? L10n.text(.noSession, language))
            return parts.joined(separator: " · ")
        }
        let kind = RowNames.kindWord(owner.kind, language) ?? (owner.kind == .tmuxPane ? L10n.text(.kindPane, language) : "")
        let id = owner.bundleID ?? owner.id
        return [kind, id, L10n.format(.processCount, language, "\(owner.pids.count)")]
            .filter { !$0.isEmpty }.joined(separator: " · ")
    }

    public static func cards(_ owner: Owner, history: [HistoryPoint], catalog: [String: Insight],
                             language: Language) -> [Card] {
        var cards: [Card] = []
        if let agent = owner.agent { cards.append(.session(session(owner, agent, language))) }
        if owner.verdict.rule.hasPrefix("system."), let system = catalog[owner.verdict.rule]?.system {
            cards.append(.system(SystemCard(
                command: system.command, permissions: system.permissions.text(language),
                launchd: system.launchd.text(language), sideEffects: system.sideEffects.text(language),
                safety: system.restartSafety.text(language))))
        }
        if !owner.evidence.isEmpty {
            cards.append(.evidence(owner.evidence.map {
                EvidenceLine(title: Words.evidence($0.kind, language), detail: $0.detail,
                             confidence: Words.confidence($0.confidence, language))
            }))
        }
        if !history.isEmpty { cards.append(.curve) }
        if !owner.parts.isEmpty { cards.append(.parts(owner.parts.sorted { $0.memBytes > $1.memBytes })) }
        if !owner.facts.isEmpty {
            cards.append(.facts(owner.facts, linuxNote: owner.facts.contains { $0.key.hasPrefix("docker") }))
        }
        if !owner.insights.isEmpty { cards.append(.advice(Advice.items(owner.insights, catalog: catalog, language: language))) }
        return cards
    }

    static func session(_ owner: Owner, _ agent: Agent, _ language: Language) -> SessionCard {
        var fields: [SessionCard.Field] = []
        func add(_ key: L10nKey, _ value: String?) {
            if let value, !value.isEmpty { fields.append(.init(label: L10n.text(key, language), value: value)) }
        }
        let auto = L10n.text(.autoNamed, language)
        add(.sessionName, agent.name.map { name in
            agent.nameSource != "derived" ? name : language == .zh ? "\(name)（\(auto)）" : "\(name) (\(auto))"
        } ?? (agent.sessionId == nil ? L10n.text(.noSession, language) : nil))
        add(.status, agent.status.map { Words.status($0, language) })
        if let idle = agent.idleSeconds { add(.idle, Format.duration(idle, language)) }
        if let origin = owner.origin, let pane = origin.tmuxPane {
            var where_ = "tmux \(pane)"
            if let window = origin.windowName { where_ += " · " + Panel.windowLabel(window, origin) }
            if origin.paneAlive == false { where_ += " · " + L10n.text(.paneClosed, language) }
            add(.location, where_)
        }
        if let cwd = owner.origin?.cwd ?? agent.cwd {
            add(.project, Panel.abbreviate(cwd) + (agent.branch.map { " · \($0)" } ?? ""))
        }
        return SessionCard(fields: fields, prompt: agent.firstPrompt, reply: agent.firstReply,
                           confidence: Words.confidence(agent.confidence, language))
    }

    public static func actions(_ owner: Owner) -> DetailActions {
        let protected = Panel.isProtected(owner)
        let pane = owner.origin?.paneAlive == false ? nil : owner.origin?.tmuxPane.flatMap(TmuxJump.validPane)
        let folder = (owner.origin?.cwd ?? owner.agent?.cwd).flatMap { $0 == "/" ? nil : $0 }
        // System processes keep a greyed close button with its reason; the system card has no action.
        return DetailActions(jumpShown: pane != nil || owner.kind == .agentSession, pane: pane, folder: folder,
                             close: owner.pids.count, closeDisabled: protected)
    }

    /// System overview, as label/value lines per section.
    public struct Overview: Equatable, Sendable {
        public var memory: [(String, String)]
        public var zonesTitle: String?
        public var zones: [(String, String)]
        public var gpu: String?
        public var diagnostics: [String]
        public static func == (a: Overview, b: Overview) -> Bool {
            a.memory.map { [$0.0, $0.1] } == b.memory.map { [$0.0, $0.1] } && a.zonesTitle == b.zonesTitle
                && a.zones.map { [$0.0, $0.1] } == b.zones.map { [$0.0, $0.1] } && a.gpu == b.gpu
                && a.diagnostics == b.diagnostics
        }
    }

    public static func overview(_ host: Host, language: Language) -> Overview {
        let memory = [
            (L10n.text(.memoryUsed, language), "\(Format.bytes(host.memUsedBytes)) / \(Format.bytes(host.memTotalBytes))"),
            (L10n.text(.compressed, language), Format.bytes(host.compressedBytes)),
            (L10n.text(.swap, language), Format.bytes(host.swapUsedBytes)),
            (L10n.text(.wired, language), Format.bytes(host.wiredBytes)),
        ]
        var zonesTitle: String?, zones: [(String, String)] = []
        if let k = host.kernelZones {
            zonesTitle = L10n.text(.kernelZones, language) + (k.status == .fresh ? "" : " · " + L10n.text(.notFresh, language))
            zones = k.top.prefix(5).map { ($0.name, Format.bytes($0.bytes)) }
            if zones.isEmpty, let reason = k.reason { zones = [(L10n.text(.unavailable, language), reason)] }
        }
        let gpu = host.gpuMemory.map { g in
            g.status != .missing ? g.bytes.map(Format.bytes) ?? "—"
                : L10n.format(.gpuMissing, language, g.reason ?? "—")
        }
        return Overview(memory: memory, zonesTitle: zonesTitle, zones: zones, gpu: gpu,
                        diagnostics: host.memoryDiagnostics?.map(\.text) ?? [])
    }
}
