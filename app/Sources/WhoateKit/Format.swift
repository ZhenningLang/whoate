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

    /// Points scaled into a width×height box, newest at the right edge.
    /// Gaps (missing seconds) are kept as separate segments, never bridged.
    public static func sparkline(_ points: [HistoryPoint], width: Double, height: Double) -> [[(x: Double, y: Double)]] {
        guard let first = points.first, let last = points.last else { return [] }
        var lo = Double(points.map(\.memBytes).min()!), hi = Double(points.map(\.memBytes).max()!)
        // Changes under 5% of the value stay visually flat instead of filling the box.
        let floor = hi * 0.05
        if hi - lo < floor { (lo, hi) = ((lo + hi - floor) / 2, (lo + hi + floor) / 2) }
        let span = Double(max(last.timestampS - first.timestampS, 1))
        let gapLimit = max(span / Double(max(points.count - 1, 1)) * 3, 2)
        var segments: [[(x: Double, y: Double)]] = [[]]
        var previous: Int64?
        for p in points {
            if let previous, Double(p.timestampS - previous) > gapLimit { segments.append([]) }
            previous = p.timestampS
            let x = points.count == 1 ? width : Double(p.timestampS - first.timestampS) / span * width
            let y = hi > lo ? height - (Double(p.memBytes) - lo) / (hi - lo) * height : height / 2
            segments[segments.count - 1].append((x, y))
        }
        return segments.filter { !$0.isEmpty }
    }
}
