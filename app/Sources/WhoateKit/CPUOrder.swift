// CPU-mode row order. A single tick's CPU swaps rows whose values are close on
// every refresh, so the order uses a short average and a row only overtakes
// the one above it by a clear margin. Memory order does not use this.
import Foundation

public struct CPUOrder: Equatable, Sendable {
    /// Five samples at the visible 2 s cadence; a real spike reaches the top within 10 s.
    public static let window: Double = 10
    /// A row overtakes the one above when ahead by max(1 point, 15 % of that row).
    public static let minimumLead: Double = 1
    public static let relativeLead: Double = 0.15

    struct Sample: Equatable, Sendable {
        var at: Double
        var pct: Double?
    }
    private var samples: [String: [Sample]] = [:]
    /// Row ids in display order. While frozen it keeps its order (gone ids
    /// dropped, new ids appended) and is the base the next reorder starts from.
    public private(set) var order: [String] = []
    public private(set) var frozen = false

    public init() {}

    /// One tick's values (owners and swarms) at `time` seconds. Ids absent from
    /// the tick are dropped; samples at or before `time - window` are dropped.
    public mutating func record(_ values: [(id: String, pct: Double?)], at time: Double) {
        let present = Set(values.map(\.id))
        samples = samples.filter { present.contains($0.key) }
        for (id, pct) in values { samples[id, default: []].append(Sample(at: time, pct: pct)) }
        for id in samples.keys { samples[id]?.removeAll { $0.at <= time - Self.window } }
        if frozen {
            let kept = order.filter(present.contains)
            order = kept + present.subtracting(kept).sorted(by: before)
        } else {
            order = reordered()
        }
    }

    /// Frozen while the pointer is over the panel list; unfreezing reorders at once.
    public mutating func freeze(_ on: Bool) {
        guard on != frozen else { return }
        frozen = on
        if !on { order = reordered() }
    }

    /// Mean of the non-nil values in the window; nil when there are none.
    public func average(_ id: String) -> Double? {
        let values = (samples[id] ?? []).compactMap(\.pct)
        return values.isEmpty ? nil : values.reduce(0, +) / Double(values.count)
    }

    /// `rows` (in their plain CPU order) rearranged to `order`; unknown ids keep their place after it.
    public func arrange(_ rows: [PanelRow]) -> [PanelRow] {
        let rank = Dictionary(order.enumerated().map { ($1, $0) }, uniquingKeysWith: { a, _ in a })
        return rows.enumerated()
            .sorted { (rank[$0.element.id] ?? Int.max, $0.offset) < (rank[$1.element.id] ?? Int.max, $1.offset) }
            .map(\.element)
    }

    /// Insertion over the previous order: a row moves up only past rows it
    /// overtakes. New ids are inserted by plain average order. Deterministic.
    func reordered() -> [String] {
        let present = Set(samples.keys)
        let kept = order.filter(present.contains)
        var result: [String] = []
        for id in kept {
            var i = result.count
            while i > 0, overtakes(id, result[i - 1]) { i -= 1 }
            result.insert(id, at: i)
        }
        for id in present.subtracting(kept).sorted(by: before) {
            var i = result.count
            while i > 0, before(id, result[i - 1]) { i -= 1 }
            result.insert(id, at: i)
        }
        return result
    }

    func overtakes(_ a: String, _ b: String) -> Bool {
        switch (average(a), average(b)) {
        case let (x?, y?): return x > y + max(Self.minimumLead, Self.relativeLead * y)
        case (.some, nil): return true
        default: return false
        }
    }

    /// The plain rule (Panel.rows' CPU order on averages): higher first, unknown last, then id.
    func before(_ a: String, _ b: String) -> Bool {
        switch (average(a), average(b)) {
        case let (x?, y?) where x != y: return x > y
        case (.some, nil): return true
        case (nil, .some): return false
        default: return a < b
        }
    }
}
