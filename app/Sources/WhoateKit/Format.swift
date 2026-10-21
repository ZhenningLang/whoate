// Compact numbers as in the mockups: 41.2G, 400M. Binary units.
import Foundation

public enum Format {
    static let kib = 1024.0, mib = kib * 1024, gib = mib * 1024

    public static func bytes(_ value: UInt64) -> String {
        let v = Double(value)
        if v >= gib { return String(format: "%.1fG", v / gib) }
        if v >= mib { return String(format: "%.0fM", v / mib) }
        if v >= kib { return String(format: "%.0fK", v / kib) }
        return "\(value)B"
    }

    /// A memory number and its hover text. With unreadable processes the sum is a
    /// lower bound: "≥ 596M", and the hover says how many (R5, owner 2026-10-21).
    public struct Memory: Equatable, Sendable {
        public var text: String
        public var help: String?
    }
    public static func memory(_ value: UInt64, unknown: Int, _ language: Language) -> Memory {
        unknown > 0 ? Memory(text: "≥ " + bytes(value), help: L10n.format(.unknownMem, language, "\(unknown)"))
            : Memory(text: bytes(value), help: nil)
    }

    nonisolated(unsafe) private static let minute: DateFormatter = {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.dateFormat = "yyyy-MM-dd HH:mm"
        return f
    }()
    /// "2026-10-21 14:52" in the local time zone.
    public static func localTime(_ date: Date) -> String { minute.string(from: date) }

    /// Signs are kept as given: a negative change means usage went up.
    public static func signedBytes(_ value: Int64) -> String {
        let magnitude = bytes(value.magnitude)
        return value < 0 ? "−" + magnitude : "+" + magnitude
    }

    /// nil CPU is unknown, never zero.
    public static func cpu(_ value: Double?) -> String {
        guard let value else { return "—" }
        return value >= 10 ? String(format: "%.0f%%", value) : String(format: "%.1f%%", value)
    }

    public static func duration(_ seconds: UInt64, _ language: Language) -> String {
        let (d, h, m) = (seconds / 86400, seconds % 86400 / 3600, seconds % 3600 / 60)
        switch language {
        case .zh:
            if d > 0 { return "\(d) 天 \(h) 小时" }
            if h > 0 { return "\(h) 小时 \(m) 分" }
            if m > 0 { return "\(m) 分钟" }
            return "\(seconds) 秒"
        case .en:
            if d > 0 { return "\(d)d \(h)h" }
            if h > 0 { return "\(h)h \(m)m" }
            if m > 0 { return "\(m)m" }
            return "\(seconds)s"
        }
    }

    public enum Trend: Equatable, Sendable {
        /// Points scaled into the box, oldest at x=0 and newest at the right edge.
        case line([Point])
        /// The latest continuous run is shorter than `minimumSpan`: show text, not a curve.
        case insufficient
    }
    public struct Point: Equatable, Sendable {
        public var x: Double, y: Double
        public init(x: Double, y: Double) { self.x = x; self.y = y }
    }
    public static let minimumSpan: Int64 = 30
    /// Raw history has one point per visible refresh (2 s) and one per hidden
    /// full sample (AppModel.hiddenFullInterval, 30 s). 1.5 × 30 s keeps hidden
    /// points joined; one missed hidden sample (60 s), sleep or a restart breaks the run.
    public static let gapLimit: Int64 = 45
    /// At most the last 30 minutes; history keeps 1800 points, about 15 hours at 30 s.
    public static let window: Int64 = 1800

    /// Only the latest continuous run of raw history (AppModel.histories, never a
    /// tick's downsampled copy) is drawn; older or isolated points are never joined.
    /// `metric` .cpu draws CPU percent; points without a CPU value are left out.
    public static func trend(_ points: [HistoryPoint], width: Double, height: Double,
                             metric: SortKey = .memory) -> Trend {
        let value: (HistoryPoint) -> Double? = metric == .cpu ? { $0.cpuPct } : { Double($0.memBytes) }
        // The latest run of samples, then the points with a value, split again:
        // a missing value is a gap too, and a stale stretch never stands in for
        // a missing tail (its last value must be within gapLimit of the newest sample).
        let samples = latestRun(points)
        var run = latestRun(samples.filter { value($0) != nil })
        if let newest = samples.last?.timestampS, let last = run.last?.timestampS, newest - last > gapLimit { run = [] }
        if let end = run.last?.timestampS { run.removeAll { $0.timestampS < end - window } }
        guard let first = run.first, let last = run.last, last.timestampS - first.timestampS >= minimumSpan else {
            return .insufficient
        }
        let values = run.map { value($0)! }
        var lo = values.min()!, hi = values.max()!
        // Changes under 5% of the value (5 points of CPU) stay visually flat instead of filling the box.
        let floor = metric == .cpu ? 5 : hi * 0.05
        if hi - lo < floor { (lo, hi) = ((lo + hi - floor) / 2, (lo + hi + floor) / 2) }
        let span = Double(last.timestampS - first.timestampS)
        return .line(zip(run, values).map { p, v in
            Point(x: Double(p.timestampS - first.timestampS) / span * width,
                  y: hi > lo ? height - (v - lo) / (hi - lo) * height : height / 2)
        })
    }

    /// Detail chart: every continuous run in the last `window` seconds, split
    /// at gaps longer than `gapLimit` so the chart leaves a hole, never a bridge.
    public static func runs(_ points: [HistoryPoint]) -> [[HistoryPoint]] {
        guard let end = points.last?.timestampS else { return [] }
        var runs: [[HistoryPoint]] = []
        for p in points where p.timestampS >= end - window {
            if let last = runs.last?.last, p.timestampS - last.timestampS <= gapLimit {
                runs[runs.count - 1].append(p)
            } else {
                runs.append([p])
            }
        }
        return runs
    }

    static func latestRun(_ points: [HistoryPoint]) -> [HistoryPoint] {
        guard var start = points.indices.last else { return [] }
        while start > 0, points[start].timestampS - points[start - 1].timestampS <= gapLimit { start -= 1 }
        return Array(points[start...])
    }
}
