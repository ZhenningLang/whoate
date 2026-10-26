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
    /// R21, owners only: "物理内存约 X · 压缩或 swap 约 Y · 合计（占用）Z", with the
    /// header's "≥" and hover when some processes could not be read.
    public var memorySplit: Format.Memory? = nil
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

/// One line of the process relation tree (R10): a program, what it is for when
/// whoate can tell, and its memory. Where the owner came from has no memory; its
/// parents outside the owner show theirs when read; an exited parent says so.
public struct RelationLine: Equatable, Sendable {
    public var depth: Int
    public var name: String
    public var role: String?
    public var memory: Format.Memory?
    public var exited = false
    /// The owner's only process, below its parents: "← this one".
    public var marked = false
    public init(depth: Int, name: String, role: String? = nil, memory: Format.Memory? = nil,
                exited: Bool = false, marked: Bool = false) {
        (self.depth, self.name, self.role, self.memory, self.exited, self.marked) = (depth, name, role, memory, exited, marked)
    }
}

/// "进程关系" (R10, owner 2026-10-24): a top-down tree from where the owner came
/// from to its processes, past `relationLimit` lines folded into `rest`, and one
/// plain sentence on how whoate decided. No technical identifiers anywhere.
public struct Relation: Equatable, Sendable {
    public var lines: [RelationLine]
    public var rest: [RelationLine]
    /// "其余 N 个进程 共 X" for `rest`; nil when nothing is folded.
    public var restSummary: String?
    public var how: String
    /// inferred or weak: "这是推测，可能不准".
    public var guess: Bool
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
    case relation(Relation)
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
        // R17: git's fsmonitor reads as normal; the plain-words line is ours (the core's verdict table is frozen).
        let advice = RowNames.isGitMonitor(owner) ? L10n.text(.gitMonitorAdvice, language) : nil
        return DetailHeader(kindLine: kindLine(owner, language), title: row.title, subtitle: row.subtitle, pill: row.pill,
                            pillLevel: row.pillLevel,
                            verdict: owner.verdict.level == .ok ? advice : MemoryWording.verdict(owner.verdict.text, language),
                            memorySplit: MemoryWording.splitLine(owner, language))
    }

    /// R19: no raw signature on top; a spread group's plain-words summary is its own card.
    public static func header(_ swarm: Swarm, row: PanelRow, language: Language) -> DetailHeader {
        DetailHeader(kindLine: L10n.format(.processGroupLine, language, "\(swarm.count)"), title: row.title,
                     subtitle: row.subtitle, pill: row.pill, pillLevel: row.pillLevel,
                     verdict: Spread.isSpread(swarm) || swarm.verdict.level == .ok ? nil
                         : MemoryWording.verdict(swarm.verdict.text, language))
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
    /// them the pane's sessions and the folder groups are absent, and the process
    /// relation shows the owner's parts instead of its tree.
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
        if !owner.evidence.isEmpty || !procs.isEmpty {
            cards.append(.relation(relation(owner, procs: procs, report: report, language: language)))
        }
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
    // ceiling: about twelve tree lines (owner 2026-10-24 R10), the rest folded
    // into one line that expands. Raise it if owners ask to see more at once.
    public static let relationLimit = 12

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

    /// R10: where the owner came from (its tmux split, its app, an exited parent),
    /// the parents above its processes, then its processes by parent, largest first.
    /// Same-name siblings without children merge into one "×N" line. Without `procs`
    /// (not fetched yet, or a replay) the owner's parts stand in for the processes.
    public static func relation(_ owner: Owner, procs: [ProcBrief], report: Report?, language: Language) -> Relation {
        let owners = report?.owners ?? []
        var items = origin(owner, owners: owners, language: language).map { Item(line: $0) }
        let members = procs.filter(\.inOwner)
        if members.isEmpty {
            let depth = items.count
            items += owner.parts.sorted { $0.memBytes > $1.memBytes }.map { part in
                Item(line: RelationLine(depth: depth, name: partName(part.name, language)
                                            + (part.processCount > 1 ? " ×\(part.processCount)" : ""),
                                        memory: Format.memory(part.memBytes, unknown: part.unknownMemCount, language)),
                     count: part.processCount, bytes: part.memBytes, unknown: part.unknownMemCount)
            }
        } else {
            items += tree(owner, procs: procs, members: members, owners: owners, depth: items.count, language: language)
        }
        if owner.pids.count == 1, items.count > 1, let index = items.lastIndex(where: { $0.count > 0 }) {
            items[index].line.marked = true
        }
        let (how, guess) = how(owner, owners: owners, language: language)
        guard items.count > relationLimit else {
            return Relation(lines: items.map(\.line), rest: [], restSummary: nil, how: how, guess: guess)
        }
        let rest = items[(relationLimit - 1)...]
        let summary = L10n.format(.relationRest, language, "\(rest.reduce(0) { $0 + $1.count })",
                                  Format.memory(rest.reduce(0) { $0 + $1.bytes }, unknown: rest.reduce(0) { $0 + $1.unknown },
                                                language).text)
        return Relation(lines: items.prefix(relationLimit - 1).map(\.line), rest: rest.map(\.line),
                        restSummary: summary, how: how, guess: guess)
    }

    /// A tree line and the processes it stands for (0 for lines above the owner).
    struct Item {
        var line: RelationLine
        var count = 0, bytes: UInt64 = 0, unknown = 0
    }

    /// The top of the tree: the tmux split the owner runs in, or the app itself.
    static func origin(_ owner: Owner, owners: [Owner], language: Language) -> [RelationLine] {
        if let origin = owner.origin, let pane = origin.tmuxPane {
            if origin.paneAlive == false {
                return [RelationLine(depth: 0, name: L10n.text(.relationPaneClosed, language))]
            }
            // The core's pane address; reports from before it fall back to the pane owner's name.
            let target = origin.tmuxTarget ?? (owner.kind == .tmuxPane ? owner.label
                : owners.first { $0.kind == .tmuxPane && $0.origin?.tmuxPane == pane }?.label)
            let window = origin.windowName.map { L10n.format(.relationWindow, language, $0) } ?? ""
            let split = target.map { L10n.format(.relationPane, language, $0) } ?? L10n.text(.relationPaneOnly, language)
            return [RelationLine(depth: 0, name: split + window)]
        }
        if owner.kind == .app { return [RelationLine(depth: 0, name: L10n.format(.relationApp, language, owner.label))] }
        return []
    }

    /// One step above the owner's processes: a parent outside the owner, or a parent
    /// that has exited (`pid` nil; `ppid` is what its child still records). launchd
    /// adopts children of different parents alike, so an adopted root's exited step
    /// also names that root (`root`) and is never shared with another root.
    struct Above: Hashable {
        var pid: Int32?
        var ppid: Int32
        var root: Int32? = nil
    }

    static func tree(_ owner: Owner, procs: [ProcBrief], members: [ProcBrief], owners: [Owner], depth: Int,
                     language: Language) -> [Item] {
        let byPID = Dictionary(procs.map { ($0.pid, $0) }, uniquingKeysWith: { a, _ in a })
        let inOwner = Set(members.map(\.pid))
        let children = Dictionary(grouping: members.filter { inOwner.contains($0.ppid) }, by: \.ppid)
        let roots = members.filter { !inOwner.contains($0.ppid) }
        let parts = Dictionary(owner.parts.flatMap { part in part.pids.map { ($0, part.name) } }, uniquingKeysWith: { a, _ in a })
        let ownerOf = Dictionary(owners.flatMap { o in o.pids.map { ($0, o) } }, uniquingKeysWith: { a, _ in a })
        let kinds = Set(owner.evidence.map(\.kind))
        // The session's own process: by its tool name when the core resolved it (the
        // kernel names Claude Code by its version), else the session's largest root.
        let sessionRoot: Int32? = owner.agent.flatMap { agent in
            owner.kind != .agentSession ? nil
                : (roots.first { name($0).lowercased().hasPrefix(agent.tool.lowercased()) } ?? roots.max { larger($1, $0) })?.pid
        }

        // Each root's own parents, top-down; the tmux server is the split line at the top.
        // A missing parent has exited. A root adopted by launchd (ppid 1) gets an exited
        // starter only when the owner says so: an orphan, a closed pane, or a launcher
        // that ran it; launchd's own services and an app's main process do not.
        func above(_ root: ProcBrief) -> [Above] {
            var path: [Above] = [], current = root, seen: Set<Int32> = [root.pid]
            while current.ppid > 1 {
                guard let parent = byPID[current.ppid] else { path.append(Above(pid: nil, ppid: current.ppid)); break }
                if parent.comm == "tmux", owner.origin?.tmuxPane != nil { break }
                guard seen.insert(parent.pid).inserted else { break }
                path.append(Above(pid: parent.pid, ppid: parent.ppid))
                current = parent
            }
            let adopted = owner.kind == .orphan || owner.origin?.paneAlive == false
                || (kinds.contains("responsible_launcher") && name(root) != owner.label)
            if path.isEmpty, root.ppid <= 1, adopted { path.append(Above(pid: nil, ppid: root.ppid, root: root.pid)) }
            return path.reversed()
        }

        func role(_ p: ProcBrief) -> String? {
            if p.pid == sessionRoot { return L10n.format(.relationSession, language, RowNames.title(owner, language)) }
            guard let part = parts[p.pid], part.lowercased() != name(p).lowercased(),
                  !["会话", "Session"].contains(part) else { return nil }
            return partName(part, language)
        }
        // Same-parent siblings without children merge by name and role.
        func emit(_ siblings: [ProcBrief], _ depth: Int, into items: inout [Item]) {
            var groups: [[ProcBrief]] = []
            var index: [String: Int] = [:]
            for p in siblings {
                guard children[p.pid] == nil else { groups.append([p]); continue }
                let key = name(p) + "\u{0}" + (role(p) ?? "")
                if let i = index[key] { groups[i].append(p) } else { index[key] = groups.count; groups.append([p]) }
            }
            for group in groups.sorted(by: { (bytes($0), $1[0].pid) > (bytes($1), $0[0].pid) }) {
                let p = group[0], unknown = group.filter { $0.memBytes == nil }.count
                let title = name(p) + (group.count > 1 ? " ×\(group.count)" : "")
                items.append(Item(line: RelationLine(depth: depth, name: title, role: role(p),
                                                     memory: Format.memory(bytes(group), unknown: unknown, language)),
                                  count: group.count, bytes: bytes(group), unknown: unknown))
                if group.count == 1, let kids = children[p.pid] { emit(kids, depth + 1, into: &items) }
            }
        }

        // A forest: roots that share parents share those lines; each parent line lists
        // the roots under it (grouped by their real parent, so never merged across parents).
        let paths = Dictionary(uniqueKeysWithValues: roots.map { ($0.pid, above($0)) })
        func emitLevel(_ prefix: [Above], _ depth: Int, into items: inout [Item]) {
            let here = roots.filter { paths[$0.pid]! == prefix }
            let next = Dictionary(grouping: roots.filter { paths[$0.pid]!.count > prefix.count
                                                           && Array(paths[$0.pid]!.prefix(prefix.count)) == prefix }) {
                paths[$0.pid]![prefix.count]
            }
            var blocks: [(bytes: UInt64, items: [Item])] = []
            if !here.isEmpty {
                var block: [Item] = []
                emit(here, depth, into: &block)
                blocks.append((bytes(here), block))
            }
            for (step, under) in next {
                var block = [Item(line: aboveLine(step, depth: depth, top: prefix.isEmpty))]
                emitLevel(prefix + [step], depth + 1, into: &block)
                blocks.append((bytes(under), block))
            }
            for block in blocks.sorted(by: { $0.bytes > $1.bytes }) { items += block.items }
        }
        func aboveLine(_ step: Above, depth: Int, top: Bool) -> RelationLine {
            guard let pid = step.pid, let parent = byPID[pid] else {
                return RelationLine(depth: depth, name: L10n.text(.relationStarter, language), exited: true)
            }
            // The topmost parent that is an app's process reads as the app.
            if top, let app = ownerOf[pid], app.kind == .app {
                return RelationLine(depth: depth, name: L10n.format(.relationApp, language, app.label))
            }
            return RelationLine(depth: depth, name: name(parent),
                                memory: parent.memBytes.map { Format.memory($0, unknown: 0, language) })
        }
        var items: [Item] = []
        emitLevel([], depth, into: &items)
        return items
    }

    /// The core's readable name (`name`), else the kernel's short `comm`, without a bundle-id prefix.
    static func name(_ p: ProcBrief) -> String { plainName(p.name ?? p.comm) }
    static func bytes(_ group: [ProcBrief]) -> UInt64 { group.reduce(0) { $0 + ($1.memBytes ?? 0) } }

    static func larger(_ a: ProcBrief, _ b: ProcBrief) -> Bool { (a.memBytes ?? 0, b.pid) > (b.memBytes ?? 0, a.pid) }

    /// A recognizer's part name in plain words: "MCP: fetch" → "外接小工具 fetch".
    static func partName(_ name: String, _ language: Language) -> String {
        name.hasPrefix("MCP: ") ? L10n.format(.relationMCP, language, String(name.dropFirst(5))) : plainName(name)
    }

    /// Program names without a bundle-id prefix: "com.apple.WebKit.WebContent" → "WebContent".
    static func plainName(_ name: String) -> String {
        name.hasPrefix("com.") ? String(name.split(separator: ".").last ?? Substring(name)) : name
    }

    /// "怎么判断的" in one plain sentence, and whether it is a guess (the deciding
    /// evidence is inferred or weak). The raw evidence goes only into CopyInfo.
    static func how(_ owner: Owner, owners: [Owner], language: Language) -> (String, Bool) {
        let kinds = Set(owner.evidence.map(\.kind))
        let name = RowNames.title(owner, language)
        let closed = owner.origin?.paneAlive == false
        func weakest(_ wanted: Set<String>?) -> Confidence? {
            let ranks: [Confidence: Int] = [.certain: 0, .unknown: 1, .inferred: 2, .weak: 3]
            return owner.evidence.filter { wanted?.contains($0.kind) ?? true }.map(\.confidence)
                .max { ranks[$0, default: 1] < ranks[$1, default: 1] }
        }
        var sentence: String, decided: Confidence?
        if RowNames.isGitMonitor(owner) {
            // R17: recognised from its own command line, so never a guess.
            sentence = RowNames.gitMonitorRepo(owner).map { L10n.format(.howGitMonitor, language, $0) }
                ?? L10n.text(.howGitMonitorNoRepo, language)
            decided = .certain
        } else if kinds.contains("responsible_launcher") {
            sentence = L10n.format(.howLauncher, language, owner.agent.map { RowNames.tool($0.tool) } ?? name)
            decided = weakest(["responsible_launcher"])
        } else if let agent = owner.agent, owner.kind == .agentSession {
            let tool = RowNames.tool(agent.tool)
            sentence = agent.sessionId == nil ? L10n.format(.howNoSession, language, tool)
                : kinds.contains("codex_thread_env") ? L10n.text(.howCodex, language) : L10n.format(.howSession, language, tool)
            if closed { sentence += L10n.text(.howPaneClosed, language) }
            decided = agent.confidence
        } else if owner.kind == .tmuxPane, !closed {
            sentence = L10n.text(.howPane, language)
            decided = weakest(["pane_ancestor", "tmux_environment"])
        } else if owner.kind == .orphan {
            sentence = L10n.text(.howOrphan, language) + (closed ? L10n.text(.howPaneClosed, language) : "")
            decided = weakest(nil)
        } else if kinds.contains("responsible_app") {
            sentence = L10n.format(.howResponsible, language, name)
            decided = weakest(["responsible_app"])
        } else if owner.kind == .app, kinds.contains("app_bundle") {
            sentence = L10n.format(.howOwnApp, language, name)
            // R22: opened with `open` from a pane; the pane is only where it came from.
            if kinds.contains("tmux_launch") { sentence += L10n.text(.howOpenedFromPane, language) }
            decided = weakest(["app_bundle"])
        } else if kinds.contains("other_uid") {
            sentence = L10n.text(.howOtherUser, language)
            decided = weakest(["other_uid"])
        } else if kinds.contains("service_fallback") {
            sentence = L10n.text(.howService, language)
            decided = weakest(["service_fallback"])
        } else {
            sentence = L10n.text(.howDefault, language)
            decided = weakest(nil)
        }
        return (sentence, decided == .inferred || decided == .weak)
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
        // R21: the label names what each value is (once: the value carries no second label),
        // with its limit and share where there is one.
        let memory = [
            (L10n.text(.memOverviewPhysical, language), MemoryWording.physicalValue(host)),
            (L10n.text(.memOverviewCompressed, language), L10n.format(.memPartOfUsed, language, Format.bytes(host.compressedBytes))),
            (L10n.text(.memOverviewWired, language), L10n.format(.memPartOfUsed, language, Format.bytes(host.wiredBytes))),
            (L10n.text(.swap, language), MemoryWording.swapValue(host) + L10n.text(.memSwapGrows, language)),
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
