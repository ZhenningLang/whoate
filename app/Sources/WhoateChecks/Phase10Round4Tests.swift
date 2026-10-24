// Phase 10 round 4 (owner check 2026-10-24, spec「owner 点验反馈与裁决」R9–R12):
// QA49 jump to a row's category and scroll to it, QA50 the process relation tree
// in plain words, QA51 the CPU curve in details, QA52 CPU as a lower bound.
import Foundation
@testable import WhoateKit

enum Phase10Round4Tests {
    typealias R3 = Phase10Round3Tests

    // QA49
    static func jumpOpensTheRowsCategory() throws {
        let report = try Phase10Tests.designReport()
        let rows = Phase10Tests.rows(report)
        let swarm = try require(rows.first { $0.source == .swarm }).id
        let expected: [(String, MainCategory)] = [
            ("agent:claude:5d1e8a90-2c4b-4f7e-8a1d-6b3c9e0f2a17", .agents), ("orphan:948211", .orphans),
            ("pane:%22", .tmux), ("app:com.brave.Browser", .apps), ("system:312", .system),
            ("service:com.apple.mds_stores", .system), (swarm, .all), ("app:not.in.the.list", .all),
        ]
        for (id, category) in expected {
            expect(MainWindowModel.select(id, rows: rows) == (category, id), "\(id) → \(MainWindowModel.select(id, rows: rows))")
        }
        for row in rows {
            expect(MainWindowModel.matches(row, MainWindowModel.select(row.id, rows: rows).0), "\(row.id) listed in its category")
        }
        // Panel rows and the pane card go through AppModel.select; the list scrolls on its request only.
        let app = try R3.source("WhoateApp.swift"), main = try R3.source("MainWindowView.swift")
        expect(app.contains("if let ownerID { delegate.model.select(ownerID) }") && !app.contains("model.selectedOwnerID = ownerID"))
        expect(main.contains(".onChange(of: model.scrollRequest)") && main.contains("proxy.scrollTo(request.id, anchor: .center)"))
    }

    // QA49
    @MainActor static func onlyWhoatesSelectionsScroll() async throws {
        let spy = Phase08Tests.HistorySpy(try Phase10Tests.designReport(), raw: [])
        let model = AppModel(language: .zh, makeEngine: { spy }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             sleep: { _ in try await Task.sleep(for: .milliseconds(5)) })
        model.setVisible(main: true)
        model.start()
        for _ in 0..<200 where model.report == nil { try? await Task.sleep(for: .milliseconds(5)) }
        model.stop()
        model.selectedOwnerID = "orphan:948211"
        expect(model.scrollRequest == nil, "a click in the list never scrolls")
        let session = "agent:opencode:ses_Lk92wQ"
        model.select(session)
        expect(model.category == .agents && model.selectedOwnerID == session)
        expect(model.scrollRequest == AppModel.ScrollRequest(id: session, serial: 1))
        model.select(session)
        expect(model.scrollRequest?.serial == 2, "the same row asked twice scrolls twice")
    }

    static func line(_ depth: Int, _ name: String, role: String? = nil, mem: String? = nil, exited: Bool = false,
                     marked: Bool = false) -> RelationLine {
        RelationLine(depth: depth, name: name, role: role, memory: mem.map { Format.Memory(text: $0, help: nil) },
                     exited: exited, marked: marked)
    }

    /// A whoate_why process with the kernel's short `comm` and the core's readable `name`.
    static func real(_ pid: Int32, _ ppid: Int32, comm: String, name: String, member: Bool, mem: UInt64? = nil) -> ProcBrief {
        ProcBrief(pid: pid, ppid: ppid, comm: comm, name: name, inOwner: member, memBytes: mem)
    }

    // QA50: the owner's two sketches (spec R10) become these trees. Process names
    // have the real machine's shape (review B:B1, round 4): Claude Code's kernel
    // name is its version, and the core's `name` gives "claude" back.
    static func relationIsAPlainTree() throws {
        let session = try decodeValue(Owner.self, ownerJSON("agent:claude:s1", mem: 361 << 20, kind: "agent_session",
            pids: [102, 103], overrides: [
                "label": "自动化评估设计与爬山优化博文",
                "origin": ["tmux_pane": "%216", "pane_alive": true, "window_name": "gzh"],
                "agent": R3.agent("claude", session: "s1", name: "自动化评估设计与爬山优化博文"),
                "parts": [["name": "MCP: fetch", "mem_bytes": 47 << 20, "process_count": 1, "pids": [103], "unknown_mem_count": 0]],
                "evidence": [R3.evidence("pane_ancestor", "%216"), R3.evidence("agent_source", "claude")]]))
        let pane = try decodeValue(Owner.self, ownerJSON("pane:%216", mem: 12 << 20, kind: "tmux_pane", pids: [101], overrides: [
            "label": "main:5.2", "origin": ["tmux_pane": "%216", "pane_alive": true, "window_name": "gzh"]]))
        let report = try decodeValue(Report.self, reportJSON(owners: []))
        var withPane = report
        withPane.owners = [pane, session]
        let procs = [R3.proc(100, 1, "tmux", member: false), R3.proc(101, 100, "zsh", member: false, mem: 12 << 20),
                     real(102, 101, comm: "2.1.306", name: "claude", member: true, mem: 314 << 20),
                     R3.proc(103, 102, "node", member: true, mem: 47 << 20)]
        let tree = Detail.relation(session, procs: procs, report: withPane, language: .zh)
        expect(tree.lines == [line(0, "tmux 分屏 main:5.2（gzh 窗口）"), line(1, "zsh", mem: "12M"),
                              line(2, "claude", role: "「自动化评估设计与爬山优化博文」", mem: "314M"),
                              line(3, "node", role: "外接小工具 fetch", mem: "47M")], "\(tree.lines)")
        expect(tree.how == "这些进程都是这个 Claude Code 会话启动的。" && !tree.guess && tree.restSummary == nil, tree.how)

        // ChatGPT ran node through a codex that has exited: adopted, inferred.
        let adopted = try decodeValue(Owner.self, ownerJSON("app:com.openai.codex", mem: 9 << 30, pids: [7], overrides: [
            "label": "ChatGPT", "evidence": [R3.evidence("responsible_launcher", "com.openai.codex", "inferred")]]))
        let chatgpt = Detail.relation(adopted, procs: [R3.proc(7, 1, "node", member: true, mem: 9 << 30)], report: report,
                                      language: .zh)
        expect(chatgpt.lines == [line(0, "ChatGPT（桌面应用）"), line(1, "启动它的进程", exited: true),
                                 line(2, "node", mem: "9.0G", marked: true)], "\(chatgpt.lines)")
        expect(chatgpt.how == "启动它的进程已经退出，macOS 记录是 ChatGPT 让它运行的。" && chatgpt.guess, chatgpt.how)

        // The agent is the pane's own process: no pane owner, the core's address still names the split.
        var atRoot = session
        atRoot.origin?.tmuxTarget = "main:5.2"
        let noPane = Detail.relation(atRoot, procs: procs, report: report, language: .zh)
        expect(noPane.lines.first == line(0, "tmux 分屏 main:5.2（gzh 窗口）"), "\(noPane.lines)")
        var unknown = session
        unknown.origin?.windowName = nil
        expect(Detail.relation(unknown, procs: procs, report: report, language: .en).lines.first == line(0, "tmux split"))
        // An older core without `name`: the session's largest root still carries the title.
        let bare = procs.map { ProcBrief(pid: $0.pid, ppid: $0.ppid, comm: $0.comm, inOwner: $0.inOwner, memBytes: $0.memBytes) }
        let old = Detail.relation(session, procs: bare, report: withPane, language: .zh)
        expect(old.lines.contains(line(2, "2.1.306", role: "「自动化评估设计与爬山优化博文」", mem: "314M")), "\(old.lines)")

        // Without procs (not fetched yet, or a replay) the parts stand in.
        let parts = Detail.relation(session, procs: [], report: withPane, language: .zh)
        expect(parts.lines == [line(0, "tmux 分屏 main:5.2（gzh 窗口）"), line(1, "外接小工具 fetch", mem: "47M")], "\(parts.lines)")
    }

    // QA50: 37 processes: same-name leaves merge, about 12 lines show, the rest fold.
    // The renderers' kernel name is cut at 16 bytes ("Google Chrome He"); the line shows the core's name.
    static func bigTreesMergeAndFold() throws {
        let renderer = "Google Chrome Helper (Renderer)"
        var procs = [R3.proc(500, 1, "Google Chrome", member: true, mem: 400 << 20)]
        procs += (0..<14).map { real(501 + Int32($0), 500, comm: "Google Chrome He", name: renderer, member: true, mem: 250 << 20) }
        procs += (0..<22).map { R3.proc(600 + Int32($0), 500, "helper\($0)", member: true, mem: UInt64(22 - $0) << 20) }
        let chrome = try decodeValue(Owner.self, ownerJSON("app:com.google.Chrome", mem: 1, pids: procs.map(\.pid), overrides: [
            "label": "Google Chrome", "evidence": [R3.evidence("app_bundle", "com.google.Chrome")],
            "parts": [["name": "标签页进程", "mem_bytes": 14 * 250 << 20, "process_count": 14,
                       "pids": (0..<14).map { 501 + $0 }, "unknown_mem_count": 0]]]))
        let tree = Detail.relation(chrome, procs: procs, report: nil, language: .zh)
        expect(tree.lines.count + 1 <= Detail.relationLimit, "\(tree.lines.count) lines and the folded one")
        expect(tree.lines.prefix(3) == [line(0, "Google Chrome（桌面应用）"), line(1, "Google Chrome", mem: "400M"),
                                        line(2, renderer + " ×14", role: "标签页进程", mem: "3.4G")], "\(tree.lines.prefix(3))")
        let shown = tree.lines.dropFirst(2).reduce(0) { $0 + ($1.name.hasSuffix("×14") ? 14 : 1) } + 1
        expect(tree.restSummary?.hasPrefix("其余 \(37 - shown) 个进程 共 ") == true && !tree.rest.isEmpty, tree.restSummary ?? "nil")
        expect(tree.how == "Google Chrome 自己的进程。" && !tree.guess)
    }

    // QA50 (reviews A, B and conform B1, round 4): roots with different parents keep their
    // own parent lines; leaves merge only under the same parent; launchd's own services are
    // not "started by something that exited".
    static func rootsKeepTheirOwnParents() throws {
        // ChatGPT's main process (ppid 1) and a Chrome that agent-browser started, one app owner.
        let app = try decodeValue(Owner.self, ownerJSON("app:com.openai.codex", mem: 1, pids: [10, 20], overrides: [
            "label": "ChatGPT", "evidence": [R3.evidence("app_bundle", "com.openai.codex"),
                                             R3.evidence("responsible_launcher", "com.openai.codex", "inferred")]]))
        let procs = [R3.proc(100, 1, "agent-browser", member: false, mem: 30 << 20),
                     R3.proc(20, 100, "Google Chrome", member: true, mem: 500 << 20),
                     R3.proc(10, 1, "ChatGPT", member: true, mem: 100 << 20)]
        let tree = Detail.relation(app, procs: procs, report: nil, language: .zh)
        expect(tree.lines == [line(0, "ChatGPT（桌面应用）"), line(1, "agent-browser", mem: "30M"),
                              line(2, "Google Chrome", mem: "500M"), line(1, "ChatGPT", mem: "100M")], "\(tree.lines)")
        // Two node leaves under different parents: two chains, never "node ×2".
        let two = try decodeValue(Owner.self, ownerJSON("app:x", mem: 1, pids: [11, 12], overrides: [
            "label": "X", "evidence": [R3.evidence("app_bundle", "x")]]))
        let chains = [R3.proc(100, 1, "bash", member: false), R3.proc(200, 1, "zsh", member: false),
                      R3.proc(11, 100, "node", member: true, mem: 20 << 20), R3.proc(12, 200, "node", member: true, mem: 10 << 20)]
        let forest = Detail.relation(two, procs: chains, report: nil, language: .zh)
        expect(forest.lines == [line(0, "X（桌面应用）"), line(1, "bash"), line(2, "node", mem: "20M"),
                                line(1, "zsh"), line(2, "node", mem: "10M")], "\(forest.lines)")
        let counted = (forest.lines + forest.rest).compactMap(\.memory).count
        expect(counted == 2, "each process once")
        // Safari's own XPC service, launched by launchd: no exited starter line.
        let safari = try decodeValue(Owner.self, ownerJSON("app:com.apple.Safari", mem: 1, pids: [30, 31], overrides: [
            "label": "Safari", "evidence": [R3.evidence("app_bundle", "com.apple.Safari"),
                                            R3.evidence("responsible_app", "com.apple.Safari")]]))
        let xpc = [R3.proc(30, 1, "Safari", member: true, mem: 300 << 20),
                   real(31, 1, comm: "SafariLinkExtens", name: "SafariLinkExtension", member: true, mem: 9 << 20)]
        let safariTree = Detail.relation(safari, procs: xpc, report: nil, language: .zh)
        expect(!safariTree.lines.contains { $0.exited }, "\(safariTree.lines)")
        expect(safariTree.lines.contains(line(1, "SafariLinkExtension", mem: "9M")), "the core's full name, not the cut one")
        // A parent missing from the snapshot (ppid > 1) has exited, whatever the evidence.
        let gone = Detail.relation(two, procs: [R3.proc(11, 777, "node", member: true, mem: 1 << 20)], report: nil, language: .zh)
        expect(gone.lines.contains(line(1, "启动它的进程", exited: true)), "\(gone.lines)")
        // Two orphan roots adopted by launchd: each has its own exited starter (review B nit, round 5).
        let orphan = try decodeValue(Owner.self, ownerJSON("orphan:40", mem: 1, kind: "orphan", pids: [40, 41]))
        let adopted = Detail.relation(orphan, procs: [R3.proc(40, 1, "node", member: true, mem: 2 << 20),
                                                      R3.proc(41, 1, "node", member: true, mem: 1 << 20)],
                                      report: nil, language: .zh)
        expect(adopted.lines == [line(0, "启动它的进程", exited: true), line(1, "node", mem: "2M"),
                                 line(0, "启动它的进程", exited: true), line(1, "node", mem: "1M")], "\(adopted.lines)")
    }

    // QA50: no technical identifiers in the card, for every design owner, both languages.
    static func relationShowsNoTechnicalIdentifiers() throws {
        let report = try Phase10Tests.designReport()
        let banned = ["TMUX_PANE", "CODEX_THREAD_ID", "com.", "pid", "PID", "确定", "推断", "较弱", "Certain", "Inferred", "Weak",
                      "pane_ancestor", "tmux_environment", "agent_source", "app_bundle", "responsible_", "runtime_home",
                      "other_uid", "service_fallback", "codex_thread_env", "为它负责"]
        let webkit = [R3.proc(9, 1, "com.apple.WebKit.WebContent", member: true, mem: 1 << 20)]
        for owner in report.owners {
            for language in [Language.zh, .en] {
                for procs in [[], webkit] {
                    let tree = Detail.relation(owner, procs: procs, report: report, language: language)
                    let text = (tree.lines + tree.rest).flatMap { [$0.name, $0.role ?? "", $0.memory?.text ?? ""] }
                        .joined(separator: "\n") + "\n" + tree.how + (tree.restSummary ?? "")
                    for word in banned { expect(!text.contains(word), "\(owner.id) \(language): \(word)\n\(text)") }
                    expect(text.range(of: #"%\d"#, options: .regularExpression) == nil, "\(owner.id): pane id\n\(text)")
                }
            }
        }
        let card = try R3.slice(try R3.source("DetailView.swift"), from: "private func relationCard", to: "private func partsCard")
        expect(card.contains("L10n.text(.relation, language)") && !card.contains("evidence"), "no evidence fold in the card")
        expect(L10n.text(.relation, .zh) == "进程关系" && L10n.text(.relation, .en) == "Process tree")
        // The raw evidence is copied, never shown.
        let owner = try require(report.owners.first { $0.id == "pane:%22" })
        let copied = CopyInfo.text(owner: owner, procs: [], report: report, language: .zh)
        expect(copied.contains("原始证据：") && copied.contains("pane_ancestor  %22  (certain)"), copied)
    }

    // QA51
    static func detailsDrawACpuCurveWithHoles() throws {
        let points: [HistoryPoint] = [0, 20, 40, 60, 80, 100].map { t in
            HistoryPoint(timestampS: t, memBytes: 1, cpuPct: t == 40 ? nil : Double(t),
                         cpuPctReadable: t == 80 ? 7 : nil, unknownCPUCount: t == 80 ? 1 : nil)
        }
        var noCPU = points
        noCPU[4].cpuPct = nil
        noCPU[4].cpuPctReadable = nil
        expect(Format.runs(points, metric: .cpu).map { $0.map(\.timestampS) } == [[0, 20], [60, 80, 100]], "a hole at 40")
        expect(Format.runs(noCPU, metric: .cpu).map { $0.map(\.timestampS) } == [[0, 20], [60], [100]])
        expect(Format.runs(points).map { $0.map(\.timestampS) } == [[0, 20, 40, 60, 80, 100]], "memory has no hole")
        let detail = try R3.source("DetailView.swift")
        let curve = try R3.slice(detail, from: "case .curve:", to: "case .parts")
        expect(curve.contains("L10n.text(.curveCPU30, language)") && curve.contains("metric: .cpu"), curve)
        expect(L10n.text(.curveCPU30, .zh) == "CPU · 最近 30 分钟")
        let chart = try R3.slice(detail, from: "struct HistoryChart", to: "static func value")
        expect(chart.contains("Format.runs(points, metric: metric)") && chart.contains("Format.drawable(points, metric: metric)"))
        // Review A, round 4: a missing newest stretch hides the panel's curve but never
        // the details' 30 minutes. Same input as Phase10Tests.cpuCurveBreaksWhereCpuIsMissing.
        let tail = [0, 20, 40, 60, 80, 100, 120].map { t in
            HistoryPoint(timestampS: Int64(t), memBytes: 1, cpuPct: t <= 40 ? 5 : nil)
        }
        expect(Format.trend(tail, width: 1, height: 1, metric: .cpu) == .insufficient, "panel: not fresh")
        expect(Format.drawable(tail, metric: .cpu), "details: the 0–40 s run is drawn")
        let lone = [HistoryPoint(timestampS: 0, memBytes: 1, cpuPct: nil), HistoryPoint(timestampS: 20, memBytes: 1, cpuPct: 3)]
        expect(!Format.drawable(lone, metric: .cpu), "one point is not a line")
    }

    // QA52
    static func cpuReadsAsALowerBound() throws {
        expect(Format.cpu(nil, readable: 12.5, unknown: 2, .zh) == Format.Memory(text: "≥ 12%", help: "2 个进程读不到 CPU"))
        expect(Format.cpu(nil, readable: 3.25, unknown: 1, .en) == Format.Memory(text: "≥ 3.2%", help: "CPU unreadable for 1 processes"))
        expect(Format.cpu(nil, readable: nil, unknown: 3, .zh) == Format.Memory(text: "—", help: "3 个进程读不到 CPU"))
        expect(Format.cpu(nil, readable: nil, unknown: nil, .zh) == Format.Memory(text: "—", help: nil))
        expect(Format.cpu(5, readable: nil, unknown: nil, .zh) == Format.Memory(text: "5.0%", help: nil))
        let partial: [String: Any] = ["cpu_pct_readable": 12.5, "unknown_cpu_count": 2]
        var swarm = swarmJSON("python", mem: 1, count: 3)
        swarm.merge(partial) { $1 }
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("app:brave", mem: 9, cpu: nil, overrides: partial), ownerJSON("app:known", mem: 8, cpu: 1)],
            swarms: [swarm]))
        let rows = Panel.rows(report, sort: .cpu, language: .zh, limit: .max)
        expect(rows.map(\.id) == ["app:known", "app:brave", "swarm:python"], "CPU order is unchanged: nil sorts last")
        for id in ["app:brave", "swarm:python"] {
            try expect(try require(rows.first { $0.id == id }).cpu(.zh).text == "≥ 12%", id)
        }
        let points = (0...3).map { HistoryPoint(timestampS: Int64($0 * 20), memBytes: 1, cpuPct: nil, cpuPctReadable: 5) }
        guard case .line = Format.trend(points, width: 10, height: 10, metric: .cpu) else { return fail("curve over the readable part") }
        // Panel, list, details and the copied text share one function.
        let views = try R3.viewSources().values.joined()
        expect(!views.contains("Format.cpu(row.cpuPct)"), "no view formats a row's CPU by itself")
        let panel = try R3.source("PanelView.swift"), main = try R3.source("MainWindowView.swift")
        let detail = try R3.source("DetailView.swift")
        expect(panel.contains("MemoryText(memory: row.cpu(language))") && main.contains("MemoryText(memory: row.cpu(language))")
               && detail.contains("MemoryText(memory: row.cpu(language))"))
        let owner = try require(report.owners.first { $0.id == "app:brave" })
        let copied = CopyInfo.text(owner: owner, procs: [], report: report, language: .zh)
        expect(copied.contains("CPU：≥ 12%（2 个进程读不到 CPU，只计读得到的）"), copied)
        // Nothing readable: "—" and the count, never "counted the rest" (review B nit, round 4).
        let none = try decodeValue(Owner.self, ownerJSON("app:dark", mem: 1, cpu: nil, overrides: ["unknown_cpu_count": 3]))
        let dark = CopyInfo.text(owner: none, procs: [], report: report, language: .zh)
        expect(dark.contains("CPU：—（3 个进程读不到 CPU）"), dark)
        expect(!CopyInfo.text(owner: none, procs: [], report: report, language: .en).contains("counted the rest"))
    }
}
