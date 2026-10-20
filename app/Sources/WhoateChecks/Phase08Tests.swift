// Phase 08a: session titles (QA7), CPU header (QA8), advice text (QA9), the
// latest-run curve (QA10), window policy (QA12), quit (QA13), English (QA21).
import AppKit
import Foundation
@testable import WhoateKit

enum Phase08Tests {
    static func agentJSON(name: String?, source: String?, title: String?, idle: UInt64? = 9000) -> [String: Any] {
        [
            "tool": "claude", "session_id": "s", "name": name as Any? ?? NSNull(), "name_source": source as Any? ?? NSNull(),
            "title": title as Any? ?? NSNull(), "first_prompt": NSNull(), "first_reply": NSNull(), "status": "idle",
            "cwd": NSNull(), "branch": NSNull(), "model": NSNull(), "cost_usd": NSNull(), "confidence": "certain",
            "updated_at_ms": NSNull(), "stale": false, "idle_seconds": idle as Any? ?? NSNull(),
        ]
    }
    static func session(_ id: String, name: String?, source: String?, title: String?, origin: [String: Any]? = nil,
                        idle: UInt64? = 9000, mem: UInt64 = 1, cpu: Double? = 1) throws -> [String: Any] {
        var o = ownerJSON(id, mem: mem, cpu: cpu, kind: "agent_session")
        o["label"] = name ?? "claude"
        o["agent"] = agentJSON(name: name, source: source, title: title, idle: idle)
        o["origin"] = origin ?? [
            "tmux_pane": "%15", "pane_alive": true, "cwd": NSHomeDirectory() + "/src/site", "ports": [5173],
            "window_name": "main", "pane_title": "claude",
        ]
        return o
    }

    static func derivedSessionsAreTitledAndSubtitledInOrder() throws {
        let title = "X 数据流部署到 Claw 机器"
        let report = try decodeValue(Report.self, reportJSON(owners: [
            try session("d", name: "7-74", source: "derived", title: title, mem: 30),
            try session("m", name: "我的会话", source: "manual", title: "别的标题", mem: 20),
            try session("bare", name: "dotfiles-37", source: "derived", title: nil, origin: ["tmux_pane": NSNull(),
                "pane_alive": NSNull(), "cwd": NSNull(), "ports": NSNull()], idle: nil, mem: 10),
        ]))
        let rows = Panel.rows(report, sort: .memory, language: .zh)
        expect(rows.map(\.title) == [title, "我的会话", "dotfiles-37"])
        expect(rows[0].subtitle == "claude · main (claude) · site · 空闲 2 小时 30 分", rows[0].subtitle)
        expect(rows[2].subtitle == "claude", "missing parts are omitted: \(rows[2].subtitle)")
        let blank = try decodeValue(Owner.self, try session("b", name: "9-1", source: "derived", title: " \n "))
        expect(Panel.title(blank) == "9-1", "a blank title (newlines too, like the core's trim) falls back")
        let partial = try decodeValue(Owner.self, try session("p", name: "n", source: "derived", title: "t",
            origin: ["tmux_pane": "%3", "pane_alive": true, "cwd": "/", "ports": NSNull(), "window_name": NSNull(),
                     "pane_title": NSNull()]))
        expect(Panel.subtitle(partial, .en) == "claude · %3 · Idle 2h 30m")
        for row in rows { expect(!row.subtitle.contains(" ·  · ") && !row.subtitle.hasSuffix(" · ")) }
        // The interim main window builds its rows with the same function.
        let main = String(decoding: try repoFile("app/Sources/Whoate/MainWindowView.swift"), as: UTF8.self)
        expect(main.contains("Panel.rows(") && main.contains("row.title") && !main.contains("owner.label"))
    }

    static func host(cpu: [String: Any]?) throws -> WhoateKit.Host {
        var json: [String: Any] = [
            "mem_total_bytes": 48 << 30, "mem_used_bytes": 41 << 30, "compressed_bytes": 8 << 30,
            "wired_bytes": 4 << 30, "swap_used_bytes": 20 << 30, "pressure": "warn",
        ]
        if let cpu { json["cpu"] = cpu }
        return try decodeValue(WhoateKit.Host.self, json)
    }
    static func cpu(_ usage: Double?, load: Double? = 1, cores: Int? = 12, status: String = "fresh",
                    reason: String? = nil) -> [String: Any] {
        ["status": status, "usage_pct": usage as Any? ?? NSNull(), "load_1m": load as Any? ?? NSNull(),
         "logical_cores": cores as Any? ?? NSNull(), "reason": reason as Any? ?? NSNull()]
    }

    static func cpuHeaderShowsUsageLoadAndBusyness() throws {
        let cases: [(Double, Double, String)] = [(20, 2, "空闲"), (60, 2, "偏忙"), (30, 12.5, "繁忙"), (90, 1, "繁忙")]
        for (usage, load, word) in cases {
            let header = Panel.header(try host(cpu: cpu(usage, load: load)), sort: .cpu, .zh)
            expect(header.headline == "整机 CPU \(Format.cpu(usage))", header.headline)
            expect(header.detail == "1 分钟负载 \(String(format: "%.2f", load)) / 12 核 · \(word)", header.detail ?? "nil")
        }
        try expect(Panel.cpuLevel(try host(cpu: cpu(10, load: 9, cores: 12)).cpu) == .moderate, "load 0.75/core")
        try expect(Panel.cpuLevel(try host(cpu: cpu(84.9, load: 0, cores: 12)).cpu) == .moderate)
        try expect(Panel.cpuLevel(try host(cpu: cpu(49.9, load: 8.3, cores: 12)).cpu) == .idle)
        let missing = Panel.header(try host(cpu: cpu(nil, status: "missing", reason: "first_sample")), sort: .cpu, .zh)
        expect(missing.headline == "整机 CPU 暂缺：刚开始采样", missing.headline)
        expect(!missing.headline.contains("0%") && !missing.headline.contains("0.0%"))
        let absent = Panel.header(try host(cpu: nil), sort: .cpu, .en)
        expect(absent.headline == "CPU unavailable: not reported" && absent.detail == nil)
        let odd = Panel.header(try host(cpu: cpu(nil, status: "missing", reason: "new_code")), sort: .cpu, .en)
        expect(odd.headline == "CPU unavailable: new_code")
        // Memory mode is the base headline, unchanged.
        let memory = try host(cpu: cpu(50))
        expect(Panel.header(memory, sort: .memory, .zh) == Header(headline: Panel.headline(memory, .zh)))
        expect(Panel.header(memory, sort: .memory, .zh).headline == "内存偏紧")
    }

    @MainActor static func panelAndMainWindowShareOneSort() async throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("big", mem: 900, cpu: 1), ownerJSON("busy", mem: 1, cpu: 40), ownerJSON("none", mem: 2, cpu: nil),
            ownerJSON("a", mem: 3), ownerJSON("b", mem: 4), ownerJSON("c", mem: 5), ownerJSON("d", mem: 6),
        ]))
        let spy = Spy(report)
        let model = AppModel(language: .en, makeEngine: { spy }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             sleep: { _ in try await Task.sleep(for: .milliseconds(5)) })
        model.setVisible(main: true)
        model.start()
        for _ in 0..<200 where model.report == nil { try? await Task.sleep(for: .milliseconds(5)) }
        model.stop()
        model.sort = .cpu
        let all = Panel.rows(try require(model.report), sort: model.sort, language: .en, limit: .max)
        expect(all.count == 7, "the main window list is not truncated")
        expect(all.first?.id == "busy" && all.last?.id == "none", "CPU descending, unknown last")
        expect(model.rows.first?.id == "busy", "the panel follows the same sort")
        model.sort = .memory
        expect(model.rows.first?.id == "big")
        for file in ["MainWindowView", "PanelView"] {
            let source = String(decoding: try repoFile("app/Sources/Whoate/\(file).swift"), as: UTF8.self)
            expect(source.contains("selection: $model.sort"), "\(file) binds model.sort")
        }
    }

    final class Spy: RefreshEngine, @unchecked Sendable {
        let report: Report
        init(_ report: Report) { self.report = report }
        func tick() -> Decoded<Report> { .value(report) }
        func setActive(_ active: Bool) -> Decoded<RefreshControl> {
            decode(RefreshControl.self, from: Data("{\"active\":\(active)}".utf8))
        }
    }

    static func adviceTextIsVisibleForEachEntry() throws {
        let known = try require(Engine.insights().first)
        let catalog = Advice.catalog
        expect(catalog[known.id] == known)
        let item = Advice.items([known.id], catalog: catalog, language: .zh)[0]
        expect(item.cause == known.cause.zh && item.advice == known.advice.zh && item.source == known.source)
        expect(!(item.cause ?? "").isEmpty && !(item.advice ?? "").isEmpty && !(item.source ?? "").isEmpty)
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("o", mem: 1, overrides: ["insights": [known.id, "demo.insight"]]),
        ]))
        let row = Panel.rows(report, sort: .memory, language: .zh)[0]
        expect(row.pill == "有 2 条建议" && row.insights == [known.id, "demo.insight"])
        let zh = Advice.lines(row.insights, language: .zh)
        expect(zh.contains("原因：" + known.cause.zh) && zh.contains("建议：" + known.advice.zh) && zh.contains(known.source))
        expect(zh.hasSuffix("\n\ndemo.insight"), "an unknown id is shown as itself")
        let en = Advice.lines(row.insights, language: .en)
        expect(en.contains("Cause: " + known.cause.en) && en.contains("Advice: " + known.advice.en))
        expect(!en.contains(known.cause.zh) || known.cause.zh == known.cause.en, "current language only")
    }

    static func points(_ seconds: [Int64]) -> [HistoryPoint] {
        seconds.enumerated().map { HistoryPoint(timestampS: $0.element, memBytes: UInt64(100 + $0.offset), cpuPct: nil) }
    }

    static func curveDrawsOnlyTheLatestContinuousRun() {
        // Raw history: one point per 2 s visible refresh.
        let old = Array(stride(from: Int64(0), through: 60, by: 2))
        let recent = Array(stride(from: Int64(660), through: 720, by: 2))
        guard case .line(let line) = Format.trend(points(old + recent), width: 64, height: 16) else {
            return fail("a 60 s run after a 10 min gap is drawn")
        }
        expect(line.count == recent.count, "only the run after the gap")
        expect(line.first?.x == 0 && line.last?.x == 64, "the axis covers only that run")
        expect(Format.trend(points(old + [700, 702, 704]), width: 64, height: 16) == .insufficient, "< 30 s")
        expect(Format.trend(points(old + [720]), width: 64, height: 16) == .insufficient, "a lone point is never joined")
        expect(Format.trend(points([0, 300, 600, 900, 1200, 1210, 1212]), width: 64, height: 16) == .insufficient,
               "sparse points are not a curve")
        // One point per brief opening, 30 s apart: 59 or exactly 60 (the tick cap) points are still not a curve.
        for count in [59, 60, 61] {
            let sparse = points((0..<count).map { Int64($0) * 30 })
            expect(Format.trend(sparse, width: 64, height: 16) == .insufficient, "\(count) isolated points")
        }
        expect(Format.trend(points([5]), width: 64, height: 16) == .insufficient)
        expect(Format.trend([], width: 64, height: 16) == .insufficient)
        // 30 minutes of continuous raw history is one run.
        guard case .line(let long) = Format.trend(points(Array(stride(from: Int64(0), through: 1800, by: 2))),
                                                  width: 64, height: 16) else { return fail("continuous history") }
        expect(long.count == 901)
        guard case .line(let flat) = Format.trend([1000, 1010, 1005, 1000, 1002, 1004, 1001, 1003, 1000, 1001, 1002, 1000, 1001, 1002, 1003, 1000].enumerated().map {
            HistoryPoint(timestampS: Int64($0.offset * 2), memBytes: UInt64($0.element), cpuPct: nil)
        }, width: 10, height: 10) else { return fail("flat") }
        expect(abs(flat[0].y - flat[1].y) < 2.5, "a 1% change stays near flat")
    }

    final class HistorySpy: RefreshEngine, @unchecked Sendable {
        let report: Report
        let raw: [HistoryPoint]
        private let lock = NSLock()
        private var asked: [String] = []
        init(_ report: Report, raw: [HistoryPoint]) { self.report = report; self.raw = raw }
        func tick() -> Decoded<Report> { .value(report) }
        func setActive(_ active: Bool) -> Decoded<RefreshControl> {
            decode(RefreshControl.self, from: Data("{\"active\":\(active)}".utf8))
        }
        func history(pid: Int32, owner: String) -> [HistoryPoint]? {
            lock.withLock { asked.append(owner) }
            return owner == "raw" ? raw : nil
        }
        var requested: [String] { lock.withLock { asked } }
    }

    /// The panel draws from raw history, never from a tick's downsampled copy.
    @MainActor static func panelCurvesUseRawHistoryNotTheTickCopy() async throws {
        // The tick copy looks continuous (60 points, 30 s apart) but hides gaps.
        let downsampled = (0..<60).map { ["timestamp_s": $0 * 30, "mem_bytes": 100, "cpu_pct": NSNull()] as [String: Any] }
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("raw", mem: 9, pids: [11], overrides: ["history": downsampled]),
            ownerJSON("unfetched", mem: 8, pids: [12], overrides: ["history": downsampled]),
        ]))
        let raw = points(Array(stride(from: Int64(1000), through: 1060, by: 2)))
        let spy = HistorySpy(report, raw: raw)
        let model = AppModel(language: .en, makeEngine: { spy }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             sleep: { _ in try await Task.sleep(for: .milliseconds(5)) })
        model.setVisible(panel: true)
        model.start()
        for _ in 0..<200 where model.report == nil { try? await Task.sleep(for: .milliseconds(5)) }
        model.stop()
        let rows = Dictionary(uniqueKeysWithValues: model.rows.map { ($0.id, $0) })
        expect(Set(spy.requested) == ["raw", "unfetched"], "both visible owners are fetched")
        expect(rows["raw"]?.history == raw)
        expect(rows["unfetched"]?.history == [], "no raw history means no curve")
        expect(Format.trend(rows["unfetched"]?.history ?? [], width: 64, height: 16) == .insufficient)
    }

    @MainActor static func mainWindowSwitchesPolicyAndIsRaisedNotDuplicated() throws {
        _ = NSApplication.shared
        expect(WindowPolicy.activation(mainWindows: 0) == .accessory)
        expect(WindowPolicy.activation(mainWindows: 1) == .regular)
        expect(WindowPolicy.open(mainWindowExists: true) == .bringToFront)
        expect(WindowPolicy.open(mainWindowExists: false) == .create)
        expect(WindowPolicy.openLabel(mainWindowExists: true, .zh) == "显示主窗口")
        expect(WindowPolicy.openLabel(mainWindowExists: false, .zh) == "打开 whoate")
        expect(WindowPolicy.openLabel(mainWindowExists: true, .en) == "Show main window")
        var applied: [NSApplication.ActivationPolicy] = []
        let windows = MainWindows(apply: { applied.append($0) })
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 80, height: 60), styleMask: [.titled, .closable],
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.hidesOnDeactivate = true
        expect(!windows.bringToFront(), "nothing to raise yet")
        windows.register(window)
        windows.register(window)
        expect(windows.count == 1 && windows.exists)
        expect(!window.hidesOnDeactivate, "deactivation never hides the main window")
        expect(applied == [.regular])
        expect(windows.bringToFront())
        expect(windows.count == 1, "raising never creates a second window")
        window.close()
        expect(windows.count == 0 && !windows.exists)
        expect(applied.last == .accessory)
    }

    @MainActor static func quitGoesThroughOneInjectedTerminate() throws {
        var calls = 0
        let quit = QuitAction(terminate: { calls += 1 })
        quit()
        expect(calls == 1)
        let panel = String(decoding: try repoFile("app/Sources/Whoate/PanelView.swift"), as: UTF8.self)
        let app = String(decoding: try repoFile("app/Sources/Whoate/WhoateApp.swift"), as: UTF8.self)
        expect(panel.contains("L10n.text(.quit, language)) { quit() }") && panel.contains(".keyboardShortcut(\"q\")"))
        expect(app.contains("CommandGroup(replacing: .appTermination)") && app.contains("delegate.quit()"))
        expect(!panel.contains("exit(") && !app.contains("NSApp.terminate"), "only QuitAction terminates")
        expect(L10n.text(.quit, .zh) == "退出 whoate" && L10n.text(.quit, .en) == "Quit whoate")
    }

    static func automationTimersAreInertUnlessEnabled() {
        let keys = ["WHOATE_UI_STATE_FILE": "/tmp/s", "WHOATE_UI_LAYOUT_FILE": "/tmp/l", "WHOATE_UI_QUIT_AFTER": "3",
                    "WHOATE_UI_MAIN_CLOSE_AFTER": "4", "WHOATE_UI_SHOW_MAIN_AFTER": "5"]
        expect(!Automation(environment: keys).isActive)
        let a = Automation(environment: keys.merging(["WHOATE_UI_AUTOMATION": "main-window"]) { $1 })
        expect(a.mainWindow && !a.panelWindow && a.stateFile == "/tmp/s" && a.layoutFile == "/tmp/l")
        expect(a.quitAfter == 3 && a.mainCloseAfter == 4 && a.showMainAfter == 5)
        expect(Automation(environment: ["WHOATE_UI_AUTOMATION": "menubar", "WHOATE_UI_QUIT_AFTER": "-1"]).quitAfter == nil)
    }

    static func hostCpuDecodesAndMatchesTheSchema() throws {
        let decoded = try require(try host(cpu: cpu(12.5, load: 3, cores: 10)).cpu)
        expect(decoded == HostCPU(status: .fresh, usagePct: 12.5, load1m: 3, logicalCores: 10, reason: nil))
        try expect(host(cpu: nil).cpu == nil, "old reports have no cpu")
        let schema = try require(JSONSerialization.jsonObject(with: repoFile("schema/whoate-v1.schema.json")) as? [String: Any])
        let host = try require(((schema["$defs"] as? [String: Any])?["host"] as? [String: Any])?["anyOf"] as? [[String: Any]])
            .first { $0["type"] as? String == "object" }
        let properties = try require(((host?["properties"] as? [String: Any])?["cpu"] as? [String: Any])?["properties"] as? [String: Any])
        expect(Set(properties.keys) == Set(HostCPU.CodingKeys.allCases.map(\.rawValue)))
    }

    /// U-DEC-4: English view-model text has no CJK, apart from data fields.
    static func englishTextHasNoCJK() throws {
        let known = try require(Engine.insights().first)
        var reports = [try decodeValue(Report.self, reportJSON(owners: [
            try session("d", name: "7-74", source: "derived", title: "部署 Claw"),
            ownerJSON("o", mem: 2, level: "act", rule: "orphan-heavy", kind: "orphan", overrides: ["insights": [known.id]]),
        ]))]
        reports.append(try JSONDecoder().decode(Report.self, from: repoFile("tests/fixtures/attribution/sample-v1.json")))
        if let path = ProcessInfo.processInfo.environment["WHOATE_EN_TICK_FILE"] {
            reports.append(try JSONDecoder().decode(Report.self, from: Data(contentsOf: URL(fileURLWithPath: path))))
        } else if ProcessInfo.processInfo.environment["WHOATE_REQUIRE_EN_TICK"] != nil {
            fail("WHOATE_EN_TICK_FILE required in this tier")
        }
        let cjk = try NSRegularExpression(pattern: "[\\p{Han}\\p{Hiragana}\\p{Katakana}\\p{Hangul}]")
        for report in reports {
            var data: [String] = []
            for o in report.owners {
                data += [o.label, o.agent?.title, o.agent?.name, o.agent?.cwd, o.origin?.cwd, o.origin?.windowName,
                         o.origin?.paneTitle].compactMap { $0 }
                data += o.parts.map(\.name)
                data += o.origin?.cwd.map { [URL(fileURLWithPath: $0).lastPathComponent] } ?? []
            }
            data += report.swarms.map(\.label)
            var texts = [Panel.header(report.host, sort: .cpu, .en).headline, Panel.header(report.host, sort: .memory, .en).headline,
                         Panel.header(report.host, sort: .cpu, .en).detail ?? "", Panel.footer(report, lastError: nil, interval: 2, language: .en),
                         WindowPolicy.openLabel(mainWindowExists: true, .en), L10n.text(.quit, .en),
                         L10n.text(.notEnoughSamples, .en), L10n.text(.mainWindowPending, .en)]
            for row in Panel.rows(report, sort: .cpu, language: .en, limit: .max) {
                texts += [row.title, row.subtitle, row.pill, Advice.lines(row.insights, language: .en)]
            }
            for var text in texts {
                for value in data.sorted(by: { $0.count > $1.count }) where !value.isEmpty {
                    text = text.replacingOccurrences(of: value, with: "")
                }
                expect(cjk.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) == nil, "CJK in English: \(text)")
            }
        }
    }
}
