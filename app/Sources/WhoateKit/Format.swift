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
    public static func trend(_ points: [HistoryPoint], width: Double, height: Double) -> Trend {
        var run = latestRun(points)
        if let end = run.last?.timestampS { run.removeAll { $0.timestampS < end - window } }
        guard let first = run.first, let last = run.last, last.timestampS - first.timestampS >= minimumSpan else {
            return .insufficient
        }
        var lo = Double(run.map(\.memBytes).min()!), hi = Double(run.map(\.memBytes).max()!)
        // Changes under 5% of the value stay visually flat instead of filling the box.
        let floor = hi * 0.05
        if hi - lo < floor { (lo, hi) = ((lo + hi - floor) / 2, (lo + hi + floor) / 2) }
        let span = Double(last.timestampS - first.timestampS)
        return .line(run.map { p in
            Point(x: Double(p.timestampS - first.timestampS) / span * width,
                  y: hi > lo ? height - (Double(p.memBytes) - lo) / (hi - lo) * height : height / 2)
        })
    }

    static func latestRun(_ points: [HistoryPoint]) -> [HistoryPoint] {
        guard var start = points.indices.last else { return [] }
        while start > 0, points[start].timestampS - points[start - 1].timestampS <= gapLimit { start -= 1 }
        return Array(points[start...])
    }
}
