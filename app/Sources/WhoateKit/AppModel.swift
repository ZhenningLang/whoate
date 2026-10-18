// App state: periodic ticks off the main thread and the two-step close flow.
import Foundation
import Observation

@MainActor @Observable
public final class AppModel {
    /// U-SPK-6 (phases/05_gui/spike_cpu.md): 1 s missed the 1% CPU budget, so this
    /// is owner's approved 2 s fallback; 2 s also misses it. Core speed-up is Phase 09.
    public static let refreshInterval: Double = 2

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
    public var close: CloseFlow = .idle
    public var selectedOwnerID: String?

    @ObservationIgnored private var engine: Engine?
    @ObservationIgnored private let makeEngine: @Sendable () -> Engine?
    @ObservationIgnored private var loop: Task<Void, Never>?

    public init(language: Language, makeEngine: @escaping @Sendable () -> Engine?) {
        self.language = language
        self.makeEngine = makeEngine
    }

    public var rows: [PanelRow] {
        report.map { Panel.rows($0, sort: sort, language: language) } ?? []
    }

    public func owner(_ id: String) -> Owner? { report?.owners.first { $0.id == id } }

    public func start() {
        guard loop == nil else { return }
        loop = Task { [weak self] in
            let clock = ContinuousClock()
            var deadline = clock.now
            while !Task.isCancelled {
                await self?.refresh()
                deadline = max(deadline.advanced(by: .seconds(Self.refreshInterval)), clock.now)
                try? await Task.sleep(until: deadline, clock: clock)
            }
        }
    }

    public func stop() {
        loop?.cancel()
        loop = nil
    }

    /// One tick. A failed engine start is retried once per refresh, never in a burst.
    public func refresh() async {
        if engine == nil {
            let make = makeEngine
            engine = await Task.detached(priority: .utility) { make() }.value
            engineFailed = engine == nil
            if engine == nil { return }
        }
        guard let engine else { return }
        let decoded = await Task.detached(priority: .utility) { engine.tick() }.value
        switch decoded {
        case .value(let report):
            self.report = report
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
