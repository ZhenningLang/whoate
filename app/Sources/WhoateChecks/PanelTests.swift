// QA9 (panel and close-confirmation parts): concrete inputs, concrete outputs.
import Foundation
@testable import WhoateKit

enum PanelTests {
    static func rowsSortByMemoryMixSwarmsAndDoNotPinAct() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("big", mem: 900), ownerJSON("act", mem: 100, level: "act", rule: "orphan-heavy", kind: "orphan"),
            ownerJSON("mid", mem: 500), ownerJSON("small", mem: 50), ownerJSON("tiny", mem: 10),
            ownerJSON("least", mem: 1),
        ], swarms: [swarmJSON("mcp", mem: 600, count: 61)]))
        let rows = Panel.rows(report, sort: .memory, language: .zh)
        expect(rows.map(\.id) == ["big", "swarm:mcp", "mid", "act", "small"])
        expect(rows[1].title == "mcp × 61")
        expect(rows[1].source == .swarm)
        expect(rows[3].closable)
        expect(!rows[0].closable)
    }

    static func cpuSortPutsUnknownLastAndShowsDash() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("unknown", mem: 900, cpu: nil), ownerJSON("busy", mem: 1, cpu: 40),
            ownerJSON("idle", mem: 2, cpu: 0),
        ], swarms: [swarmJSON("s", mem: 5, count: 2, cpu: 3)]))
        let rows = Panel.rows(report, sort: .cpu, language: .en)
        expect(rows.map(\.id) == ["busy", "swarm:s", "idle", "unknown"])
        expect(Format.cpu(rows[3].cpuPct) == "—")
        expect(Format.cpu(0) == "0.0%")
    }

    static func confirmationListsEveryPidAndEstimatesOnlyFromReclaimable() throws {
        let estimate: [String: Any] = ["bytes": 3 << 30, "unknown_count": 0, "basis": "b", "reason": NSNull()]
        let owner = try decodeValue(Owner.self, ownerJSON(
            "o", mem: 9 << 30, level: "act", rule: "orphan-heavy", kind: "orphan", pids: [11, 12, 13],
            overrides: ["reclaimable_physical": estimate]))
        let c = KillConfirmation(owner: owner, language: .zh)
        expect(c.pids == [11, 12, 13])
        expect(c.estimate == "3.0G")
        expect(!c.isProtected)

        let unknown: [String: Any] = ["bytes": NSNull(), "unknown_count": 2, "basis": "b", "reason": "resident_measurement_missing"]
        let partial = try decodeValue(Owner.self, ownerJSON(
            "p", mem: 9 << 30, pids: [1, 2, 3], overrides: ["reclaimable_physical": unknown]))
        let u = KillConfirmation(owner: partial, language: .zh)
        expect(u.estimate == "无法预估（2 个进程测不到）")
        expect(u.estimateBytes == nil)
        expect(!u.estimate.contains(Format.bytes(9 << 30)), "footprint must never stand in for the estimate")

        let absent = try decodeValue(Owner.self, ownerJSON("q", mem: 9 << 30, pids: [1, 2]))
        expect(KillConfirmation(owner: absent, language: .en).estimate == "Cannot estimate (2 unmeasured)")
    }

    static func systemOwnersAreProtected() throws {
        let cases: [(String, String)] = [("system", "default"), ("service", "default"), ("app", "system"), ("app", "system.fseventsd")]
        for (kind, rule) in cases {
            let owner = try decodeValue(Owner.self, ownerJSON("x", mem: 1, level: "act", rule: rule, kind: kind))
            expect(KillConfirmation(owner: owner, language: .zh).isProtected, "\(kind) \(rule)")
            let report = try decodeValue(Report.self, reportJSON(owners: [ownerJSON("x", mem: 1, level: "act", rule: rule, kind: kind)]))
            expect(!Panel.rows(report, sort: .memory, language: .zh)[0].closable)
        }
        let system = try decodeValue(Owner.self, ownerJSON("x", mem: 1, rule: "system.fseventsd", kind: "system"))
        expect(Panel.pill(system, .zh) == "系统服务")
    }

    static func resultShowsSignedChangeAndUnmeasured() throws {
        let negative = try decodeValue(KillResult.self, killJSON([(7, "terminated", nil)], change: -(300 << 20)))
        let outcome = KillOutcome(code: 0, result: negative, language: .zh)
        expect(outcome.change == "−300M")
        expect(outcome.lines[0].text == "已结束")
        let missing = try decodeValue(KillResult.self, killJSON([(7, "refused", "protected_pid")], change: nil))
        let refused = KillOutcome(code: 4, result: missing, language: .zh)
        expect(refused.change == "未测到")
        expect(refused.lines[0].text == "已拒绝 (受保护的进程)")
        let codes = try decodeValue(KillResult.self, killJSON([(7, "refused", "no_confirmed_plan"), (8, "refused", "new_core_reason")], change: nil))
        let english = KillOutcome(code: 4, result: codes, language: .en)
        expect(english.lines.map(\.text) == ["refused (not in the confirmed close list)", "refused (new_core_reason)"])
        expect(KillOutcome.reasons.values.allSatisfy { L10n.table[$0] != nil })
    }

    static func forceCannotBeSentTwice() throws {
        let result = try decodeValue(KillResult.self, killJSON([(7, "terminated", nil), (8, "still_alive", nil)], change: nil))
        let outcome = KillOutcome(code: 0, result: result, language: .zh)
        MainActor.assumeIsolated {
            let model = AppModel(language: .zh, makeEngine: { nil })
            model.close = .result(outcome)
            expect(model.begin(force: false) == nil, "a plain confirm tap on a result does nothing")
            let first = model.begin(force: true)
            expect(first?.pids == [8])
            expect(model.close == .forcing(outcome))
            expect(model.begin(force: true) == nil, "a second tap while forcing sends nothing")
        }
    }

    static func footerShowsRefreshFailure() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: []))
        let now = try require(Panel.parseDate("2026-10-14T19:40:00+08:00"))
        expect(Panel.footer(nil, lastError: nil, interval: 2, language: .zh, now: now) == "每 2 秒刷新")
        expect(Panel.footer(report, lastError: nil, interval: 2, language: .zh, now: now) == "每 2 秒刷新 · 系统进程 6 秒前")
        expect(Panel.footer(report, lastError: "boom", interval: 2, language: .en, now: now)
            == "Refreshes every 2 s · refresh failed: boom; showing the last result")
    }

    static func forceStepReplacesStillAliveAndAddsChanges() throws {
        let first = KillOutcome(
            code: 0, result: try decodeValue(KillResult.self, killJSON([(7, "terminated", nil), (8, "still_alive", nil)], change: 100 << 20)),
            language: .en)
        expect(first.stillAlive == [8])
        let merged = first.merging(
            code: 0, force: try decodeValue(KillResult.self, killJSON([(8, "terminated", nil)], change: 50 << 20)), language: .en)
        expect(merged.lines.map(\.status) == [.terminated, .terminated])
        expect(merged.stillAlive.isEmpty)
        expect(merged.changeBytes == 150 << 20)
        let unmeasured = first.merging(
            code: 0, force: try decodeValue(KillResult.self, killJSON([(8, "terminated", nil)], change: nil)), language: .en)
        expect(unmeasured.change == "not measured")
    }

    static func pillsAndSubtitlesUseRealFields() throws {
        var agent = ownerJSON("a", mem: 1, level: "watch", rule: "idle-agent", kind: "agent_session", pids: [1, 2, 3])
        agent["agent"] = [
            "tool": "claude", "session_id": "s", "name": NSNull(), "name_source": NSNull(), "title": NSNull(),
            "first_prompt": NSNull(), "first_reply": NSNull(), "status": "idle", "cwd": NSNull(), "branch": NSNull(),
            "model": NSNull(), "cost_usd": NSNull(), "confidence": "certain", "updated_at_ms": NSNull(),
            "stale": false, "idle_seconds": 9000,
        ]
        agent["origin"] = [
            "tmux_pane": "%15", "pane_alive": true, "cwd": NSHomeDirectory() + "/src/site", "ports": [5173],
            "window_name": "main", "pane_title": "claude",
        ]
        let owner = try decodeValue(Owner.self, agent)
        expect(Panel.pill(owner, .zh) == "空闲 2 小时 30 分")
        // Phase 08: sessions read tool · window (pane title) · directory · idle.
        expect(Panel.subtitle(owner, .zh) == "claude · main (claude) · site · 空闲 2 小时 30 分")

        var orphan = ownerJSON("o", mem: 1, level: "act", rule: "orphan-dev-server", kind: "orphan")
        orphan["origin"] = ["tmux_pane": "%9", "pane_alive": false, "cwd": "/srv/app", "ports": NSNull()]
        let o = try decodeValue(Owner.self, orphan)
        expect(Panel.pill(o, .en) == "Consider closing")
        expect(Panel.subtitle(o, .en) == "launching pane closed · /srv/app")

        var app = ownerJSON("c", mem: 1, pids: [1, 2, 3, 4], overrides: ["insights": ["a", "b"]])
        app["origin"] = ["tmux_pane": NSNull(), "pane_alive": NSNull(), "cwd": "/", "ports": NSNull()]
        app["parts"] = [
            ["name": "GPU", "mem_bytes": 10, "process_count": 1, "pids": [1], "unknown_mem_count": 0],
            ["name": "扩展进程", "mem_bytes": 30, "process_count": 2, "pids": [2, 3], "unknown_mem_count": 0],
            ["name": "存储服务", "mem_bytes": 5, "process_count": 1, "pids": [4], "unknown_mem_count": 0],
        ]
        let advised = try decodeValue(Owner.self, app)
        expect(Panel.pill(advised, .zh) == "有 2 条建议")
        let one = try decodeValue(Owner.self, ownerJSON("d", mem: 1, overrides: ["insights": ["a"]]))
        expect(Panel.pill(one, .en) == "1 suggestion")
        expect(Panel.subtitle(advised, .zh) == "扩展进程 ×2 · GPU · 4 个进程", "an app shows parts, not cwd /")
        let row = Panel.rows(try decodeValue(Report.self, reportJSON(owners: [app])), sort: .memory, language: .zh)[0]
        expect(row.level == .ok && row.pillLevel == .watch, "advice is highlighted without changing the level")
        let swarm = try decodeValue(Swarm.self, swarmJSON("mcp", mem: 61 * (40 << 20), count: 61))
        expect(Panel.subtitle(swarm, .zh) == "61 份 · 单个约 40M")
        // Phase 10 R19 (owner 2026-10-25): a spread group is used by several apps, nothing to free.
        expect(Panel.pill(swarm, .zh) == "多个应用在用")
    }

    static func headerPressureAndSystemAge() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: []))
        let host = try require(report.host)
        expect(Panel.headline(host, .zh) == "内存偏紧")
        let s = Panel.pressureSegments(host)
        expect(abs(s.used - 33.0 / 68) < 1e-9 && abs(s.compressed - 8.0 / 68) < 1e-9 && abs(s.swap - 20.0 / 68) < 1e-9)
        let now = try require(Panel.parseDate("2026-10-14T19:40:00+08:00"))
        expect(Panel.systemAge(report, now: now) == 6)
    }

    static func formatting() {
        expect(Format.bytes(41_200 << 20) == "40.2G")
        expect(Format.bytes(400 << 20) == "400M")
        expect(Format.signedBytes(1 << 30) == "+1.0G")
        expect(Format.duration(59, .en) == "59s")
        expect(Format.duration(90_061, .zh) == "1 天 1 小时")
    }
}
