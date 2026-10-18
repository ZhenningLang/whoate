import Foundation
import AppKit
@testable import WhoateKit

enum RefreshTests {
    final class Spy: RefreshEngine, @unchecked Sendable {
        private let lock = NSLock()
        private var made = 0, ticks = 0, hosts = 0, inFlight = 0, maxFlight = 0
        private var activity: [Bool] = []
        private var delays: [Double] = []
        private var failHost = false, failTick = false
        let report: Report
        let sample: HostSample
        init() throws {
            let bytes = Data("""
            {"schema":"whoate/v1","taken_at":"2026-10-19T00:00:00Z","host":{"mem_total_bytes":8192,"mem_used_bytes":4096,"compressed_bytes":0,"wired_bytes":0,"swap_used_bytes":0,"pressure":"normal"},"system_procs":{"status":"missing"},"owners":[],"swarms":[],"issues":[]}
            """.utf8)
            report = try JSONDecoder().decode(Report.self, from: bytes)
            sample = try JSONDecoder().decode(HostSample.self, from: Data("""
            {"schema":"whoate/host-v1","taken_at":"2026-10-19T00:00:01Z","host":{"mem_total_bytes":8192,"mem_used_bytes":2048,"compressed_bytes":0,"wired_bytes":0,"swap_used_bytes":0,"pressure":"normal"}}
            """.utf8))
        }
        func make() -> (any RefreshEngine)? { lock.withLock { made += 1 }; return self }
        func tick() -> Decoded<Report> {
            lock.withLock { ticks += 1; inFlight += 1; maxFlight = max(maxFlight, inFlight) }
            Thread.sleep(forTimeInterval: 0.02)
            lock.withLock { inFlight -= 1 }
            return lock.withLock { failTick ? .failure(FFIError(code: 1, message: "full_failure")) : .value(report) }
        }
        func host() -> Decoded<HostSample> {
            lock.withLock { hosts += 1; return failHost ? .failure(FFIError(code: 1, message: "host_failure")) : .value(sample) }
        }
        func failing(host: Bool, tick: Bool) { lock.withLock { failHost = host; failTick = tick } }
        func setActive(_ active: Bool) -> Decoded<RefreshControl> {
            lock.withLock { activity.append(active) }
            return decode(RefreshControl.self, from: Data("{\"active\":\(active)}".utf8))
        }
        func delay(_ seconds: Double) async throws {
            lock.withLock { delays.append(seconds) }
            try await Task.sleep(for: .milliseconds(10))
        }
        func state() -> (made: Int, ticks: Int, hosts: Int, maxFlight: Int, activity: [Bool], delays: [Double]) {
            lock.withLock { (made, ticks, hosts, maxFlight, activity, delays) }
        }
        func now() -> Double { lock.withLock { Double(ticks) * 0.25 } }
    }
    @MainActor static func wait(_ condition: () -> Bool) async {
        for _ in 0..<200 {
            if condition() { return }
            try? await Task.sleep(for: .milliseconds(5))
        }
        fail("refresh condition timed out")
    }
    @MainActor static func hiddenIsHostOnlyAndVisibleUsesOneEngine() async throws {
        let spy = try Spy()
        let model = AppModel(language: .en, makeEngine: { spy.make() }, sampleHost: { spy.host() }, sleep: { try await spy.delay($0) })
        model.start()
        model.start()
        await wait { spy.state().hosts > 0 }
        expect(spy.state().made == 0, "cold hidden must not create an engine")
        expect(spy.state().ticks == 0)
        expect(model.host?.memUsedBytes == 2048)
        model.setVisible(panel: true)
        await wait { model.report != nil }
        expect(spy.state().made == 1)
        model.setVisible(main: true)
        model.setVisible(panel: false)
        let previous = spy.state().ticks
        await wait { spy.state().ticks > previous }
        model.setVisible(main: false)
        await wait { spy.state().activity.last == false }
        let report = model.report
        await wait { spy.state().hosts > 1 }
        expect(model.report == report, "host must preserve full report/history")
        model.setVisible(panel: true)
        await wait { spy.state().activity.last == true }
        model.stop()
        await wait { spy.state().activity.last == false }
        expect(spy.state().made == 1)
        expect(spy.state().maxFlight == 1)
        expect(spy.state().delays.contains(where: { $0 > 0 && $0 <= 2 }))
        expect(spy.state().delays.contains(where: { $0 > 0 && $0 <= 10 && $0 != 2 }))
    }

    @MainActor static func failuresPreserveBothGoodValuesAndIndependentErrors() async throws {
        let spy = try Spy()
        let model = AppModel(language: .en, makeEngine: { spy.make() }, sampleHost: { spy.host() }, sleep: { try await spy.delay($0) })
        model.start()
        await wait { model.host != nil }
        let goodHost = model.host
        let goodAt = model.hostTakenAt
        spy.failing(host: true, tick: true)
        await wait { model.hostError != nil }
        expect(model.host == goodHost)
        expect(model.hostTakenAt == goodAt)
        model.setVisible(main: true)
        await wait { model.lastError != nil }
        expect(model.refreshError == "full_failure; host_failure")
        spy.failing(host: true, tick: false)
        await wait { model.report != nil }
        expect(model.hostError == "host_failure", "full success must not erase host failure")
        let goodReport = model.report
        spy.failing(host: false, tick: true)
        await wait { model.lastError != nil }
        expect(model.report == goodReport)
        model.setVisible(main: false)
        await wait { model.hostError == nil }
        expect(model.lastError == "full_failure", "host success must not erase full failure")
        model.stop()
    }

    @MainActor static func stopDropsLateTickAndRestartKeepsOneEngine() async throws {
        let spy = try Spy()
        let model = AppModel(language: .en, makeEngine: { spy.make() }, sampleHost: { spy.host() }, sleep: { try await spy.delay($0) })
        model.setVisible(panel: true)
        model.start()
        await wait { spy.state().ticks == 1 }
        model.stop()
        await wait { spy.state().activity.last == false }
        expect(model.report == nil, "stopped in-flight tick must not publish")
        model.start()
        await wait { model.report != nil }
        expect(spy.state().made == 1)
        expect(spy.state().maxFlight == 1)
        model.stop()
        await wait { spy.state().activity.last == false }
    }

    @MainActor static func realWindowAdapterPublishesShowOrderOutAndClose() async throws {
        _ = NSApplication.shared
        let spy = try Spy()
        let model = AppModel(language: .en, makeEngine: { spy.make() }, sampleHost: { spy.host() }, sleep: { try await spy.delay($0) })
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 80, height: 60), styleMask: [.titled], backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        let view = RefreshVisibility.VisibilityView { model.setVisible(main: $0) }
        window.contentView = view
        model.start()
        window.orderFront(nil)
        await wait { model.fullRefresh && model.report != nil }
        expect(!window.isKeyWindow, "visibility must not require focus")
        window.orderOut(nil)
        await wait { !model.fullRefresh && spy.state().activity.last == false }
        window.orderFront(nil)
        await wait { model.fullRefresh }
        window.close()
        await wait { !model.fullRefresh }
        view.disconnect()
        model.stop()
    }

    @MainActor static func nativeHostDecodesTheNewAbi() async throws {
        switch await Task.detached(operation: { Engine.host() }).value {
        case .value(let sample): expect(sample.host.memTotalBytes > 0); expect(!sample.takenAt.isEmpty)
        case .failure(let error): fail("host ABI failed: \(error.message)")
        }
    }

    @MainActor static func oldCloseCannotOverwriteAReattachedVisibleWindow() async throws {
        _ = NSApplication.shared
        var events: [Bool] = []
        let first = NSWindow(contentRect: NSRect(x: 0,y: 0,width: 80,height: 60), styleMask: [.titled], backing: .buffered, defer: false)
        let second = NSWindow(contentRect: NSRect(x: 0,y: 0,width: 80,height: 60), styleMask: [.titled], backing: .buffered, defer: false)
        first.isReleasedWhenClosed = false
        second.isReleasedWhenClosed = false
        let view = RefreshVisibility.VisibilityView { events.append($0) }
        first.contentView = view
        first.orderFront(nil)
        view.publish()
        await wait { events.last == true }
        NotificationCenter.default.post(name: NSWindow.willCloseNotification, object: first)
        first.contentView = nil
        second.contentView = view
        second.orderFront(nil)
        view.publish()
        let marker = events.count
        try? await Task.sleep(for: .milliseconds(30))
        expect(!events.dropFirst(marker).contains(false), "queued old close must not hide the new host")
        expect(events.last == true)
        view.disconnect()
        first.close()
        second.close()
    }

    @MainActor static func visibleCadenceIncludesWorkRatherThanAddingTwoSeconds() async throws {
        let spy = try Spy()
        let model = AppModel(language: .en, makeEngine: { spy.make() }, sampleHost: { spy.host() },
                             now: { spy.now() }, sleep: { try await spy.delay($0) })
        model.setVisible(panel: true)
        model.start()
        await wait { !spy.state().delays.isEmpty }
        // Fake work consumes .25 s; a 2 s start-to-start cadence waits 1.75 s.
        expect(spy.state().delays.first == 1.75)
        model.stop()
        await wait { spy.state().activity.last == false }
    }
}
