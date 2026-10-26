// Phase 10 round 3, owner check feedback 2026-10-21 14:3x (spec「owner 点验反馈与裁决」):
// footer words (QA37, R1), red close with a count (QA38, R2), a pane's
// sessions (QA39, R3), copy details (QA40, R4), "≥" for unreadable memory
// (QA41, R5), the removed buttons (QA42, R6), Codex sessions (QA43, R7) and
// the rewritten "why it is grouped here" (QA44, R8).
import Foundation
@testable import WhoateKit

enum Phase10Round3Tests {
    static func source(_ file: String) throws -> String {
        String(decoding: try repoFile("app/Sources/Whoate/" + file), as: UTF8.self)
    }
    static func viewSources() throws -> [String: String] {
        let dir = repoRoot.appendingPathComponent("app/Sources/Whoate")
        let names = try FileManager.default.contentsOfDirectory(atPath: dir.path).filter { $0.hasSuffix(".swift") }
        return try Dictionary(uniqueKeysWithValues: names.map { ($0, try source($0)) })
    }
    /// The source between `start` and the next `end`.
    static func slice(_ text: String, from start: String, to end: String) throws -> String {
        try require(text.components(separatedBy: start).dropFirst().first?.components(separatedBy: end).first)
    }
    static func evidence(_ kind: String, _ detail: String, _ confidence: String = "certain") -> [String: Any] {
        ["kind": kind, "detail": detail, "confidence": confidence]
    }
    static func agent(_ tool: String, session: String?, name: String? = nil, cwd: String? = nil,
                      confidence: String = "certain", extra: [String: Any] = [:]) -> [String: Any] {
        var a: [String: Any] = ["tool": tool, "session_id": session as Any? ?? NSNull(), "name": name as Any? ?? NSNull(),
                                "cwd": cwd as Any? ?? NSNull(), "confidence": confidence, "stale": false]
        for (k, v) in extra { a[k] = v }
        return a
    }
    static func proc(_ pid: Int32, _ ppid: Int32, _ comm: String, member: Bool, mem: UInt64? = nil,
                     cwd: String? = nil, argv: [String]? = nil) -> ProcBrief {
        ProcBrief(pid: pid, ppid: ppid, comm: comm, inOwner: member, memBytes: mem, cwd: cwd, argv: argv)
    }

    // QA37
    static func footerSaysQuitAndFullPanel() throws {
        expect(L10n.text(.quit, .zh) == "退出" && L10n.text(.quit, .en) == "Quit")
        for exists in [true, false] {
            expect(WindowPolicy.openLabel(mainWindowExists: exists, .zh) == "完整面板", "\(exists)")
            expect(WindowPolicy.openLabel(mainWindowExists: exists, .en) == "Full Panel", "\(exists)")
        }
        for (file, text) in try viewSources() {
            for old in ["退出 whoate", "显示主窗口", "打开 whoate"] { expect(!text.contains(old), "\(file): \(old)") }
        }
        let footer = try slice(try source("PanelView.swift"), from: "private var footer", to: "private var footerText")
        expect(footer.contains(#"rowFrame("footer", "quit")"#) && footer.contains(#"rowFrame("footer", "showMain")"#),
               "QA17 still measures by layout id")
    }

    // QA38
    static func closeConfirmationCountsProcessesAndTheButtonsAreRed() throws {
        let estimate: [String: Any] = ["bytes": 1288490188, "unknown_count": 0, "basis": "rss", "reason": NSNull()]
        let owner = try decodeValue(Owner.self, ownerJSON("orphan:1", mem: 1, kind: "orphan", pids: [1, 2, 3],
                                                          overrides: ["reclaimable_physical": estimate]))
        let zh = KillConfirmation(owner: owner, language: .zh)
        expect(zh.summary.contains("3 个进程") && zh.summary.contains("1.2G"), zh.summary)
        expect(KillConfirmation(owner: owner, language: .en).summary.contains("3 processes"))
        let unknown = try decodeValue(Owner.self, ownerJSON("orphan:2", mem: 1, kind: "orphan", pids: [1, 2, 3]))
        expect(KillConfirmation(owner: unknown, language: .zh).summary.contains("无法预估"))
        expect(KillConfirmation(owner: unknown, language: .en).summary.contains("cannot estimate"))
        let flow = try source("CloseFlowView.swift")
        expect(flow.contains("Text(c.summary)"), "the confirmation shows the summary")
        let detail = try slice(try source("DetailView.swift"), from: "struct OwnerActions", to: "func copy(")
        let close = try slice(detail, from: ".closeProcesses", to: ".disabled(")
        let row = try slice(try source("PanelView.swift"), from: "openMain(row.id) }", to: ".rowFrame(")
        for (name, text) in [("detail", close), ("panel", row)] {
            expect(text.contains("role: .destructive") && text.contains(".tint(.red)"), name)
        }
        for text in [detail, try source("PanelView.swift")] { expect(!text.contains(".keyboardShortcut(.defaultAction)")) }
        expect(row.contains(".foregroundStyle(.red)"), "a bordered button ignores tint: the panel title is red")
    }

    static func paneReport(sessions: Int = 2) throws -> Report {
        var owners: [[String: Any]] = [
            ownerJSON("pane:%41", mem: 100, kind: "tmux_pane", overrides: ["label": "work:2.1",
                "origin": ["tmux_pane": "%41", "pane_alive": true, "cwd": NSNull(), "ports": NSNull()]]),
            ownerJSON("agent:claude:s1", mem: 300 << 20, kind: "agent_session", overrides: [
                "label": "修登录页", "origin": ["tmux_pane": "%41", "pane_alive": true],
                "agent": agent("claude", session: "s1", name: "修登录页")]),
            ownerJSON("agent:codex:t-a", mem: 500 << 20, kind: "agent_session", overrides: [
                "label": "重构归属", "unknown_mem_count": 2, "origin": ["tmux_pane": "%41", "pane_alive": true],
                "agent": agent("codex", session: "t-a", name: "重构归属")]),
            ownerJSON("agent:claude:s9", mem: 900 << 20, kind: "agent_session", overrides: [
                "label": "别的 pane", "origin": ["tmux_pane": "%42", "pane_alive": true],
                "agent": agent("claude", session: "s9", name: "别的 pane")]),
            ownerJSON("pane:%43", mem: 10, kind: "tmux_pane", overrides: ["label": "idle:0.0",
                "origin": ["tmux_pane": "%43", "pane_alive": true]]),
        ]
        if sessions > 2 {
            owners.append(ownerJSON("agent:opencode:s3", mem: 1 << 20, kind: "agent_session", overrides: [
                "label": "第三个", "origin": ["tmux_pane": "%41", "pane_alive": true],
                "agent": agent("opencode", session: "s3", name: "第三个")]))
        }
        return try decodeValue(Report.self, reportJSON(owners: owners))
    }

    // QA39
    static func paneDetailsListItsSessions() throws {
        let report = try paneReport()
        let pane = try require(report.owners.first { $0.id == "pane:%41" })
        let sessions = Detail.paneSessions(pane, in: report, language: .zh)
        expect(sessions.map(\.ownerID) == ["agent:codex:t-a", "agent:claude:s1"], "\(sessions.map(\.ownerID))")
        expect(sessions.map(\.title) == ["重构归属", "修登录页"] && sessions.map(\.tool) == ["Codex", "Claude Code"])
        expect(sessions.first?.memory.text == "≥ 500M" && sessions.last?.memory.text == "300M")
        let cards = Detail.cards(pane, history: [], catalog: [:], language: .zh, report: report)
        expect(cards.contains(.paneSessions(sessions)))
        // A click selects through the same path as WHOATE_UI_SELECT; since R9 it opens "agent sessions".
        let rows = Phase10Tests.rows(report)
        expect(MainWindowModel.select(sessions[0].ownerID, rows: rows) == (.agents, "agent:codex:t-a"))
        let detail = try source("DetailView.swift")
        expect(detail.contains("Button { select(session.ownerID) }") && detail.contains("select: { model.select($0) }"))
        let subtitle = try require(rows.first { $0.id == "pane:%41" }).subtitle
        expect(subtitle.contains("会话：重构归属、修登录页"), subtitle)
        let three = Phase10Tests.rows(try paneReport(sessions: 3))
        let threeSessions = try require(three.first { $0.id == "pane:%41" }).subtitle
        expect(threeSessions.contains("等 3 个"), threeSessions)
        let idle = try require(report.owners.first { $0.id == "pane:%43" })
        expect(!Detail.cards(idle, history: [], catalog: [:], language: .zh, report: report)
            .contains { if case .paneSessions = $0 { true } else { false } }, "no sessions, no card")
        let idleRow = try require(rows.first { $0.id == "pane:%43" })
        expect(!idleRow.subtitle.contains("会话"))
    }

    // QA40
    static func copiedDetailsCarryFactsButNoConversation() throws {
        let owner = try decodeValue(Owner.self, ownerJSON("agent:claude:s1", mem: 1288490188, cpu: 12.4, level: "watch",
            rule: "leak-suspect", kind: "agent_session", pids: [41001, 41022, 41023], overrides: [
                "label": "修复登录页跳转", "unknown_mem_count": 3,
                "origin": ["tmux_pane": "%41", "pane_alive": true, "cwd": "/Users/demo/Projects/demo", "window_name": "api"],
                "agent": agent("claude", session: "s1", name: "修复登录页跳转",
                               extra: ["first_prompt": "SECRET-PROMPT-1", "first_reply": "SECRET-REPLY-2"]),
                "evidence": [evidence("pane_ancestor", "%41"), evidence("agent_source", "claude")],
                "verdict": ["level": "watch", "rule": "leak-suspect", "text": "30 分钟只涨不降，疑似泄漏"]]))
        var procs = (0..<35).map { i in
            proc(41000 + Int32(i), 40999, "node", member: true, mem: UInt64(100 + i) << 20,
                 cwd: "/Users/demo/Projects/demo", argv: ["node", "~/redacted/path-\(i)", "--port"])
        }
        procs.append(proc(40999, 1, "zsh", member: false))
        let text = CopyInfo.text(owner: owner, procs: procs, report: try Phase10Tests.designReport(), language: .zh,
                                 home: "/Users/demo")
        for want in ["帮我判断保留还是关闭", "软件/会话：Claude Code 会话「修复登录页跳转」", "种类：AI & Agent 会话（claude）",
                     "内存：≥ 1.2G（3 个进程读不到内存", "CPU：12%", "工作目录：~/Projects/demo", "tmux：pane %41，窗口「api」",
                     "归属理由：", "建议：30 分钟只涨不降，疑似泄漏", "进程（3 个）：", "另有 5 个未列出",
                     "41034  node  node ~/redacted/path-34 --port"] {
            expect(text.contains(want), "missing: \(want)\n\(text)")
        }
        expect(!text.contains("SECRET-PROMPT-1") && !text.contains("SECRET-REPLY-2"), "never the conversation")
        expect(text.split(separator: "\n").filter { $0.hasPrefix("  41") }.count == CopyInfo.processLimit)
        let bare = CopyInfo.text(owner: owner, procs: [], report: nil, language: .en, home: "/Users/demo")
        expect(bare.contains("Command lines unavailable") && bare.contains("41001 41022 41023"), bare)
        expect(!bare.contains("SECRET"))
        let actions = try slice(try source("DetailView.swift"), from: "struct OwnerActions", to: "func copy(")
        expect(actions.contains("L10n.text(.copyInfo, language)") && actions.contains("CopyInfo.text("))
    }

    // QA41
    static func unreadableMemoryReadsAsALowerBound() throws {
        let zh = Format.memory(596 << 20, unknown: 3, .zh)
        expect(zh.text == "≥ 596M" && zh.help?.contains("3") == true, "\(zh)")
        expect(Format.memory(596 << 20, unknown: 3, .en).help == "3 processes unreadable")
        expect(Format.memory(596 << 20, unknown: 0, .zh) == Format.Memory(text: "596M", help: nil))
        let panel = try source("PanelView.swift"), main = try source("MainWindowView.swift"), detail = try source("DetailView.swift")
        let value = try slice(panel, from: "struct ValueText", to: "struct MemoryText")
        expect(value.contains("row.memory(language)"), "panel and list rows, detail header: ValueText")
        expect(panel.contains("ValueText(row: row, sort: sort") && main.contains("ValueText(row: row, sort: sort"))
        expect(main.contains("MemoryText(memory: row.memory(language))"), "the list's second line in CPU mode")
        // Conform review B1: the mark follows this tick, not the curve's history.
        var row = try require(Phase10Tests.rows(try Phase10Tests.designReport()).first { $0.id == "app:com.brave.Browser" })
        expect(row.history.contains { ($0.unknownMemCount ?? 0) == 3 }, "the curve was partly unreadable")
        row.unknownMemCount = 0
        expect(row.memory(.zh) == Format.Memory(text: Format.bytes(row.memBytes), help: nil), "readable now: no ≥")
        row.unknownMemCount = 1
        expect(row.memory(.zh).text.hasPrefix("≥ ") && row.memory(.zh).help == "1 个进程读不到内存", "now 1, not the history's 3")
        for file in ["PanelView.swift", "MainWindowView.swift", "DetailView.swift"] {
            let text = try source(file)
            expect(!text.contains("unreadable("), "\(file) uses no history maximum")
        }
        expect(detail.contains("ValueText(row: row, sort: .memory") && detail.contains("Format.memory(part.memBytes"))
        for (file, text) in try viewSources() {
            expect(!text.contains("questionmark") && !text.contains("UnknownMemMark"), file)
        }
    }

    // QA42
    static func onlyThreeDetailButtonsRemain() throws {
        for (file, text) in try viewSources() {
            for gone in [".copyCommand", ".revealInFinder", "activateFileViewerSelecting"] { expect(!text.contains(gone), "\(file) \(gone)") }
        }
        let owner = try decodeValue(Owner.self, ownerJSON("orphan:1", mem: 1, kind: "orphan", overrides: [
            "origin": ["tmux_pane": "%41", "pane_alive": true, "cwd": "/Users/demo/x"]]))
        let labels = Mirror(reflecting: Detail.actions(owner)).children.compactMap(\.label)
        expect(labels == ["jumpShown", "pane", "close", "closeDisabled"], "\(labels)")
        let actions = try slice(try source("DetailView.swift"), from: "struct OwnerActions", to: "func copy(")
        expect(actions.components(separatedBy: "Button(").count - 1 == 3, "jump, copy details, close")
    }

    // QA43 (GUI)
    static func codexSessionsAreAgentRowsAndTheAppGroupsByFolder() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("agent:codex:t-a", mem: 10, kind: "agent_session", overrides: [
                "label": "codex", "agent": agent("codex", session: "t-a", cwd: "/work/demo",
                                                 extra: ["updated_at_ms": 1760000000000, "branch": "main"])]),
            ownerJSON("agent:codex:t-b", mem: 5, kind: "agent_session", overrides: [
                "label": "重构归属", "agent": agent("codex", session: "t-b", name: "重构归属")]),
            ownerJSON("agent:codex:t-c", mem: 1, kind: "agent_session", overrides: [
                "label": "codex", "agent": agent("codex", session: "t-c")]),
        ]))
        let rows = Phase10Tests.rows(report)
        let title = { (id: String) in rows.first { $0.id == id }?.title }
        expect(title("agent:codex:t-a") == "Codex 会话 · demo" && title("agent:codex:t-b") == "重构归属"
               && title("agent:codex:t-c") == "Codex 会话", "\(rows.map(\.title))")
        expect(rows.allSatisfy { MainWindowModel.matches($0, .agents) }, "Codex sessions are agent sessions")
        expect(RowNames.tool("codex") == "Codex")
        let card = Detail.session(report.owners[0], report.owners[0].agent!, .zh)
        expect(card.fields.contains { $0.label == "最近活动" } && card.fields.contains { $0.value.hasSuffix("demo · main") })
        expect(card.prompt == nil && card.reply == nil)

        let app = try decodeValue(Owner.self, ownerJSON("app:com.openai.codex", mem: 1, overrides: ["label": "ChatGPT"]))
        let procs = (0..<10).map { i in
            proc(Int32(100 + i), 1, "node", member: true, mem: UInt64(10 - i) << 20, cwd: "/Users/demo/p\(i)")
        } + [proc(120, 1, "node", member: true, mem: 9 << 20, cwd: "/Users/demo/p0"), proc(1, 0, "launchd", member: false)]
        let groups = Detail.folders(app, procs: procs, language: .zh, home: "/Users/demo")
        expect(groups.count == 9 && groups.last?.folder == "其他", "\(groups.map(\.folder))")
        expect(groups.first == FolderGroup(folder: "~/p0", processes: 2, memBytes: 19 << 20, unknown: 0))
        expect(groups.dropLast().map(\.memBytes) == groups.dropLast().map(\.memBytes).sorted(by: >))
        expect(groups.last == FolderGroup(folder: "其他", processes: 2, memBytes: 3 << 20))
        expect(Detail.folders(app, procs: [], language: .zh).isEmpty, "no procs, no card")
        let other = try decodeValue(Owner.self, ownerJSON("app:com.brave.Browser", mem: 1))
        expect(Detail.folders(other, procs: procs, language: .zh).isEmpty, "only the Codex app")
        expect(Detail.cards(app, history: [], catalog: [:], language: .zh, procs: procs)
            .contains { if case .folders = $0 { true } else { false } })
    }
}
