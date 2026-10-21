// App state: periodic ticks off the main thread and the two-step close flow.
import Foundation
import Observation

@MainActor @Observable
public final class AppModel {
    /// Visible views retain the approved 2 s cadence. Hidden menus sample host
    /// every 5 s and make one full tick, engine still paused, every 30 s.
    public static let refreshInterval: Double = 2
    public static let hiddenInterval: Double = 5
    public static let hiddenFullInterval: Double = 30

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
    /// A jump message belongs to the owner it was for: a new selection clears it.
    public var selectedOwnerID: String? { didSet { if selectedOwnerID != oldValue { notice = nil } } }
    /// Main window sidebar selection.
    public var category: MainCategory = .all
    /// The action bar's message for the selected owner: a tmux jump result (no client,
    /// no terminal, failure; nil after success) or "copied".
    public private(set) var notice: String?
    /// Main window list rows on screen now (onAppear/onDisappear); they get full histories.
    @ObservationIgnored private var visibleMainRows = Set<String>()
    /// ceiling: one whoate_why per visible row per visible tick; a tall window shows
    /// about 30 rows, so at most this many are fetched beyond the first rows.
    public static let visibleHistoryRows = 40
    /// Main window rows that get a full history while it is open (design round 1, finding 2).
    /// ceiling: one whoate_why per row per visible tick (P08-PANEL-WHY-COST); with the
    /// visible rows below, at most 60 per tick. Batch the FFI call if QA26 (recorded) grows.
    public static let mainHistoryRows = 20
    /// Panel row whose advice text is expanded.
    public var expandedAdvice: String?
    /// Raw (not downsampled) history of the owners the panel can show, by owner id.
    /// A tick's own history is capped at 60 evenly spaced points, which hides gaps.
    public private(set) var histories: [String: [HistoryPoint]] = [:]
    /// The selected owner's processes from its whoate_why (R4, R8); empty until it arrives.
    public private(set) var selectedProcs: (owner: String, procs: [ProcBrief])?
    public func procs(_ ownerID: String) -> [ProcBrief] {
        selectedProcs?.owner == ownerID ? selectedProcs?.procs ?? [] : []
    }
    /// CPU-mode order shared by the panel and the main window (10 s average, hover freeze).
    public private(set) var cpuOrder = CPUOrder()

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
    /// Start of the latest full tick, visible or hidden, on the `now` clock.
    @ObservationIgnored private var lastFullAt: Double?

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
        // A closing panel may never report the pointer leaving its list, and a
        // late hover-in can arrive after it closed: any panel change unfreezes.
        if panel != nil { cpuOrder.freeze(false) }
        let previous = fullRefresh
        if let panel { panelVisible = panel }
        if let main { mainVisible = main }
        guard fullRefresh != previous else { return }
        revision &+= 1
        sleeper?.cancel()
    }

    public var rows: [PanelRow] { rows(limit: Panel.rowLimit) }

    public func setMainRow(_ id: String, visible: Bool) {
        if visible { visibleMainRows.insert(id) } else { visibleMainRows.remove(id) }
    }

    /// A tmux jump finished: its message shows only if its owner is still the selected one.
    public func finishJump(_ message: String?, for ownerID: String) {
        if selectedOwnerID == ownerID { notice = message }
    }

    /// Panel and main window rows. Memory order is Panel.rows' own; CPU order is `cpuOrder`.
    /// Names are made unique over the whole list, then cut to `limit`.
    public func rows(limit: Int) -> [PanelRow] {
        guard let report else { return [] }
        let all = Panel.rows(report, sort: sort, language: language, limit: .max, histories: histories)
        let ordered = sort == .cpu ? cpuOrder.arrange(all) : all
        return Array(RowNames.apply(ordered, report: report, language: language).prefix(limit))
    }

    /// The pointer is over the panel list: CPU order holds still, values keep updating.
    public func setListHovered(_ hovered: Bool) { cpuOrder.freeze(hovered) }

    public func owner(_ id: String) -> Owner? { report?.owners.first { $0.id == id } }

    /// WHOATE_UI_SELECT and a pane's session list (R3) share this one selection path.
    public func select(_ value: String) {
        let (category, id) = MainWindowModel.select(value, rows: rows(limit: .max))
        self.category = category
        selectedOwnerID = id
    }

    /// "已复制" in the action bar's message for two seconds (R4).
    public func markCopied(_ ownerID: String) {
        guard selectedOwnerID == ownerID else { return }
        let text = L10n.text(.copied, language)
        notice = text
        let sleep = sleep
        Task { [weak self] in
            try? await sleep(2)
            if self?.notice == text { self?.notice = nil }
        }
    }

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
            if lastFullAt.map({ now() - $0 >= Self.hiddenFullInterval - 0.5 }) ?? true {
                if await hiddenFull(current) { return }
                guard running, revision == current else { return }
            }
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
        lastFullAt = now()
        let decoded = await Task.detached(priority: .utility) { engine.tick() }.value
        guard running, revision == current else { return }
        switch decoded {
        case .value(let report):
            let at = now()
            // A copy only to know the CPU top rows for histories; the shown
            // order changes once, in publish, after the histories arrive.
            var order = cpuOrder
            order.record(Self.cpuValues(report), at: at)
            // Either sort can be shown next; fetch both top lists (at most 2 × rowLimit owners).
            // Plus, with the main window open, its selected owner and its first rows
            // in the current category, so their curves are drawn there too.
            var wanted = Set(Panel.rows(report, sort: .memory, language: language).map(\.id) + order.order.prefix(Panel.rowLimit))
            if mainVisible {
                let all = Panel.rows(report, sort: sort, language: language, limit: .max)
                let listed = (sort == .cpu ? order.arrange(all) : all).filter { MainWindowModel.matches($0, category) }
                wanted.formUnion(listed.prefix(Self.mainHistoryRows).map(\.id) + [selectedOwnerID].compactMap { $0 })
                wanted.formUnion(visibleMainRows.sorted().prefix(Self.visibleHistoryRows))
            }
            let targets = report.owners.filter { wanted.contains($0.id) }.compactMap { o in o.pids.first.map { (o.id, $0) } }
            let selected = mainVisible ? selectedOwnerID : nil
            let full = await Task.detached(priority: .utility) {
                Dictionary(uniqueKeysWithValues: targets.compactMap { id, pid in engine.detail(pid: pid, owner: id).map { (id, $0) } })
            }.value
            guard running, revision == current else { return }
            histories = full.mapValues(\.history)
            let procs = selected.flatMap { id in full[id].map { (owner: id, procs: $0.procs) } }
            if procs?.owner != selectedProcs?.owner || procs?.procs != selectedProcs?.procs { selectedProcs = procs }
            publish(report, at: at)
        case .failure(let error):
            lastError = error.message
        }
    }

    /// Hidden: one full tick per hiddenFullInterval with the engine left paused
    /// (resuming restarts the slow worker and its `top`). No histories: nothing
    /// draws them until a view is visible. False when the caller should sample host instead.
    private func hiddenFull(_ current: UInt64) async -> Bool {
        lastFullAt = now()
        if engine == nil {
            let make = makeEngine
            let made = await Task.detached(priority: .utility) { make() }.value
            engineFailed = made == nil
            guard let made else { return false }
            engine = made
            engineIsActive = true
            // A new engine starts active; pause it before its first tick.
            let result = await Task.detached { made.setActive(false) }.value
            if case .failure(let error) = result { hostError = error.message; return false }
            engineIsActive = false
        }
        guard running, revision == current, let engine, !engineIsActive else { return false }
        trace("hidden-full")
        let decoded = await Task.detached(priority: .utility) { engine.tick() }.value
        guard running, revision == current else { return true }
        switch decoded {
        case .value(let report):
            publish(report, at: now())
            return true
        case .failure(let error):
            lastError = error.message
            return false
        }
    }

    static func cpuValues(_ report: Report) -> [(id: String, pct: Double?)] {
        report.owners.map { ($0.id, $0.cpuPct) } + report.swarms.map { ($0.id, $0.cpuPct) }
    }

    /// `at` is the tick's time on the `now` clock (the CPU average window).
    private func publish(_ report: Report, at: Double) {
        self.report = report
        cpuOrder.record(Self.cpuValues(report), at: at)
        host = report.host
        if report.host != nil { hostTakenAt = report.takenAt }
        lastError = nil
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
