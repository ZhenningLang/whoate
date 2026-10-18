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
}

public enum Panel {
    public static let rowLimit = 5

    /// Owners and swarms mixed, by the chosen metric only. Unknown CPU sorts last.
    public static func rows(_ report: Report, sort: SortKey, language: Language, limit: Int = rowLimit) -> [PanelRow] {
        let owners = report.owners.map { o in
            PanelRow(
                id: o.id, source: .owner, title: o.label, memBytes: o.memBytes, cpuPct: o.cpuPct,
                subtitle: subtitle(o, language), level: o.verdict.level, pill: pill(o, language),
                pillLevel: o.verdict.level == .ok && !o.insights.isEmpty && !isProtected(o) ? .watch : o.verdict.level,
                history: o.history, closable: o.verdict.level == .act && !isProtected(o))
        }
        let swarms = report.swarms.map { s in
            PanelRow(
                id: s.id, source: .swarm, title: "\(s.label) × \(s.count)", memBytes: s.memBytes,
                cpuPct: s.cpuPct, subtitle: subtitle(s, language), level: s.verdict.level,
                pill: pill(s, language), pillLevel: s.verdict.level, history: [], closable: false)
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
        case .spread: return L10n.text(.sharedAcross, language)
        case .unknown: return L10n.text(s.verdict.level == .ok ? .normal : .watch, language)
        }
    }

    public static func subtitle(_ o: Owner, _ language: Language) -> String {
        var parts: [String] = []
        if let agent = o.agent { parts.append(agent.tool) }
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
                .map { $0.processCount > 1 ? "\($0.name) ×\($0.processCount)" : $0.name }
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

    public static func abbreviate(_ path: String, home: String = NSHomeDirectory()) -> String {
        path == home ? "~" : path.hasPrefix(home + "/") ? "~" + path.dropFirst(home.count) : path
    }

    public static func headline(_ host: Host?, _ language: Language) -> String {
        switch host?.pressure {
        case .normal: return L10n.text(.pressureNormal, language)
        case .warn: return L10n.text(.pressureWarn, language)
        case .critical: return L10n.text(.pressureCritical, language)
        default: return L10n.text(.pressureUnknown, language)
        }
    }

    /// Fractions of (total + swap) for the used, compressed and swap segments.
    public static func pressureSegments(_ host: Host) -> (used: Double, compressed: Double, swap: Double) {
        let whole = Double(host.memTotalBytes) + Double(host.swapUsedBytes)
        guard whole > 0 else { return (0, 0, 0) }
        let compressed = min(host.compressedBytes, host.memUsedBytes)
        return (Double(host.memUsedBytes - compressed) / whole, Double(compressed) / whole,
                Double(host.swapUsedBytes) / whole)
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
