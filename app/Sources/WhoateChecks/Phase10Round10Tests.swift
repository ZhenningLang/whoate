// Phase 10 round 10 (owner check 2026-10-26 17:51–17:55, spec 末三节): QA67 the
// sidebar adds the footprints its list shows (R23); QA68 "AI & Agent" replaces
// "agent sessions" and takes the AI desktop apps (R24); QA69 every row says where
// it was started, and an untitled session is never named by its id (R25).
import Foundation
@testable import WhoateKit

enum Phase10Round10Tests {
    typealias R3 = Phase10Round3Tests

    static func footprint(_ rows: [PanelRow], _ language: Language = .zh) -> String {
        Format.memory(rows.reduce(0) { $0 + $1.memBytes }, unknown: rows.reduce(0) { $0 + $1.unknownMemCount }, language).text
    }

    static func origin(_ via: String?, name: String? = nil, bundle: String? = nil, attached: Bool? = nil,
                       pane: String? = nil, alive: Bool? = nil, target: String? = nil, window: String? = nil) -> [String: Any] {
        var launched: [String: Any] = [:]
        if let via { launched["via"] = via }
        if let name { launched["name"] = name }
        if let bundle { launched["bundle_id"] = bundle }
        if let attached { launched["attached"] = attached }
        var origin: [String: Any] = ["tmux_pane": pane as Any? ?? NSNull(), "pane_alive": alive as Any? ?? NSNull(),
                                     "cwd": "/Users/demo/work/oss-atlas", "ports": NSNull()]
        if let target { origin["tmux_target"] = target }
        if let window { origin["window_name"] = window }
        if via != nil { origin["launched_from"] = launched }
        return origin
    }

    static func session(_ id: String, tool: String, mem: UInt64, pid: Int32, origin: [String: Any], kind: String = "agent_session") throws -> [String: Any] {
        ownerJSON(id, mem: mem, kind: kind, pids: [pid], overrides: [
            "label": tool, "origin": origin,
            "agent": ["tool": tool, "session_id": NSNull(), "name": NSNull(), "name_source": NSNull(), "title": NSNull(),
                      "first_prompt": NSNull(), "first_reply": NSNull(), "status": NSNull(), "cwd": NSNull(),
                      "branch": NSNull(), "model": NSNull(), "cost_usd": NSNull(), "confidence": "certain",
                      "updated_at_ms": NSNull(), "stale": false],
        ])
    }

    // QA67 (R23)
    static func sidebarKindsAddTheFootprintsTheirListShows() throws {
        let report = try Phase10Tests.designReport()
        for language in [Language.zh, .en] {
            let rows = Phase10Tests.rows(report, language)
            for category in [MainCategory.all, .tmux, .agents, .apps, .orphans, .system] {
                // The list's own filter (OwnerListView), owners only: a process group overlays them.
                let listed = rows.filter { MainWindowModel.matches($0, category) && $0.source == .owner }
                let total = MainWindowModel.total(rows, category, language)
                expect(total.text == footprint(listed, language), "\(category): \(total.text) vs \(footprint(listed, language))")
            }
        }
        let rows = Phase10Tests.rows(report)
        // The owner's case (spec R23: "里面 1G，外面 100M"): the kind is never below its largest row.
        let largestPane = try require(rows.filter { MainWindowModel.matches($0, .tmux) }.map(\.memBytes).max())
        expect(MainWindowModel.total(rows, .tmux, .zh).text == "319M" && largestPane < 319 << 20 && largestPane > 300 << 20,
               "\(MainWindowModel.total(rows, .tmux, .zh).text) vs \(largestPane)")
        expect(!rows.filter { MainWindowModel.matches($0, .agents) }.isEmpty)
        // "≥" as in the list: Brave is partly unreadable, so "Apps" is a lower bound with the list's hover.
        let apps = MainWindowModel.total(rows, .apps, .zh)
        expect(apps.text.hasPrefix("≥ ") && apps.help == L10n.format(.unknownMem, .zh, "3"), "\(apps)")
        // A tmux pane with an unreadable process: "≥" there too.
        let partial = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("pane:%1", mem: 1 << 30, kind: "tmux_pane", pids: [1, 2], overrides: ["label": "main:1.0", "unknown_mem_count": 1]),
            ownerJSON("pane:%2", mem: 512 << 20, kind: "tmux_pane", pids: [3], overrides: ["label": "main:2.0"]),
        ]))
        let tmux = MainWindowModel.total(Phase10Tests.rows(partial), .tmux, .zh)
        expect(tmux.text == "≥ 1.5G" && tmux.help == L10n.format(.unknownMem, .zh, "1"), "\(tmux)")
        // "All" may exceed physical memory, and its hover says why (zh and en).
        let big = try decodeValue(Report.self, reportJSON(owners: (0..<4).map { i in ownerJSON("app:big\(i)", mem: 20 << 30, pids: [Int32(i + 1)]) }))
        for language in [Language.zh, .en] {
            let all = MainWindowModel.total(Phase10Tests.rows(big, language), .all, language)
            expect(all.text == "80.0G" && all.help == L10n.text(.memAllHelp, language), "\(all)")
        }
        expect(80 << 30 > (big.host?.memTotalBytes ?? .max), "the case is past the host's 48G")
        expect(L10n.text(.memSectionOwners, .zh) == "归属 · 占用" && L10n.text(.memSectionOwners, .en) == "Owners · footprint")
        expect(L10n.text(.memFootprint, .en).lowercased() == "footprint", "the glossary's word for 占用")
        let main = try R3.source("MainWindowView.swift")
        expect(main.contains(".help(total?.help ?? \"\")") && main.contains("MainWindowModel.total(rows, category, model.language)"))
    }

    // QA68 (R24)
    static func aiAppsAndSessionsShareAIAndAgent() throws {
        let report = try Phase10Tests.designReport()
        let rows = Phase10Tests.rows(report)
        let row = { (id: String) in try require(rows.first { $0.id == id }) }
        for id in ["app:com.openai.codex", "app:com.anthropic.claudefordesktop"] {
            let app = try row(id)
            expect(MainWindowModel.matches(app, .agents) && !MainWindowModel.matches(app, .apps), id)
            expect(MainWindowModel.category(of: app) == .agents && MainWindowModel.select(id, rows: rows).0 == .agents, id)
            expect(app.subtitle.hasPrefix("桌面应用"), "still reads as an app: \(app.subtitle)")
        }
        for id in ["app:com.brave.Browser", "app:com.openai.codex.cli", "app:com.pilotmoon.scroll-reverser"] {
            let app = try row(id)
            expect(MainWindowModel.matches(app, .apps) && !MainWindowModel.matches(app, .agents), id)
        }
        // One category per row: the kinds split "all" without overlap.
        for r in rows where r.source == .owner {
            let kinds = [MainCategory.tmux, .agents, .apps, .orphans, .system].filter { MainWindowModel.matches(r, $0) }
            expect(kinds.count == 1, "\(r.id): \(kinds)")
        }
        // Sessions and apps in one footprint order.
        let listed = rows.filter { MainWindowModel.matches($0, .agents) }
        expect(listed.contains { $0.kind == .app } && listed.contains { $0.kind == .agentSession })
        expect(zip(listed, listed.dropFirst()).allSatisfy { $0.memBytes >= $1.memBytes }, "\(listed.map(\.memBytes))")
        expect(MainWindowModel.aiApps.count == 7 && MainWindowModel.aiApps.contains("com.openai.chat"))
        // The name, and no "助手会话" / bare "Agent session" left in the interface.
        expect(MainCategory.agents.title(.zh) == "AI & Agent" && MainCategory.agents.title(.en) == "AI & Agent")
        for key in L10nKey.allCases {
            expect(!L10n.text(key, .zh).contains("助手会话"), "\(key)")
            let en = L10n.text(key, .en)
            expect(en.components(separatedBy: "Agent session").count == en.components(separatedBy: "AI & Agent session").count, "\(key): \(en)")
        }
        for file in ["MainWindowView.swift", "PanelView.swift", "DetailView.swift"] {
            let text = try R3.source(file)
            expect(!text.contains("助手会话"), file)
        }
        let session = try require(report.owners.first { $0.kind == .agentSession })
        let copied = CopyInfo.text(owner: session, procs: [], report: report, language: .zh)
        expect(!copied.contains("助手会话") && copied.contains("AI & Agent 会话"), copied)
    }

    // QA69 (R25)
    static func rowsSayWhereTheyWereStarted() throws {
        let owners: [[String: Any]] = [
            try session("agent:kilo:pid:86021", tool: "kilo", mem: 9 << 20, pid: 86021,
                        origin: origin("tmux", name: "Ghostty", bundle: "com.mitchellh.ghostty", attached: true,
                                       pane: "%100", alive: true, target: "main:10.1", window: "受众分析")),
            try session("agent:kilo:pid:86022", tool: "kilo", mem: 8 << 20, pid: 86022,
                        origin: origin("terminal", name: "iTerm2", bundle: "com.googlecode.iterm2")),
            try session("agent:codex:t-1", tool: "codex", mem: 7 << 20, pid: 86023,
                        origin: origin("app", name: "ChatGPT", bundle: "com.openai.codex")),
            try session("agent:kilo:pid:86024", tool: "kilo", mem: 6 << 20, pid: 86024, origin: origin(nil)),
            try session("agent:claude:pid:86025", tool: "claude", mem: 5 << 20, pid: 86025,
                        origin: origin("terminal", name: "Terminal", bundle: "com.apple.Terminal")),
            try session("agent:claude:pid:86026", tool: "claude", mem: 4 << 20, pid: 86026,
                        origin: origin("tmux", attached: false, pane: "%7", alive: true, target: "spare:0.0")),
            ownerJSON("orphan:900", mem: 3 << 20, kind: "orphan", pids: [900], overrides: [
                "label": "node", "origin": origin("tmux", pane: "%505", alive: false)]),
            ownerJSON("orphan:920", mem: 2 << 20, kind: "orphan", pids: [920], overrides: [
                "label": "node", "origin": origin("terminal", name: "Ghostty", bundle: "com.mitchellh.ghostty")]),
            ownerJSON("orphan:930", mem: 1 << 20, kind: "orphan", pids: [930], overrides: ["label": "node", "origin": origin(nil)]),
        ]
        let report = try decodeValue(Report.self, reportJSON(owners: owners))
        let zh = Dictionary(uniqueKeysWithValues: Phase10Tests.rows(report).map { ($0.id, $0) })
        let en = Dictionary(uniqueKeysWithValues: Phase10Tests.rows(report, .en).map { ($0.id, $0) })
        let want: [(String, String?, String?)] = [
            ("agent:kilo:pid:86021", "Ghostty · tmux main:10.1", "Ghostty · tmux main:10.1"),
            ("agent:kilo:pid:86022", "iTerm2", "iTerm2"),
            ("agent:codex:t-1", "ChatGPT 桌面版", "ChatGPT desktop app"),
            ("agent:claude:pid:86025", "终端", "Terminal"),
            ("agent:claude:pid:86026", "tmux spare:0.0（未连接）", "tmux spare:0.0 (not attached)"),
            ("orphan:900", "tmux（分屏已关闭）", "tmux (split closed)"),
            ("orphan:920", "Ghostty", "Ghostty"),
        ]
        for (id, zhLabel, enLabel) in want {
            let z = try require(zh[id]), e = try require(en[id])
            expect(z.subtitle.hasPrefix(zhLabel! + " · ") || z.subtitle == zhLabel, "\(id): \(z.subtitle)")
            expect(e.subtitle.hasPrefix(enLabel! + " · ") || e.subtitle == enLabel, "\(id): \(e.subtitle)")
            let owner = try require(report.owners.first { $0.id == id })
            expect(Detail.how(owner, owners: report.owners, language: .zh).0.hasSuffix("启动位置：\(zhLabel!)。"), id)
            expect(Detail.how(owner, owners: report.owners, language: .en).0.hasSuffix(" Started from \(enLabel!)."), id)
        }
        // The pane wording is not repeated next to the label.
        let closed = try require(zh["orphan:900"]).subtitle, tmux = try require(zh["agent:kilo:pid:86021"]).subtitle
        expect(closed.components(separatedBy: "已关闭").count == 2 && !tmux.contains("%100"), "\(closed) | \(tmux)")
        // Nothing known: no label and no "unknown".
        for id in ["agent:kilo:pid:86024", "orphan:930"] {
            let owner = try require(report.owners.first { $0.id == id })
            expect(RowNames.launch(owner, .zh) == nil && RowNames.launch(owner, .en) == nil, id)
            let subtitle = try require(zh[id]).subtitle
            for word in ["未知", "unknown", "Unknown"] { expect(!subtitle.contains(word), subtitle) }
        }
        // Apps keep "桌面应用" even with a source; a source this build does not know shows nothing.
        let app = try decodeValue(Owner.self, ownerJSON("app:x", mem: 1, overrides: ["origin": origin("app", name: "Dock")]))
        expect(RowNames.launch(app, .zh) == nil)
        let future = try decodeValue(Owner.self, ownerJSON("orphan:1", mem: 1, kind: "orphan", overrides: [
            "origin": ["tmux_pane": NSNull(), "pane_alive": NSNull(), "cwd": NSNull(), "ports": NSNull(),
                       "launched_from": ["via": "dock", "name": "X"]]]))
        expect(future.origin?.launchedFrom?.via == nil && RowNames.launch(future, .zh) == nil)

        // Titles: "<tool> · <folder>" for an untitled session; never an id, in the list or the panel.
        let kilo = try require(zh["agent:kilo:pid:86024"]).title, claude = try require(zh["agent:claude:pid:86025"]).title
        expect(kilo.hasPrefix("kilo · oss-atlas") && claude.hasPrefix("Claude Code · oss-atlas"), "\(kilo) | \(claude)")
        for rows in [Phase10Tests.rows(report), Phase10Tests.rows(report, .en),
                     RowNames.apply(Panel.rows(report, sort: .memory, language: .zh), report: report, language: .zh)] {
            for row in rows {
                for word in ["pid:", "agent:", "orphan:"] { expect(!row.title.contains(word), "\(word) in \(row.title)") }
            }
            expect(Set(rows.map(\.title)).count == rows.count, "titles stay unique: \(rows.map(\.title))")
        }
        // Detail headers neither (an app keeps its bundle id).
        for owner in report.owners + (try Phase10Tests.designReport()).owners {
            let row = try require((Phase10Tests.rows(report) + Phase10Tests.rows(try Phase10Tests.designReport())).first { $0.id == owner.id })
            let line = Detail.header(owner, row: row, language: .zh).kindLine
            for word in ["pid:", "agent:", "orphan:", "pane:", "service:", "system:"] { expect(!line.contains(word), line) }
        }
        // Copied details may keep the id.
        let copied = CopyInfo.text(owner: try require(report.owners.first), procs: [], report: report, language: .zh)
        expect(copied.contains("kilo"), copied)
    }
}
