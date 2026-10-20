// whoate/v1 as defined by schema/whoate-v1.schema.json. The GUI only consumes
// this shape; it never defines its own. Phase 07 fields are optional and
// unknown enum values decode to `.unknown` instead of failing.
import Foundation

public protocol LenientEnum: RawRepresentable, Decodable, Sendable, Equatable where RawValue == String {
    static var unknown: Self { get }
}
extension LenientEnum {
    public init(from decoder: Decoder) throws {
        self = Self(rawValue: try decoder.singleValueContainer().decode(String.self)) ?? .unknown
    }
}

public enum OwnerKind: String, LenientEnum {
    case tmuxPane = "tmux_pane", agentSession = "agent_session", app, orphan, service, system, unknown
}
public enum Confidence: String, LenientEnum { case certain, inferred, weak, unknown }
public enum Level: String, LenientEnum { case ok, watch, act, unknown }
public enum Pressure: String, LenientEnum { case normal, warn, critical, unknown }
public enum Freshness: String, LenientEnum { case fresh, stale, missing, unknown }
public enum Shape: String, LenientEnum { case burst, spread, unknown }
public enum KillStatus: String, LenientEnum {
    case terminated, stillAlive = "still_alive", refused, missing, unknown
}

public struct Report: Decodable, Sendable, Equatable {
    public var schema: String?
    public var takenAt: String
    public var host: Host?
    public var systemProcs: SystemProcs
    public var owners: [Owner]
    public var swarms: [Swarm]
    public var issues: [Issue]
    enum CodingKeys: String, CodingKey, CaseIterable {
        case schema, takenAt = "taken_at", host, systemProcs = "system_procs", owners, swarms, issues
    }
}

public struct SystemProcs: Decodable, Sendable, Equatable {
    public var status: Freshness
    public var asOf: String?
    enum CodingKeys: String, CodingKey, CaseIterable { case status, asOf = "as_of" }
}

public struct Issue: Decodable, Sendable, Equatable {
    public var stage: String
    public var pid: Int32?
    public var code: String
}

public struct Host: Decodable, Sendable, Equatable {
    public var memTotalBytes: UInt64
    public var memUsedBytes: UInt64
    public var compressedBytes: UInt64
    public var wiredBytes: UInt64
    public var swapUsedBytes: UInt64
    public var pressure: Pressure
    public var kernelZones: KernelZones?
    public var gpuMemory: GpuMemory?
    public var memoryDiagnostics: [Verdict]?
    public var cpu: HostCPU?
    enum CodingKeys: String, CodingKey, CaseIterable {
        case memTotalBytes = "mem_total_bytes", memUsedBytes = "mem_used_bytes"
        case compressedBytes = "compressed_bytes", wiredBytes = "wired_bytes"
        case swapUsedBytes = "swap_used_bytes", pressure
        case kernelZones = "kernel_zones", gpuMemory = "gpu_memory"
        case memoryDiagnostics = "memory_diagnostics", cpu
    }
}

/// Whole-machine CPU since the previous sample. usage is 0–100 of all cores.
public struct HostCPU: Decodable, Sendable, Equatable {
    public var status: Freshness
    public var usagePct: Double?
    public var load1m: Double?
    public var logicalCores: Int?
    public var reason: String?
    enum CodingKeys: String, CodingKey, CaseIterable {
        case status, usagePct = "usage_pct", load1m = "load_1m", logicalCores = "logical_cores", reason
    }
}

public struct KernelZones: Decodable, Sendable, Equatable {
    public struct Zone: Decodable, Sendable, Equatable {
        public var name: String
        public var bytes: UInt64
        public var collectible: Bool?
        public var pageable: Bool?
    }
    public var status: Freshness
    public var asOf: String?
    public var basis: String
    public var top: [Zone]
    public var reason: String?
    enum CodingKeys: String, CodingKey, CaseIterable { case status, asOf = "as_of", basis, top, reason }
}

public struct GpuMemory: Decodable, Sendable, Equatable {
    public var status: Freshness
    public var asOf: String?
    public var bytes: UInt64?
    public var source: String?
    public var reason: String?
    enum CodingKeys: String, CodingKey, CaseIterable { case status, asOf = "as_of", bytes, source, reason }
}

public struct Owner: Decodable, Sendable, Equatable, Identifiable {
    public var id: String
    public var kind: OwnerKind
    public var label: String
    public var memBytes: UInt64
    public var reclaimablePhysical: PhysicalEstimate?
    public var unknownMemCount: Int
    public var cpuPct: Double?
    public var pids: [Int32]
    public var origin: Origin?
    public var agent: Agent?
    public var evidence: [Evidence]
    public var parts: [Part]
    public var facts: [Fact]
    public var insights: [String]
    public var history: [HistoryPoint]
    public var verdict: Verdict
    enum CodingKeys: String, CodingKey, CaseIterable {
        case id, kind, label, memBytes = "mem_bytes", reclaimablePhysical = "reclaimable_physical"
        case unknownMemCount = "unknown_mem_count", cpuPct = "cpu_pct", pids, origin, agent
        case evidence, parts, facts, insights, history, verdict
    }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        kind = try c.decode(OwnerKind.self, forKey: .kind)
        label = try c.decode(String.self, forKey: .label)
        memBytes = try c.decode(UInt64.self, forKey: .memBytes)
        reclaimablePhysical = try c.decodeIfPresent(PhysicalEstimate.self, forKey: .reclaimablePhysical)
        unknownMemCount = try c.decode(Int.self, forKey: .unknownMemCount)
        cpuPct = try c.decodeIfPresent(Double.self, forKey: .cpuPct)
        pids = try c.decode([Int32].self, forKey: .pids)
        origin = try c.decodeIfPresent(Origin.self, forKey: .origin)
        agent = try c.decodeIfPresent(Agent.self, forKey: .agent)
        evidence = try c.decode([Evidence].self, forKey: .evidence)
        parts = try c.decode([Part].self, forKey: .parts)
        facts = try c.decode([Fact].self, forKey: .facts)
        insights = try c.decode([String].self, forKey: .insights)
        // Optional in the schema: older reports and one-shot samples omit it.
        history = try c.decodeIfPresent([HistoryPoint].self, forKey: .history) ?? []
        verdict = try c.decode(Verdict.self, forKey: .verdict)
    }
}

extension Owner {
    /// The app's bundle id (`app:<id>`); nil for other kinds.
    public var bundleID: String? { kind == .app && id.hasPrefix("app:") ? String(id.dropFirst("app:".count)) : nil }
}

extension Part {
    /// "node ×3", or the name alone for one process.
    public var displayName: String { processCount > 1 ? "\(name) ×\(processCount)" : name }
}

public struct PhysicalEstimate: Decodable, Sendable, Equatable {
    public var bytes: UInt64?
    public var unknownCount: Int
    public var basis: String
    public var reason: String?
    enum CodingKeys: String, CodingKey, CaseIterable { case bytes, unknownCount = "unknown_count", basis, reason }
}

public struct Origin: Decodable, Sendable, Equatable {
    public var tmuxPane: String?
    public var paneAlive: Bool?
    public var cwd: String?
    public var paneTitle: String?
    public var windowName: String?
    public var ports: [UInt16]?
    enum CodingKeys: String, CodingKey, CaseIterable {
        case tmuxPane = "tmux_pane", paneAlive = "pane_alive", cwd
        case paneTitle = "pane_title", windowName = "window_name", ports
    }
}

public struct Agent: Decodable, Sendable, Equatable {
    public var tool: String
    public var sessionId: String?
    public var name: String?
    public var nameSource: String?
    public var title: String?
    public var firstPrompt: String?
    public var firstReply: String?
    public var status: String?
    public var cwd: String?
    public var branch: String?
    public var model: String?
    public var costUsd: Double?
    public var confidence: Confidence
    public var updatedAtMs: Int64?
    public var lastActivityAt: String?
    public var idleSeconds: UInt64?
    public var stale: Bool
    enum CodingKeys: String, CodingKey, CaseIterable {
        case tool, sessionId = "session_id", name, nameSource = "name_source", title
        case firstPrompt = "first_prompt", firstReply = "first_reply", status, cwd, branch, model
        case costUsd = "cost_usd", confidence, updatedAtMs = "updated_at_ms"
        case lastActivityAt = "last_activity_at", idleSeconds = "idle_seconds", stale
    }
}

public struct Evidence: Decodable, Sendable, Equatable {
    public var kind: String
    public var detail: String
    public var confidence: Confidence
}

public struct Part: Decodable, Sendable, Equatable {
    public var name: String
    public var memBytes: UInt64
    public var processCount: Int
    public var pids: [Int32]
    public var unknownMemCount: Int
    enum CodingKeys: String, CodingKey, CaseIterable {
        case name, memBytes = "mem_bytes", processCount = "process_count", pids
        case unknownMemCount = "unknown_mem_count"
    }
}

public enum FactValue: Decodable, Sendable, Equatable {
    case bool(Bool), count(UInt64), number(Double), text(String), strings([String])
    public init(from decoder: Decoder) throws {
        let c = try decoder.singleValueContainer()
        if let v = try? c.decode(Bool.self) { self = .bool(v) }
        else if let v = try? c.decode(UInt64.self) { self = .count(v) }
        else if let v = try? c.decode(Double.self) { self = .number(v) }
        else if let v = try? c.decode(String.self) { self = .text(v) }
        else { self = .strings(try c.decode([String].self)) }
    }
}
public struct Fact: Decodable, Sendable, Equatable {
    public var key: String
    public var label: String
    public var value: FactValue
}

public struct HistoryPoint: Decodable, Sendable, Equatable {
    public var timestampS: Int64
    public var memBytes: UInt64
    public var cpuPct: Double?
    /// Processes whose memory could not be read; memBytes counts the rest.
    public var unknownMemCount: Int?
    enum CodingKeys: String, CodingKey, CaseIterable {
        case timestampS = "timestamp_s", memBytes = "mem_bytes", cpuPct = "cpu_pct"
        case unknownMemCount = "unknown_mem_count"
    }
}

public struct Verdict: Decodable, Sendable, Equatable {
    public var level: Level
    public var rule: String
    public var text: String
    public var action: String?
}

public struct Swarm: Decodable, Sendable, Equatable, Identifiable {
    public struct Member: Decodable, Sendable, Equatable {
        public var id: String
        public var count: Int
    }
    public var signature: String
    public var label: String
    public var count: Int
    public var memBytes: UInt64
    public var unknownMemCount: Int
    public var cpuPct: Double?
    public var shape: Shape
    public var owners: [Member]
    public var parents: [Int32]
    public var startedSpanS: Double?
    public var pids: [Int32]
    public var evidence: [Evidence]
    public var verdict: Verdict
    public var id: String { "swarm:\(signature)" }
    enum CodingKeys: String, CodingKey, CaseIterable {
        case signature, label, count, memBytes = "mem_bytes", unknownMemCount = "unknown_mem_count"
        case cpuPct = "cpu_pct", shape, owners, parents, startedSpanS = "started_span_s", pids
        case evidence, verdict
    }
}

/// `whoate_kill` result, same shape as `whoate kill --json`.
public struct KillResult: Decodable, Sendable, Equatable {
    public struct Item: Decodable, Sendable, Equatable {
        public var pid: Int32
        public var status: KillStatus
        public var reason: String?
    }
    public struct Measurement: Decodable, Sendable, Equatable {
        public var measuredAt: String?
        public var hostUsedBytes: UInt64?
        public var targetResidentBytes: UInt64?
        public var unknownCount: Int
        public var reason: String?
        enum CodingKeys: String, CodingKey {
            case measuredAt = "measured_at", hostUsedBytes = "host_used_bytes"
            case targetResidentBytes = "target_resident_bytes", unknownCount = "unknown_count", reason
        }
    }
    public struct Memory: Decodable, Sendable, Equatable {
        public var before: Measurement
        public var after: Measurement?
        public var physicalMemoryChangeBytes: Int64?
        public var targetResidentChangeBytes: Int64?
        public var basis: String
        enum CodingKeys: String, CodingKey {
            case before, after, basis
            case physicalMemoryChangeBytes = "physical_memory_change_bytes"
            case targetResidentChangeBytes = "target_resident_change_bytes"
        }
    }
    public var executed: Bool
    public var results: [Item]
    public var missingTargets: [String]
    public var memory: Memory
    enum CodingKeys: String, CodingKey {
        case executed, results, missingTargets = "missing_targets", memory
    }
}

/// Knowledge-base entry from `whoate_insights_json`. Only display fields.
public struct Insight: Decodable, Sendable, Equatable, Identifiable {
    public struct Localized: Decodable, Sendable, Equatable {
        public var zh: String
        public var en: String
        public func text(_ language: Language) -> String { language == .zh ? zh : en }
    }
    /// System service policy (system.toml `[entries.system]`); display fields only.
    public struct System: Decodable, Sendable, Equatable {
        public var member: String
        /// fseventsd | spotlight | none (core `Restart`).
        public var restart: String
        public var restartSafety: Localized
        public var launchd: Localized
        public var permissions: Localized
        public var sideEffects: Localized
        enum CodingKeys: String, CodingKey {
            case member, restart, launchd, permissions
            case restartSafety = "restart_safety", sideEffects = "side_effects"
        }
        /// Same strings as core `Restart::command`; whoate never runs them.
        public var command: String? {
            switch restart {
            case "fseventsd": "sudo launchctl kickstart -k system/com.apple.fseventsd"
            case "spotlight": "sudo launchctl kickstart -k system/com.apple.metadata.mds"
            default: nil
            }
        }
    }
    public var id: String
    public var recognizer: String
    public var cause: Localized
    public var advice: Localized
    public var source: String
    public var checked: String
    public var system: System?
}

/// `{"schema":"whoate/v1","error":{"code","message"}}` from any FFI call.
public struct FFIError: Decodable, Sendable, Equatable, Error {
    public var code: Int
    public var message: String
}
