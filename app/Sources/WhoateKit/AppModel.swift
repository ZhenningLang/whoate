// App state: periodic ticks off the main thread and the two-step close flow.
import Foundation
import Observation

@MainActor @Observable
public final class AppModel {
    /// Visible views retain the approved 2 s cadence. Hidden menus only sample host.
    public static let refreshInterval: Double = 2
    public static let hiddenInterval: Double = 5

    public enum CloseFlow: Equatable {
        case idle
        case confirm(KillConfirmation)
        case running(KillConfirmation)
        case result(KillOutcome)
        case forcing(KillOutcome)
        case failed(String)
    }

    public let language: Language
    public var sort: SortKey = .memory
    public private(set) var report: Report?
    public private(set) var engineFailed = false
    public private(set) var lastError: String?
    public private(set) var host: Host?
    public private(set) var hostError: String?
    public private(set) var hostTakenAt: String?
    public var refreshError: String? {
        let errors = [lastError, hostError].compactMap { $0 }
        return errors.isEmpty ? nil : errors.joined(separator: "; ")
    }
    public var close: CloseFlow = .idle
    public var selectedOwnerID: String?

    @ObservationIgnored private var engine: (any RefreshEngine)?
    @ObservationIgnored private let makeEngine: @Sendable () -> (any RefreshEngine)?
    @ObservationIgnored private let sampleHost: @Sendable () -> Decoded<HostSample>
    @ObservationIgnored private let sleep: @Sendable (Double) async throws -> Void
    @ObservationIgnored private let trace: @Sendable (String) -> Void
    @ObservationIgnored private let now: @Sendable () -> Double
    @ObservationIgnored private var loop: Task<Void, Never>?
    @ObservationIgnored private var sleeper: Task<Void, Error>?
    @ObservationIgnored private var running = false
    @ObservationIgnored private var refreshing = false
    @ObservationIgnored private var revision: UInt64 = 0
    @ObservationIgnored private var panelVisible = false
    @ObservationIgnored private var mainVisible = false
    @ObservationIgnored private var engineIsActive = false

    public init(language: Language, makeEngine: @escaping @Sendable () -> (any RefreshEngine)?,
                sampleHost: @escaping @Sendable () -> Decoded<HostSample> = { Engine.host() },
                trace: @escaping @Sendable (String) -> Void = { _ in },
                now: @escaping @Sendable () -> Double = { ProcessInfo.processInfo.systemUptime },
                sleep: @escaping @Sendable (Double) async throws -> Void = { try await Task.sleep(for: .seconds($0)) }) {
        self.language = language
        self.makeEngine = makeEngine
        self.sampleHost = sampleHost
        self.sleep = sleep
        self.trace = trace
        self.now = now
    }
    public var fullRefresh: Bool { panelVisible || mainVisible }
    public func setVisible(panel: Bool? = nil, main: Bool? = nil) {
        let previous = fullRefresh
        if let panel { panelVisible = panel }
        if let main { mainVisible = main }
        guard fullRefresh != previous else { return }
        revision &+= 1
        sleeper?.cancel()
    }

    public var rows: [PanelRow] {
        report.map { Panel.rows($0, sort: sort, language: language) } ?? []
    }

    public func owner(_ id: String) -> Owner? { report?.owners.first { $0.id == id } }

    public func start() {
        guard !running else { return }
        running = true
        revision &+= 1
        guard loop == nil else { return }
        loop = Task { [weak self] in
            await self?.runLoop()
        }
    }

    public func stop() {
        running = false
        revision &+= 1
        sleeper?.cancel()
    }

    private func runLoop() async {
        while running {
            let current = revision
            let began = now()
            await refresh()
            guard running else { break }
            if current != revision { continue }
            let interval = fullRefresh ? Self.refreshInterval : Self.hiddenInterval
            // Start-to-start cadence. A slow tick never creates a catch-up queue.
            let delay = max(0, interval - max(0, now() - began))
            let sleep = sleep
            let task = Task { try await sleep(delay) }
            sleeper = task
            try? await task.value
            sleeper = nil
        }
        // One loop owns both in-flight work and suspension. stop/start never
        // creates a second engine or lets a cancelled detached tick write back.
        if let engine, engineIsActive {
            let result = await Task.detached { engine.setActive(false) }.value
            if case .failure(let error) = result { hostError = error.message }
            engineIsActive = false
            trace("pause")
        }
        loop = nil
        if running { running = false; start() }
    }

    /// One scheduled sample: host when hidden, full tick when visible. Failed
    /// engine starts retry once per cadence, never in a catch-up burst.
    public func refresh() async {
        guard running, !refreshing else { return }
        refreshing = true
        defer { refreshing = false }
        let current = revision
        if !fullRefresh {
            if let engine, engineIsActive {
                let result = await Task.detached { engine.setActive(false) }.value
                if case .failure(let error) = result { hostError = error.message; return }
                engineIsActive = false
                trace("pause")
            }
            guard running, revision == current else { return }
            let sample = sampleHost
            trace("host")
            let value = await Task.detached(priority: .utility) { sample() }.value
            guard running, revision == current else { return }
            switch value {
            case .value(let sample): host = sample.host; hostTakenAt = sample.takenAt; hostError = nil
            case .failure(let error): hostError = error.message
            }
            return
        }
        if engine == nil {
            let make = makeEngine
            engine = await Task.detached(priority: .utility) { make() }.value
            engineIsActive = engine != nil
            engineFailed = engine == nil
            if engine == nil { return }
        }
        guard let engine else { return }
        guard running, revision == current else { return }
        if !engineIsActive {
            let result = await Task.detached { engine.setActive(true) }.value
            if case .failure(let error) = result { lastError = error.message; return }
            engineIsActive = true
            trace("resume")
        }
        guard running, revision == current else { return }
        trace("full")
        let decoded = await Task.detached(priority: .utility) { engine.tick() }.value
        guard running, revision == current else { return }
        switch decoded {
        case .value(let report):
            self.report = report
            host = report.host
            if report.host != nil { hostTakenAt = report.takenAt }
            lastError = nil
        case .failure(let error):
            lastError = error.message
        }
    }

    public func requestClose(_ ownerID: String) {
        guard let owner = owner(ownerID) else { return }
        close = .confirm(KillConfirmation(owner: owner, language: language))
    }

    public func cancelClose() { close = .idle }

    /// Step one: SIGTERM, wait five seconds. Step two (force) only for pids the
    /// library reported still alive after step one.
    public func confirmClose(force: Bool = false) async {
        guard let step = begin(force: force) else { return }
        let (language, pids) = (language, step.pids)
        let (code, result) = await Task.detached { Engine.kill(pids: pids, force: force) }.value
        // The flow moved on while waiting (closed or replaced): drop the late answer.
        guard close == step.state else { return }
        switch (result, step.state) {
        case (.value(let value), .forcing(let outcome)):
            close = .result(outcome.merging(code: code, force: value, language: language))
        case (.value(let value), _):
            close = .result(KillOutcome(code: code, result: value, language: language))
        case (.failure(let error), _):
            close = .failed(L10n.format(.killFailed, language, error.message))
        }
    }

    /// The synchronous half of a step: leaves .confirm/.result before any await,
    /// so a second tap during the wait finds nothing to send.
    func begin(force: Bool) -> (state: CloseFlow, pids: [Int32])? {
        switch close {
        case .confirm(let confirmation) where !force && !confirmation.isProtected:
            close = .running(confirmation)
            return (close, confirmation.pids)
        case .result(let outcome) where force && !outcome.stillAlive.isEmpty:
            close = .forcing(outcome)
            return (close, outcome.stillAlive)
        default:
            return nil
        }
    }
}
