import Foundation
@testable import WhoateKit

let repoRoot = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
    .deletingLastPathComponent()

func repoFile(_ path: String) throws -> Data {
    try Data(contentsOf: repoRoot.appendingPathComponent(path))
}

func decodeValue<T: Decodable & Sendable>(_ type: T.Type, _ object: Any) throws -> T {
    try JSONDecoder().decode(type, from: JSONSerialization.data(withJSONObject: object))
}

/// A schema-shaped owner; `overrides` replace top-level keys.
func ownerJSON(_ id: String, mem: UInt64, cpu: Double? = 1, level: String = "ok", rule: String = "default",
               kind: String = "app", pids: [Int32] = [100], overrides: [String: Any] = [:]) -> [String: Any] {
    var owner: [String: Any] = [
        "id": id, "kind": kind, "label": id, "mem_bytes": mem, "unknown_mem_count": 0,
        "cpu_pct": cpu as Any? ?? NSNull(), "pids": pids, "origin": NSNull(), "agent": NSNull(),
        "evidence": [], "parts": [], "facts": [], "insights": [], "history": [],
        "verdict": ["level": level, "rule": rule, "text": "t"],
    ]
    for (key, value) in overrides { owner[key] = value }
    return owner
}

func swarmJSON(_ signature: String, mem: UInt64, count: Int, cpu: Double? = nil, shape: String = "spread") -> [String: Any] {
    [
        "signature": signature, "label": signature, "count": count, "mem_bytes": mem, "unknown_mem_count": 0,
        "cpu_pct": cpu as Any? ?? NSNull(), "shape": shape, "owners": [], "parents": [], "started_span_s": NSNull(),
        "pids": [], "evidence": [], "verdict": ["level": "watch", "rule": "swarm-\(shape)", "text": "t"],
    ]
}

func reportJSON(owners: [[String: Any]], swarms: [[String: Any]] = []) -> [String: Any] {
    [
        "schema": "whoate/v1", "taken_at": "2026-10-14T19:40:00+08:00",
        "host": [
            "mem_total_bytes": 48 << 30, "mem_used_bytes": 41 << 30, "compressed_bytes": 8 << 30,
            "wired_bytes": 4 << 30, "swap_used_bytes": 20 << 30, "pressure": "warn",
        ],
        "system_procs": ["status": "fresh", "as_of": "2026-10-14T19:39:54+08:00"],
        "owners": owners, "swarms": swarms, "issues": [],
    ]
}

func killJSON(_ results: [(Int32, String, String?)], change: Int64?) -> [String: Any] {
    let measurement: [String: Any] = [
        "measured_at": NSNull(), "host_used_bytes": NSNull(), "target_resident_bytes": NSNull(),
        "unknown_count": 0, "reason": NSNull(),
    ]
    return [
        "schema": "whoate/v1", "executed": true, "missing_targets": [],
        "results": results.map { ["pid": $0.0, "status": $0.1, "reason": $0.2 as Any? ?? NSNull()] },
        "memory": [
            "before": measurement, "after": measurement, "basis": "b",
            "physical_memory_change_bytes": change as Any? ?? NSNull(), "target_resident_change_bytes": NSNull(),
        ],
    ]
}
