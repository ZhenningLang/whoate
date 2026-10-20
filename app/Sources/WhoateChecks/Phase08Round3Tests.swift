// Phase 08a round 3 (owner 2026-10-20 20:4x): CPU order holds still (QA30),
// hidden full samples every 30 s with the engine paused (QA31), curve gap
// limit and window (QA32).
import Foundation
@testable import WhoateKit

enum Phase08Round3Tests {
    final class Clock: @unchecked Sendable {
        private let lock = NSLock()
        private var t = 0.0, hold = Double.infinity
        var now: Double { lock.withLock { t } }
        func set(_ value: Double) { lock.withLock { t = value } }
        func hold(at value: Double) { lock.withLock { hold = value } }
        /// Advances by `seconds`, then waits (cancellable) while at or past the hold.
        func sleep(_ seconds: Double) async throws {
            lock.withLock { t += seconds }
            while lock.withLock({ t >= hold }) { try await Task.sleep(for: .milliseconds(1)) }
        }
    }

    /// Returns whatever report the test set last; logs every engine call in order.
    final class Feed: RefreshEngine, @unchecked Sendable {
        private let lock = NSLock()
        private var current: Report
        private var log: [String] = []
        var raw: [String: [HistoryPoint]] = [:]
        init(_ report: Report) { current = report }
        var report: Report {
            get { lock.withLock { current } }
            set { lock.withLock { current = newValue } }
        }
        var events: [String] { lock.withLock { log } }
        func made() -> Feed { lock.withLock { log.append("make") }; return self }
        func tick() -> Decoded<Report> { lock.withLock { log.append("tick"); return .value(current) } }
        func setActive(_ active: Bool) -> Decoded<RefreshControl> {
            lock.withLock { log.append("active(\(active))") }
            return decode(RefreshControl.self, from: Data("{\"active\":\(active)}".utf8))
        }
        func history(pid: Int32, owner: String) -> [HistoryPoint]? {
            lock.withLock { log.append("history"); return raw[owner] }
        }
    }

    final class Trace: @unchecked Sendable {
        private let lock = NSLock()
        private var rows: [(Double, String)] = []
        func append(_ at: Double, _ mode: String) { lock.withLock { rows.append((at, mode)) } }
        var entries: [(Double, String)] { lock.withLock { rows } }
    }

    static func report(_ cpu: [(String, Double?)], mem: [String: UInt64] = [:]) throws -> Report {
        try decodeValue(Report.self, reportJSON(owners: cpu.enumerated().map { i, entry in
            ownerJSON(entry.0, mem: mem[entry.0] ?? UInt64(100 - i), cpu: entry.1, pids: [Int32(200 + i)])
        }))
    }

    /// A visible model whose own loop sleeps for good after its first refresh;
    /// the test then drives each refresh at a chosen clock time.
    @MainActor static func driven(_ feed: Feed, clock: Clock, panel: Bool = false, main: Bool = true) async -> AppModel {
        let model = AppModel(language: .en, makeEngine: { feed.made() }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             now: { clock.now }, sleep: { _ in try await Task.sleep(for: .seconds(3600)) })
        model.setVisible(panel: panel, main: main)
        model.sort = .cpu
        model.start()
        await RefreshTests.wait { model.report != nil }
        return model
    }

    @MainActor static func step(_ model: AppModel, _ feed: Feed, _ clock: Clock, at t: Double, _ report: Report) async {
        clock.set(t)
        feed.report = report
        await model.refresh()
    }

    /// QA30: alternating leaders do not swap; the same input swaps under the single-tick rule.
    @MainActor static func cpuOrderHoldsStillWhenLeadersAlternate() async throws {
        let ticks = try (0..<10).map { i in try report(i % 2 == 0 ? [("A", 12), ("B", 8)] : [("A", 8), ("B", 12)]) }
        let clock = Clock()
        let feed = Feed(ticks[0])
        let model = await driven(feed, clock: clock)
        var panel = [model.rows.map(\.id)], main = [model.rows(limit: .max).map(\.id)]
        for (i, tick) in ticks.enumerated().dropFirst() {
            await step(model, feed, clock, at: Double(i * 2), tick)
            panel.append(model.rows.map(\.id))
            main.append(model.rows(limit: .max).map(\.id))
        }
        model.stop()
        expect(panel.count == 10 && Set(panel).count == 1, "panel order changed: \(panel)")
        expect(Set(main).count == 1 && main[0] == panel[0], "main window order changed: \(main)")
        let base = ticks.map { Panel.rows($0, sort: .cpu, language: .en).map(\.id) }
        let swaps = zip(base, base.dropFirst()).filter { $0 != $1 }.count
        expect(swaps >= 5, "the construction must swap under the single-tick rule: \(swaps)")
    }

    /// QA30: the average covers (latest − 10 s, latest]; all-nil rows go last.
    static func cpuAverageUsesOnlyTheLastTenSeconds() {
        var order = CPUOrder()
        order.record([("A", 10), ("B", 1000), ("C", nil)], at: 0)
        order.record([("A", 10), ("B", 900), ("C", nil)], at: 2)
        expect(order.order == ["B", "A", "C"])
        order.record([("A", 10), ("B", 5), ("C", nil)], at: 12)
        expect(order.average("B") == 5, "values at 0 s and 2 s (= 12 − 10) are outside: \(order.average("B") ?? -1)")
        expect(order.average("C") == nil)
        expect(order.order == ["A", "B", "C"], "\(order.order)")
        expect(CPUOrder.window == 10 && CPUOrder.minimumLead == 1 && CPUOrder.relativeLead == 0.15)
        // 40 vs 45 stays; 40 vs 50 swaps (max(1 point, 15 %)).
        var close = CPUOrder()
        close.record([("A", 40), ("B", 39)], at: 0)
        close.record([("A", 40), ("B", 45)], at: 20)
        expect(close.order == ["A", "B"])
        close.record([("A", 40), ("B", 50)], at: 40)
        expect(close.order == ["B", "A"])
        // A new id is placed by its average, not appended.
        close.record([("A", 40), ("B", 50), ("N", 45)], at: 42)
        expect(close.order == ["B", "N", "A"], "\(close.order)")
    }

    /// QA30: a real jump to a sustained 40 % reaches the top within 2 refreshes.
    @MainActor static func cpuOrderLetsARealJumpThrough() async throws {
        let clock = Clock()
        let feed = Feed(try report([("A", 10.5), ("B", 10)]))
        let model = await driven(feed, clock: clock)
        for i in 1...5 { await step(model, feed, clock, at: Double(i * 2), try report([("A", 10.5), ("B", 10)])) }
        expect(model.rows.map(\.id) == ["A", "B"])
        var after: Int?
        for i in 1...2 where after == nil {
            await step(model, feed, clock, at: Double(10 + i * 2), try report([("A", 10), ("B", 40)]))
            if model.rows.first?.id == "B" { after = i }
        }
        model.stop()
        expect(after != nil, "B at 40 % was not ahead of A within 2 refreshes")
    }

    /// QA30: hovering freezes the CPU order (values still update); leaving or
    /// closing the panel reorders at once; memory order is never frozen.
    @MainActor static func hoverFreezesCpuOrderOnly() async throws {
        let clock = Clock()
        let feed = Feed(try report([("A", 20), ("B", 5)]))
        let model = await driven(feed, clock: clock, panel: true)
        model.setListHovered(true)
        for i in 1...3 {
            await step(model, feed, clock, at: Double(i * 2), try report([("A", 20), ("B", 80 + Double(i))]))
            expect(model.rows.map(\.id) == ["A", "B"], "frozen panel order, refresh \(i)")
            expect(model.rows(limit: .max).map(\.id) == ["A", "B"], "the main window follows the frozen order")
            expect(model.rows.last?.cpuPct == 80 + Double(i), "values are fresh while frozen")
        }
        // Memory order is Panel.rows' own, hover or not.
        model.sort = .memory
        let memory = try report([("A", 20), ("B", 83)], mem: ["A": 1, "B": 9])
        await step(model, feed, clock, at: 8, memory)
        expect(model.rows == Panel.rows(memory, sort: .memory, language: .en, histories: model.histories))
        expect(model.rows.map(\.id) == ["B", "A"], "memory reorders while hovered")
        model.sort = .cpu
        model.setListHovered(false)
        expect(model.rows.first?.id == "B", "leaving the list reorders without waiting for a tick")
        model.setListHovered(true)
        model.setVisible(panel: false)
        expect(!model.cpuOrder.frozen, "closing the panel unfreezes")
        model.setListHovered(true)
        model.setVisible(panel: true)
        expect(!model.cpuOrder.frozen, "a hover-in that arrived after closing does not survive reopening")
        model.stop()
        let source = String(decoding: try repoFile("app/Sources/Whoate/MainWindowView.swift"), as: UTF8.self)
        expect(!source.contains("Panel.rows(") && source.contains("model.rows(limit: .max)"),
               "the main window uses the model's one order")
        let panel = String(decoding: try repoFile("app/Sources/Whoate/PanelView.swift"), as: UTF8.self)
        expect(panel.contains(".onHover { model.setListHovered($0) }"))
    }

    /// QA31: hidden, a paused full tick at 0/30/60/90/120 s and host every 5 s
    /// in between; never resumed, no histories. Visible: the first refresh
    /// resumes once and the curve is already a line.
    @MainActor static func hiddenFullTicksEvery30SecondsPaused() async throws {
        let clock = Clock()
        let feed = Feed(try report([("o", 5)]))
        feed.raw = ["o": (0...4).map { HistoryPoint(timestampS: Int64($0 * 30), memBytes: 100, cpuPct: nil) }]
        let trace = Trace()
        let model = AppModel(language: .en, makeEngine: { feed.made() },
                             sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             trace: { trace.append(clock.now, $0) },
                             now: { clock.now }, sleep: { try await clock.sleep($0) })
        func log() -> [(Double, String)] { trace.entries }
        func times(_ mode: String, _ from: Double = 0, _ to: Double = .infinity) -> [Double] {
            log().filter { $0.1 == mode && $0.0 >= from && $0.0 < to }.map(\.0)
        }
        clock.hold(at: 125)
        model.start()
        await RefreshTests.wait { clock.now >= 125 && log().last?.0 == 120 }
        expect(times("hidden-full") == [0, 30, 60, 90, 120], "\(times("hidden-full"))")
        let hosts = times("host")
        expect(hosts == stride(from: 5.0, to: 125, by: 5).filter { $0.truncatingRemainder(dividingBy: 30) != 0 }, "\(hosts)")
        expect(Set(log().map(\.1)) == ["hidden-full", "host"], "no full, resume or pause while hidden")
        expect(Array(feed.events.prefix(3)) == ["make", "active(false)", "tick"], "paused before the first tick")
        expect(!feed.events.contains("active(true)") && !feed.events.contains("history"), "\(feed.events)")
        expect(feed.events.filter { $0 == "make" }.count == 1)
        // Visible: one resume, a full tick with histories, a line after that first refresh.
        clock.hold(at: 127)
        model.setVisible(panel: true)
        await RefreshTests.wait { clock.now >= 127 && log().last?.0 == 125 }
        expect(feed.events.filter { $0 == "active(true)" }.count == 1)
        expect(times("resume", 125) == [125] && times("full", 125) == [125], "\(log().suffix(4))")
        expect(feed.events.filter { $0 == "history" }.count == 1, "histories come with the first visible refresh")
        guard case .line = Format.trend(model.rows.first?.history ?? [], width: 64, height: 16) else {
            return fail("the first visible refresh after 120 s hidden must draw a line")
        }
        clock.hold(at: 131)
        await RefreshTests.wait { clock.now >= 131 && log().last?.0 == 129 }
        expect(times("full", 125) == [125, 127, 129], "\(log().suffix(6))")
        // Hidden again: pause first; the next full tick is ≥ 29.5 s after the last one (129 s).
        clock.hold(at: 170)
        model.setVisible(panel: false)
        await RefreshTests.wait { clock.now >= 170 }
        let after = log().filter { $0.0 >= 131 }
        expect(after.first?.1 == "pause" && after.first?.0 == 131, "\(after.prefix(3))")
        expect(times("hidden-full", 131) == [161], "\(times("hidden-full", 131))")
        expect(times("full", 131).isEmpty && times("resume", 131).isEmpty)
        model.stop()
        clock.hold(at: .infinity)
    }

    /// QA32: 45 s gap limit, 1800 s window, the constants the code uses.
    static func curveGapIsFortyFiveSecondsAndWindowThirtyMinutes() {
        func line(_ seconds: [Int64]) -> [Format.Point]? {
            if case .line(let points) = Format.trend(Phase08Tests.points(seconds), width: 270, height: 16) { return points }
            return nil
        }
        expect(Format.gapLimit == 45 && Format.window == 1800 && Format.minimumSpan == 30)
        let hidden = (0..<10).map { Int64($0) * 30 }
        expect(line(hidden)?.count == 10 && line(hidden)?.last?.x == 270, "ten 30 s points are one 270 s run")
        let mixed = hidden + stride(from: Int64(272), through: 290, by: 2)
        expect(line(mixed)?.count == mixed.count, "hidden 30 s points then visible 2 s points are one run")
        let edge = Format.gapLimit
        expect(line([0, 30, 60, 60 + edge, 60 + edge + 30])?.count == 5, "a 45 s gap does not break")
        expect(line([0, 30, 60, 60 + edge + 1, 60 + edge + 31, 60 + edge + 61])?.count == 3, "a 46 s gap breaks")
        let slept = Array(stride(from: Int64(0), through: 120, by: 30)) + Array(stride(from: Int64(720), through: 840, by: 30))
        expect(line(slept)?.count == 5, "after a 10 min gap only the later run")
        let twoHours = (0...240).map { Int64($0) * 30 }
        guard let long = line(twoHours) else { return fail("two hours of 30 s points") }
        expect(long.count == Int(Format.window / 30) + 1, "only the last 1800 s: \(long.count)")
        expect(Format.trend(Phase08Tests.points([0, 10, 20, 29]), width: 1, height: 1) == .insufficient, "under 30 s")
    }
}
