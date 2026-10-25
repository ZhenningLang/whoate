// Menu bar panel view model: top rows, short status pills and plain subtitles.
// No whole-sentence conclusions here; the engine's verdict text is for details.
import Foundation

public enum SortKey: String, CaseIterable, Sendable { case memory, cpu }

public struct PanelRow: Identifiable, Equatable, Sendable {
    public enum Source: Equatable, Sendable { case owner, swarm }
    public var id: String
    public var source: Source
    public var title: String
    public var memBytes: UInt64
    public var cpuPct: Double?
    public var subtitle: String
    public var level: Level
    public var pill: String
    /// Colour of the pill: advice on an otherwise normal owner reads as "watch".
    public var pillLevel: Level
    public var history: [HistoryPoint]
    /// Close is offered on actionable owner rows that are not protected.
    public var closable: Bool
    /// Knowledge-base ids; their text comes from `Advice.lines`.
    public var insights: [String] = []
    /// Processes whose memory could not be read; number and curve count the rest.
    public var unknownMemCount = 0
    /// With `cpuPct` nil: the readable processes' CPU and how many were unreadable (R12).
    public var cpuReadable: Double?
    public var unknownCPUCount: Int?
    /// Owner kind and verdict rule (nil for process groups), for main window filters.
    public var kind: OwnerKind?
    public var rule: String?
    /// Process groups only: the owners the group's processes belong to (R14 falls back to one).
    public var members: [String] = []
    /// R21: owners only, the part of `memBytes` in physical memory (MemoryWording.split);
    /// nil when its RSS could not be read, and for process groups.
    public var physicalBytes: UInt64?
}

extension PanelRow {
    /// The row's memory number. "≥" and its count describe this tick only: a
    /// curve that was partly unreadable earlier does not mark the number now
    /// (R5; round 3 conform review B1).
    public func memory(_ language: Language) -> Format.Memory {
        Format.memory(memBytes, unknown: unknownMemCount, language)
    }
    /// The row's CPU: "≥ 12%" over the readable processes when some are not (R12).
    /// CPU order still uses `cpuPct` only (spec R12: sorting unchanged).
    public func cpu(_ language: Language) -> Format.Memory {
        Format.cpu(cpuPct, readable: cpuReadable, unknown: unknownCPUCount, language)
    }
}

/// The panel and main-window header: memory pressure, or whole-machine CPU.
public struct Header: Equatable, Sendable {
    public var headline: String
    /// CPU mode only: load per core and how busy the machine is.
    public var detail: String?
}

public enum CPULevel: Equatable, Sendable { case idle, moderate, busy }

public enum Panel {
    public static let rowLimit = 5

    /// Owners and swarms mixed, by the chosen metric only. Unknown CPU sorts last.
    /// `histories` holds raw owner histories (AppModel); with it, an owner it
    /// lacks gets no curve rather than the tick's gap-hiding downsampled one.
    public static func rows(_ report: Report, sort: SortKey, language: Language, limit: Int = rowLimit,
                            histories: [String: [HistoryPoint]]? = nil) -> [PanelRow] {
        let owners = report.owners.map { o in
            PanelRow(
                id: o.id, source: .owner, title: title(o), memBytes: o.memBytes, cpuPct: o.cpuPct,
                subtitle: subtitle(o, language), level: o.verdict.level, pill: pill(o, language),
                pillLevel: o.verdict.level == .ok && !o.insights.isEmpty && !isProtected(o) ? .watch : o.verdict.level,
                history: histories.map { $0[o.id] ?? [] } ?? o.history,
                closable: o.verdict.level == .act && !isProtected(o), insights: o.insights,
                unknownMemCount: o.unknownMemCount, cpuReadable: o.cpuPctReadable, unknownCPUCount: o.unknownCPUCount,
                kind: o.kind, rule: o.verdict.rule, physicalBytes: MemoryWording.split(o).physical)
        }
        let swarms = report.swarms.map { s in
            // R19: a spread group is normal in the GUI (not in "needs action", grey pill); the core's level stays.
            let level = Spread.isSpread(s) ? .ok : s.verdict.level
            return PanelRow(
                id: s.id, source: .swarm, title: "\(Spread.name(s, language)) × \(s.count)", memBytes: s.memBytes,
                cpuPct: s.cpuPct, subtitle: subtitle(s, language), level: level,
                pill: pill(s, language), pillLevel: level, history: [], closable: false,
                unknownMemCount: s.unknownMemCount, cpuReadable: s.cpuPctReadable, unknownCPUCount: s.unknownCPUCount,
                members: s.owners.map(\.id))
        }
        return Array((owners + swarms).sorted { a, b in
            switch sort {
            case .memory where a.memBytes != b.memBytes:
                return a.memBytes > b.memBytes
            case .cpu:
                switch (a.cpuPct, b.cpuPct) {
                case let (x?, y?) where x != y: return x > y
                case (.some, nil): return true
                case (nil, .some): return false
                default: break
                }
            default: break
            }
            return a.id < b.id
        }.prefix(limit))
    }

    /// System processes and launchd services are never offered for closing.
    public static func isProtected(_ o: Owner) -> Bool {
        o.kind == .system || o.kind == .service || o.verdict.rule == "system"
            || o.verdict.rule.hasPrefix("system.")
    }

    public static func pill(_ o: Owner, _ language: Language) -> String {
        let rule = o.verdict.rule
        switch rule {
        case "system": return L10n.text(.systemProcess, language)
        case _ where rule.hasPrefix("system."): return L10n.text(.systemService, language)
        case "orphan-dev-server", "orphan-heavy": return L10n.text(.considerClosing, language)
        case "leak-suspect": return L10n.text(.possibleLeak, language)
        case "fast-growth": return L10n.text(.growing, language)
        case "idle-agent":
            let idle = o.agent?.idleSeconds.map { Format.duration($0, language) } ?? "—"
            return L10n.format(.idleFor, language, idle)
        default: break
        }
        switch o.verdict.level {
        case .act: return L10n.text(.considerClosing, language)
        case .watch: return L10n.text(.watch, language)
        default:
            switch o.insights.count {
            case 0: return L10n.text(.normal, language)
            case 1: return L10n.text(.adviceOne, language)
            case let n: return L10n.format(.adviceCount, language, "\(n)")
            }
        }
    }

    public static func pill(_ s: Swarm, _ language: Language) -> String {
        switch s.shape {
        case .burst: return L10n.text(.possibleRunaway, language)
        case .spread: return L10n.text(.usedByApps, language)
        case .unknown: return L10n.text(s.verdict.level == .ok ? .normal : .watch, language)
        }
    }

    /// A name the tool derived itself (`7-74`) gives way to the session title.
    public static func title(_ o: Owner) -> String {
        if let agent = o.agent, agent.nameSource == "derived",
           let title = agent.title?.trimmingCharacters(in: .whitespacesAndNewlines), !title.isEmpty {
            return title
        }
        return o.label
    }

    /// Sessions: tool · tmux window (pane title) · directory · idle time, missing parts omitted.
    public static func agentSubtitle(_ o: Owner, _ agent: Agent, _ language: Language) -> String {
        var parts = [agent.tool]
        if o.origin?.paneAlive == false {
            parts.append(L10n.text(.paneClosed, language))
        } else if let origin = o.origin, let window = origin.windowName ?? origin.tmuxPane {
            parts.append(windowLabel(window, origin))
        }
        if let cwd = Detail.folder(o), cwd != "/" {
            parts.append(URL(fileURLWithPath: cwd).lastPathComponent)
        }
        if let idle = agent.idleSeconds {
            parts.append(L10n.format(.idleFor, language, Format.duration(idle, language)))
        }
        return parts.filter { !$0.isEmpty }.joined(separator: " · ")
    }

    public static func subtitle(_ o: Owner, _ language: Language) -> String {
        if let agent = o.agent { return agentSubtitle(o, agent, language) }
        if RowNames.isGitMonitor(o) {
            // R17: the repository, and how many daemons (one per worktree).
            var parts = [o.origin?.cwd.map { abbreviate($0) } ?? L10n.text(.gitMonitorNoRepo, language)]
            if o.pids.count > 1 { parts.append(L10n.format(.gitMonitorCopies, language, "\(o.pids.count)")) }
            return parts.joined(separator: " · ")
        }
        var parts: [String] = []
        if let origin = o.origin {
            if origin.paneAlive == false {
                parts.append(L10n.text(.paneClosed, language))
            } else if let pane = origin.tmuxPane {
                parts.append("tmux " + (origin.windowName.map { "\($0) \(pane)" } ?? pane))
            }
            if let ports = origin.ports, !ports.isEmpty {
                parts.append(L10n.format(.ports, language, ports.prefix(3).map(String.init).joined(separator: ", ")))
            }
        }
        // An app's working directory says nothing; its largest parts do.
        if o.kind == .app {
            parts += o.parts.sorted { $0.memBytes > $1.memBytes }.prefix(2)
                .map(\.displayName)
        } else if let cwd = o.origin?.cwd ?? o.agent?.cwd, cwd != "/" {
            parts.append(abbreviate(cwd))
        }
        if o.pids.count > 1 { parts.append(L10n.format(.processCount, language, "\(o.pids.count)")) }
        return parts.prefix(3).joined(separator: " · ")
    }

    public static func subtitle(_ s: Swarm, _ language: Language) -> String {
        var parts = [L10n.format(.copies, language, "\(s.count)")]
        if s.count > 0 { parts.append(L10n.format(.eachAbout, language, Format.bytes(s.memBytes / UInt64(s.count)))) }
        return parts.joined(separator: " · ")
    }

    /// tmux window name with the pane title in parentheses when there is one.
    public static func windowLabel(_ window: String, _ origin: Origin) -> String {
        origin.paneTitle.map { "\(window) (\($0))" } ?? window
    }

    public static func abbreviate(_ path: String, home: String = NSHomeDirectory()) -> String {
        path == home ? "~" : path.hasPrefix(home + "/") ? "~" + path.dropFirst(home.count) : path
    }

    public static func header(_ host: Host?, sort: SortKey, _ language: Language) -> Header {
        guard sort == .cpu else { return Header(headline: headline(host, language)) }
        let cpu = host?.cpu
        let headline = if let usage = cpu?.usagePct, cpu?.status == .fresh {
            L10n.format(.cpuHeadline, language, Format.cpu(usage))
        } else {
            L10n.format(.cpuMissing, language, cpuReason(cpu?.reason, language))
        }
        var detail: [String] = []
        if let load = cpu?.load1m {
            let cores = cpu?.logicalCores.map(String.init) ?? "—"
            detail.append(L10n.format(.cpuLoad, language, String(format: "%.2f", load), cores))
        }
        if let level = cpuLevel(cpu) {
            detail.append(L10n.text(level == .busy ? .cpuBusy : level == .moderate ? .cpuModerate : .cpuIdle, language))
        }
        return Header(headline: headline, detail: detail.isEmpty ? nil : detail.joined(separator: " · "))
    }

    /// Busy: usage ≥ 85% or load ≥ 1.0 per core; fairly busy: ≥ 50% or ≥ 0.7.
    /// Either signal alone decides when the other is missing; nil when both are.
    public static func cpuLevel(_ cpu: HostCPU?) -> CPULevel? {
        let usage = cpu?.status == .fresh ? cpu?.usagePct : nil
        let perCore = cpu.flatMap { c in c.load1m.flatMap { l in c.logicalCores.flatMap { $0 > 0 ? l / Double($0) : nil } } }
        guard usage != nil || perCore != nil else { return nil }
        if (usage ?? 0) >= 85 || (perCore ?? 0) >= 1.0 { return .busy }
        if (usage ?? 0) >= 50 || (perCore ?? 0) >= 0.7 { return .moderate }
        return .idle
    }

    static func cpuReason(_ code: String?, _ language: Language) -> String {
        switch code {
        case "first_sample": L10n.text(.cpuReasonFirst, language)
        case "counter_reset": L10n.text(.cpuReasonReset, language)
        case "host_statistics_failed": L10n.text(.cpuReasonRead, language)
        case "no_elapsed_ticks": L10n.text(.cpuReasonNoTicks, language)
        case nil: L10n.text(.cpuReasonAbsent, language)
        case let other?: other
        }
    }

    public static func headline(_ host: Host?, _ language: Language) -> String {
        switch host?.pressure {
        case .normal: return L10n.text(.pressureNormal, language)
        case .warn: return L10n.text(.pressureWarn, language)
        case .critical: return L10n.text(.pressureCritical, language)
        default: return L10n.text(.pressureUnknown, language)
        }
    }

    /// R21: the physical memory bar, as fractions of the machine's total: the used part
    /// other than the compressor's, and the compressor's (both together are "used").
    public static func physicalSegments(_ host: Host) -> (other: Double, compressed: Double) {
        let total = Double(host.memTotalBytes)
        guard total > 0 else { return (0, 0) }
        let compressed = min(host.compressedBytes, host.memUsedBytes)
        return (min(1, Double(host.memUsedBytes - compressed) / total), min(1, Double(compressed) / total))
    }

    /// R21: swap's own bar, used / the swap files' current total; nil without a total
    /// (swap is then only in the text). Swap is not physical memory: never on that bar.
    public static func swapFraction(_ host: Host) -> Double? {
        guard let total = host.swapTotalBytes, total > 0 else { return nil }
        return min(1, Double(host.swapUsedBytes) / Double(total))
    }

    /// Footer line: the refresh interval, then the last refresh error while the
    /// shown report is stale, else the age of the slow system-process sample.
    public static func footer(_ report: Report?, lastError: String?, interval: Double, language: Language,
                              now: Date = Date()) -> String {
        let refresh = L10n.format(.refreshEvery, language, String(format: "%g", interval))
        if let lastError { return refresh + " · " + L10n.format(.refreshFailed, language, lastError) }
        guard let report else { return refresh }
        let age = systemAge(report, now: now).map { L10n.format(.systemProcsAgo, language, "\($0)") }
        return refresh + " · " + (age ?? L10n.text(.systemProcsMissing, language))
    }

    /// Seconds since the slow system-process sample, or nil when it is missing.
    public static func systemAge(_ report: Report, now: Date = Date()) -> Int? {
        guard report.systemProcs.status != .missing, let asOf = report.systemProcs.asOf,
              let date = parseDate(asOf) else { return nil }
        return max(0, Int(now.timeIntervalSince(date).rounded()))
    }

    nonisolated(unsafe) private static let fractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()
    nonisolated(unsafe) private static let whole = ISO8601DateFormatter()

    /// RFC 3339 with or without fractional seconds (the engine emits both).
    static func parseDate(_ text: String) -> Date? {
        fractional.date(from: text) ?? whole.date(from: text)
    }
}

/// Knowledge-base text behind the panel's "N suggestions" pill.
public enum Advice {
    public struct Item: Equatable, Sendable {
        public var id: String
        public var cause: String?
        public var advice: String?
        public var source: String?
    }

    /// Loaded once from the library; the catalogue does not change at run time.
    public static let catalog: [String: Insight] =
        Dictionary(Engine.insights().map { ($0.id, $0) }, uniquingKeysWith: { first, _ in first })

    /// Unknown ids stay as bare ids.
    public static func items(_ ids: [String], catalog: [String: Insight], language: Language) -> [Item] {
        ids.map { id in
            guard let entry = catalog[id] else { return Item(id: id) }
            return Item(id: id, cause: entry.cause.text(language), advice: entry.advice.text(language), source: entry.source)
        }
    }

    /// Tooltip and expanded text: cause and advice for each entry, then its source.
    public static func lines(_ ids: [String], catalog: [String: Insight] = catalog, language: Language) -> String {
        items(ids, catalog: catalog, language: language).map { item in
            guard let cause = item.cause, let advice = item.advice else { return item.id }
            var text = L10n.format(.adviceCause, language, cause) + "\n" + L10n.format(.adviceAdvice, language, advice)
            if let source = item.source { text += "\n" + L10n.format(.adviceSource, language, source) }
            return text
        }.joined(separator: "\n\n")
    }
}
