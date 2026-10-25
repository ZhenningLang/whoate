// Phase 10 design gate: the display logic the design shots rely on — row
// names (QA11 shape), the menu bar item (QA15 shape), main window categories
// and selection, detail curve runs, and the design report itself.
import Foundation
@testable import WhoateKit

enum Phase10DesignTests {
    static func designReport() throws -> Report {
        try JSONDecoder().decode(Report.self, from: repoFile("tests/gui/design-report.json"))
    }

    static func appPart(_ name: String, _ mem: UInt64, count: Int = 1) -> [String: Any] {
        ["name": name, "mem_bytes": mem, "process_count": count, "pids": [1], "unknown_mem_count": 0]
    }

    static func sameNamedAppsGetDistinctTitlesAndAKindWord() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("app:com.openai.codex", mem: 30, overrides: ["label": "ChatGPT", "parts": [appPart("ChatGPT Helper", 20), appPart("node", 10)]]),
            ownerJSON("app:com.openai.codex.cli", mem: 4, overrides: ["label": "ChatGPT", "parts": [appPart("codex", 4, count: 2)]]),
            ownerJSON("app:com.anthropic.claudefordesktop", mem: 16, overrides: ["label": "Claude", "parts": [appPart("Claude", 16)]]),
            ownerJSON("orphan:1", mem: 2, kind: "orphan", overrides: ["label": "node"]),
            ownerJSON("orphan:2", mem: 1, kind: "orphan", overrides: ["label": "node"]),
        ]))
        let rows = RowNames.apply(Panel.rows(report, sort: .memory, language: .zh, limit: .max), report: report, language: .zh)
        let titles = Dictionary(uniqueKeysWithValues: rows.map { ($0.id, $0.title) })
        expect(titles["app:com.openai.codex"] == "ChatGPT", "shortest id keeps the plain title")
        expect(titles["app:com.openai.codex.cli"] == "ChatGPT · codex", titles["app:com.openai.codex.cli"] ?? "")
        expect(titles["orphan:2"] == "node · 孤儿进程", titles["orphan:2"] ?? "")
        expect(Set(titles.values).count == rows.count, "titles are pairwise distinct")
        let claude = try require(rows.first { $0.id == "app:com.anthropic.claudefordesktop" })
        expect(claude.subtitle == "桌面应用", "a part named like the app adds nothing: \(claude.subtitle)")
        let chatgpt = try require(rows.first { $0.id == "app:com.openai.codex" })
        expect(chatgpt.subtitle == "桌面应用 · Helper", "one part, the app's name dropped: \(chatgpt.subtitle)")
        let en = RowNames.apply(Panel.rows(report, sort: .memory, language: .en, limit: .max), report: report, language: .en)
        expect(en.first { $0.id == "app:com.anthropic.claudefordesktop" }?.subtitle == "App")
    }

    static func swarmSubtitleNamesItsMainOwners() throws {
        try swarmSubtitle(shape: "burst")
        try swarmSubtitle(shape: "spread")
    }

    static func swarmSubtitle(shape: String) throws {
        var swarm = swarmJSON("exe:python3.13", mem: 74 << 20, count: 74, shape: shape)
        swarm["owners"] = (1...15).map { ["id": "agent:claude:s\($0)", "count": 3] }
            + (1...4).map { ["id": "agent:opencode:o\($0)", "count": 2] }
            + [["id": "orphan:7", "count": 1], ["id": "orphan:8", "count": 1], ["id": "app:com.anthropic.claude-code", "count": 19]]
        let report = try decodeValue(Report.self, reportJSON(owners: [], swarms: [swarm]))
        let row = try require(RowNames.apply(Panel.rows(report, sort: .memory, language: .zh), report: report, language: .zh).first)
        // The title says "× 74"; the subtitle spends its width on the owners (design round 1, finding 1).
        expect(row.title == "exe:python3.13 × 74")
        // R19 (owner 2026-10-25): a spread group counts processes, the unit its details list;
        // owners outside the report say nothing about the app: one "not recognised" line.
        expect(row.subtitle == (shape == "burst" ? "分属 22 个归属：Claude Code 会话 15、opencode 会话 4、孤儿进程 2…"
                                : "74 个进程 · 没认出是哪个应用"), row.subtitle)
        // One user only: its count would repeat the total (orchestrator, round 8).
        if shape == "spread" {
            let en = try require(RowNames.apply(Panel.rows(report, sort: .memory, language: .en), report: report, language: .en).first)
            expect(en.subtitle == "74 processes · App not recognised", en.subtitle)
            expect(!row.subtitle.hasSuffix(" 74"), "no repeated count")
        }
    }

    static func menuBarItemIsNeverColoured() throws {
        let host = { (pressure: String) in
            try decodeValue(Host.self, ["mem_total_bytes": 48 << 30, "mem_used_bytes": 41 << 30, "compressed_bytes": 0,
                                        "wired_bytes": 0, "swap_used_bytes": 0, "pressure": pressure])
        }
        let warn = try host("warn"), critical = try host("critical"), normal = try host("normal")
        expect(MenuBarItem.label(normal, style: .symbol, language: .zh) == .init(image: .logo, text: "41.0G"))
        expect(MenuBarItem.label(nil, style: .word, language: .zh) == .init(image: .logo, text: "—"))
        expect(MenuBarItem.label(warn, style: .symbol, language: .zh).image == .symbol("exclamationmark.triangle"))
        expect(MenuBarItem.label(critical, style: .symbol, language: .zh).image == .symbol("exclamationmark.triangle.fill"))
        expect(MenuBarItem.label(warn, style: .prefix, language: .zh).text == "! 41.0G")
        expect(MenuBarItem.label(critical, style: .word, language: .en).text == "41.0G critical")
        // R20 (owner 2026-10-25): the pie logo replaces the memory-chip symbol when normal.
        expect(MenuBarItem.label(normal, style: .systemSymbol, language: .zh) == .init(image: .logo, text: "41.0G"))
        expect(MenuBarItem.label(warn, style: .systemSymbol, language: .zh).image == .symbol("exclamationmark.triangle"))
        // Feedback 5: no tinted bitmap path is left in the app sources.
        for file in ["WhoateApp.swift", "PanelView.swift", "Palette.swift"] {
            let text = String(decoding: try repoFile("app/Sources/Whoate/" + file), as: UTF8.self)
            expect(!text.contains("isTemplate = false") && !text.contains("render(text:"), file)
        }
    }

    static func mainWindowCategoriesAndAutomationSelection() throws {
        let report = try designReport()
        let rows = RowNames.apply(Panel.rows(report, sort: .memory, language: .zh, limit: .max), report: report, language: .zh)
        expect(MainWindowModel.select("kind:agent", rows: rows) == (.agents, "agent:claude:9f2c41d7-5b0e-4c1a-9d3e-2a7f60c1b8e4"))
        expect(MainWindowModel.select("kind:orphan", rows: rows) == (.orphans, "orphan:948211"))
        expect(MainWindowModel.select("kind:browser", rows: rows) == (.apps, "app:com.brave.Browser"))
        expect(MainWindowModel.select("overview", rows: rows) == (.all, nil))
        expect(rows.filter { MainWindowModel.matches($0, .system) }.map(\.id).sorted()
               == ["service:com.apple.mds_stores", "system:312"])
        expect(MainWindowModel.total(rows, .needsAction) == "4", "orphan act, Brave, two sessions watch")
        expect(MainWindowModel.total(rows, .growing) == "2", "Brave and the Claude Code session")
        expect(Set(rows.map(\.title)).count == rows.count, "design report titles are distinct after naming")
        let brave = try require(rows.first { $0.id == "app:com.brave.Browser" })
        expect(brave.unknownMemCount == 3)
    }

    static func detailCurveLeavesGapsAndCpuModeDrawsCpu() throws {
        let points = [0, 20, 40, 160, 180, 200].map { HistoryPoint(timestampS: $0, memBytes: 100, cpuPct: Double($0)) }
        expect(Format.runs(points).map { $0.map(\.timestampS) } == [[0, 20, 40], [160, 180, 200]], "120 s gap splits")
        guard case .line(let line) = Format.trend(points, width: 10, height: 10, metric: .cpu) else {
            return fail("cpu trend should draw")
        }
        expect(line.first?.y == 10 && line.last?.y == 0, "CPU rises from 160% to 200%: bottom to top")
        let noCPU = points.map { HistoryPoint(timestampS: $0.timestampS, memBytes: 1, cpuPct: nil) }
        expect(Format.trend(noCPU, width: 10, height: 10, metric: .cpu) == .insufficient)
    }
}
