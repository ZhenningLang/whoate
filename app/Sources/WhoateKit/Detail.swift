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

/// "Why it is grouped here" (R8): one sentence; the parent chain only when the
/// grouping is not obvious; the evidence lines, folded away by the view.
public struct WhyText: Equatable, Sendable {
    public var sentence: String
    public var chain: String?
    public var details: [EvidenceLine]
}

/// An agent session running in a tmux pane (R3); selecting it opens its own row.
public struct PaneSession: Equatable, Sendable {
    public var ownerID: String
    public var title: String
    public var tool: String
    public var memory: Format.Memory
}

/// The Codex app's processes without a session, by working folder (R7 fallback).
public struct FolderGroup: Equatable, Sendable {
    public var folder: String
    public var processes: Int
    public var memBytes: UInt64
    /// Processes whose memory could not be read; memBytes counts the rest (R5).
    public var unknown = 0
}

public enum Card: Equatable, Sendable {
    case session(SessionCard)
    case paneSessions([PaneSession])
    case system(SystemCard)
    case why(WhyText)
    case curve
    case parts([Part])
    case folders([FolderGroup])
    case facts([Fact], linuxNote: Bool)
    case advice([Advice.Item])
}

/// Tmux jump, copy details, close: the only three actions (R6).
public struct DetailActions: Equatable, Sendable {
    /// Sessions always show the jump button; it is greyed when `pane` is nil (U-DEC-6).
    public var jumpShown: Bool
    public var pane: String?
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
        let kind = RowNames.kindLabel(owner.kind, language) ?? ""
        let id = owner.bundleID ?? owner.id
        return [kind, id, L10n.format(.processCount, language, "\(owner.pids.count)")]
            .filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// `procs` and `report` come with the selected owner's whoate_why; without
    /// them the chain, the pane's sessions and the folder groups are absent.
    public static func cards(_ owner: Owner, history: [HistoryPoint], catalog: [String: Insight],
                             language: Language, procs: [ProcBrief] = [], report: Report? = nil) -> [Card] {
        var cards: [Card] = []
        if let agent = owner.agent { cards.append(.session(session(owner, agent, language))) }
        if let report, case let sessions = paneSessions(owner, in: report, language: language), !sessions.isEmpty {
            cards.append(.paneSessions(sessions))
        }
        if owner.verdict.rule.hasPrefix("system."), let system = catalog[owner.verdict.rule]?.system {
            cards.append(.system(SystemCard(
                command: system.command, permissions: system.permissions.text(language),
                launchd: system.launchd.text(language), sideEffects: system.sideEffects.text(language),
                safety: system.restartSafety.text(language))))
        }
        if !owner.evidence.isEmpty { cards.append(.why(why(owner, procs: procs, language: language))) }
        if !history.isEmpty { cards.append(.curve) }
        if !owner.parts.isEmpty { cards.append(.parts(owner.parts.sorted { $0.memBytes > $1.memBytes })) }
        if case let groups = folders(owner, procs: procs, language: language), !groups.isEmpty {
            cards.append(.folders(groups))
        }
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
        // Codex keeps no activity log whoate reads; its session table's last update stands in.
        if agent.tool == "codex", let ms = agent.updatedAtMs {
            add(.lastActive, Format.localTime(Date(timeIntervalSince1970: TimeInterval(ms) / 1000)))
        }
        if let idle = agent.idleSeconds { add(.idle, Format.duration(idle, language)) }
        if let origin = owner.origin, let pane = origin.tmuxPane {
            var where_ = "tmux \(pane)"
            if let window = origin.windowName { where_ += " · " + Panel.windowLabel(window, origin) }
            if origin.paneAlive == false { where_ += " · " + L10n.text(.paneClosed, language) }
            add(.location, where_)
        }
        if let cwd = folder(owner) {
            add(.project, Panel.abbreviate(cwd) + (agent.branch.map { " · \($0)" } ?? ""))
        }
        return SessionCard(fields: fields, prompt: agent.firstPrompt, reply: agent.firstReply,
                           confidence: Words.confidence(agent.confidence, language))
    }

    public static func actions(_ owner: Owner) -> DetailActions {
        let protected = Panel.isProtected(owner)
        let pane = owner.origin?.paneAlive == false ? nil : owner.origin?.tmuxPane.flatMap(TmuxJump.validPane)
        // System processes keep a greyed close button with its reason; the system card has no action.
        return DetailActions(jumpShown: pane != nil || owner.kind == .agentSession, pane: pane,
                             close: owner.pids.count, closeDisabled: protected)
    }

    /// The owner's working folder. A Codex session's own folder comes first: its
    /// tool calls often `cd` into other worktrees (research, round 3 R7).
    public static func folder(_ o: Owner) -> String? {
        (o.agent?.tool == "codex" ? o.agent?.cwd : nil) ?? o.origin?.cwd ?? o.agent?.cwd
    }

    /// Bundle ids of the Codex app (same list as the core's attribute/codex.rs).
    public static let codexApps: Set<String> = ["com.openai.codex", "com.openai.codex.cli"]
    // ceiling: eight folder groups, then "Other"; the card is for spotting the big
    // folders. Raise it if the owner asks to see more.
    public static let folderLimit = 8
    // ceiling: six steps of the parent chain; deeper chains keep both ends.
    public static let chainLimit = 6

    /// Agent sessions whose origin is this pane, largest first (R3). `owners` may be
    /// pre-grouped by pane (RowNames.apply does this once per list).
    public static func paneSessions(_ pane: Owner, among owners: [Owner], language: Language) -> [PaneSession] {
        guard pane.kind == .tmuxPane, let id = pane.origin?.tmuxPane else { return [] }
        return owners
            .filter { $0.kind == .agentSession && $0.origin?.tmuxPane == id }
            .sorted { ($0.memBytes, $1.id) > ($1.memBytes, $0.id) }
            .map { o in
                PaneSession(ownerID: o.id, title: RowNames.title(o, language), tool: RowNames.tool(o.agent?.tool ?? ""),
                            memory: Format.memory(o.memBytes, unknown: o.unknownMemCount, language))
            }
    }
    public static func paneSessions(_ pane: Owner, in report: Report, language: Language) -> [PaneSession] {
        paneSessions(pane, among: report.owners, language: language)
    }

    /// The Codex app's own processes grouped by working folder, largest first;
    /// past `folderLimit` folders the rest is one "Other" group (R7 fallback).
    public static func folders(_ owner: Owner, procs: [ProcBrief], language: Language,
                               home: String = NSHomeDirectory()) -> [FolderGroup] {
        guard let id = owner.bundleID, codexApps.contains(id) else { return [] }
        var groups: [String: FolderGroup] = [:]
        for p in procs where p.inOwner {
            let folder = p.cwd.map { Panel.abbreviate($0, home: home) } ?? "—"
            groups[folder, default: FolderGroup(folder: folder, processes: 0, memBytes: 0)].processes += 1
            groups[folder]!.memBytes += p.memBytes ?? 0
            groups[folder]!.unknown += p.memBytes == nil ? 1 : 0
        }
        let sorted = groups.values.sorted { ($0.memBytes, $1.folder) > ($1.memBytes, $0.folder) }
        guard sorted.count > folderLimit else { return sorted }
        let rest = sorted.dropFirst(folderLimit)
        return Array(sorted.prefix(folderLimit)) + [FolderGroup(
            folder: L10n.text(.other, language), processes: rest.reduce(0) { $0 + $1.processes },
            memBytes: rest.reduce(0) { $0 + $1.memBytes }, unknown: rest.reduce(0) { $0 + $1.unknown })]
    }

    /// R8: an obvious grouping (inside the app, a certain session, a live pane) is one
    /// sentence; anything inferred, weak or adopted also gets the parent chain.
    public static func why(_ owner: Owner, procs: [ProcBrief], language: Language) -> WhyText {
        var seen = Set<String>()
        let details = owner.evidence.filter { seen.insert($0.kind + "\u{0}" + $0.detail).inserted }.map {
            EvidenceLine(title: Words.evidence($0.kind, language), detail: $0.detail,
                         confidence: Words.confidence($0.confidence, language))
        }
        let kinds = Set(owner.evidence.map(\.kind))
        let name = RowNames.title(owner, language)
        let closed = owner.origin?.paneAlive == false
        var obvious = false
        let sentence: String
        if kinds.contains("responsible_launcher") {
            let launcher = owner.agent.map { RowNames.tool($0.tool) } ?? name
            sentence = L10n.format(.whyLauncher, language, launcher)
        } else if let agent = owner.agent, owner.kind == .agentSession {
            let tool = RowNames.tool(agent.tool)
            if agent.sessionId == nil {
                sentence = L10n.format(.whyNoSession, language, tool)
                    + (closed ? (language == .zh ? "；" : "; ") + L10n.text(.whyPaneClosed, language) : "")
            } else {
                obvious = agent.confidence == .certain
                sentence = kinds.contains("codex_thread_env") ? L10n.text(.whyCodex, language)
                    : L10n.format(.whySession, language, tool)
            }
        } else if owner.kind == .tmuxPane, !closed, let pane = owner.origin?.tmuxPane {
            obvious = true
            sentence = L10n.format(.whyPane, language, pane)
        } else if owner.kind == .orphan {
            sentence = L10n.text(.whyOrphan, language)
                + (closed ? (language == .zh ? "；" : "; ") + L10n.text(.whyPaneClosed, language) : "")
        } else if kinds.contains("responsible_app") {
            sentence = L10n.format(.whyResponsible, language, name)
        } else if owner.kind == .app, kinds.contains("app_bundle") {
            obvious = owner.evidence.allSatisfy { $0.kind == "app_bundle" && $0.confidence == .certain }
            sentence = L10n.format(.whyOwnApp, language, name)
        } else {
            sentence = details.first?.title ?? ""
        }
        return WhyText(sentence: sentence, chain: obvious ? nil : chain(owner, procs: procs), details: details)
    }

    /// Top-down parent chain ending at one of the owner's root processes: the
    /// largest root that has a parent below launchd, else the largest root.
    /// Longer than `chainLimit` steps: the first two, "…", the last three.
    public static func chain(_ owner: Owner, procs: [ProcBrief]) -> String? {
        let byPID = Dictionary(procs.map { ($0.pid, $0) }, uniquingKeysWith: { a, _ in a })
        let members = procs.filter(\.inOwner)
        let roots = members.filter { byPID[$0.ppid]?.inOwner != true }
        let ranked = roots.sorted { ($0.memBytes ?? 0, $1.pid) > ($1.memBytes ?? 0, $0.pid) }
        guard let root = ranked.first(where: { byPID[$0.ppid] != nil }) ?? ranked.first else { return nil }
        var names = [root.comm], seen: Set<Int32> = [root.pid], current = root
        while current.ppid > 1, let parent = byPID[current.ppid], seen.insert(parent.pid).inserted {
            names.insert(parent.comm, at: 0)
            current = parent
        }
        if names.count > chainLimit { names = Array(names.prefix(2)) + ["…"] + Array(names.suffix(3)) }
        return names.joined(separator: " → ")
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
