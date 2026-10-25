// Close confirmation and result models (U-BEH-1). The estimate only ever comes
// from reclaimable_physical.bytes; mem_bytes (footprint) is never shown as it.
import Foundation

public struct KillConfirmation: Equatable, Sendable {
    /// One line of the confirmation's list (R16): program name, memory, pid.
    public struct Process: Equatable, Sendable {
        public var pid: Int32
        public var name: String
        public var memBytes: UInt64?
        public var memory: String
    }
    public var ownerID: String
    public var title: String
    public var pids: [Int32]
    /// Every pid, biggest first; names and memory from the owner's whoate_why when it
    /// has arrived, else from the owner's parts (memory only for a one-process part).
    public var processes: [Process]
    public var estimateBytes: UInt64?
    public var estimate: String
    /// "将结束 3 个进程，预计腾出 1.2G" (R2): the count and the estimate in one line.
    public var summary: String
    public var isProtected: Bool

    public init(owner: Owner, language: Language, procs: [ProcBrief] = []) {
        ownerID = owner.id
        title = L10n.format(.confirmTitle, language, owner.label)
        pids = owner.pids
        processes = owner.pids.map { pid in
            let name: String, bytes: UInt64?
            if let p = procs.first(where: { $0.pid == pid }) {
                (name, bytes) = (p.name ?? p.comm, p.memBytes)
            } else if let part = owner.parts.first(where: { $0.pids.contains(pid) }) {
                (name, bytes) = (part.name, part.pids.count == 1 ? part.memBytes : nil)
            } else {
                (name, bytes) = (owner.label, owner.pids.count == 1 ? owner.memBytes : nil)
            }
            return Process(pid: pid, name: name, memBytes: bytes,
                           memory: bytes.map(Format.bytes) ?? L10n.text(.notMeasured, language))
        }.sorted { ($0.memBytes ?? 0, $1.pid) > ($1.memBytes ?? 0, $0.pid) }
        estimateBytes = owner.reclaimablePhysical?.bytes
        if let bytes = estimateBytes {
            estimate = Format.bytes(bytes)
        } else {
            let unknown = owner.reclaimablePhysical?.unknownCount ?? owner.pids.count
            estimate = L10n.format(.estimateUnknown, language, "\(unknown)")
        }
        summary = estimateBytes.map { L10n.format(.confirmSummary, language, "\(owner.pids.count)", Format.bytes($0)) }
            ?? L10n.format(.confirmSummaryUnknown, language, "\(owner.pids.count)")
        isProtected = Panel.isProtected(owner)
    }
}

public struct KillOutcome: Equatable, Sendable {
    public struct Line: Equatable, Sendable {
        public var pid: Int32
        public var status: KillStatus
        public var text: String
    }
    public var exitCode: Int32
    public var lines: [Line]
    public var stillAlive: [Int32]
    /// Observed host change in bytes (before − after); may be negative.
    public var changeBytes: Int64?
    public var change: String

    public init(code: Int32, result: KillResult, language: Language) {
        exitCode = code
        lines = result.results.map { Line(pid: $0.pid, status: $0.status, text: KillOutcome.label($0, language)) }
        stillAlive = result.results.filter { $0.status == .stillAlive }.map(\.pid)
        changeBytes = result.memory.physicalMemoryChangeBytes
        change = KillOutcome.describe(changeBytes, language)
    }

    /// Applies the force step: its lines replace the same pids, and the two
    /// observed changes add up (each is that step's own before − after).
    public func merging(code: Int32, force: KillResult, language: Language) -> KillOutcome {
        var merged = self
        let second = KillOutcome(code: code, result: force, language: language)
        for line in second.lines {
            if let i = merged.lines.firstIndex(where: { $0.pid == line.pid }) { merged.lines[i] = line }
        }
        merged.stillAlive = merged.lines.filter { $0.status == .stillAlive }.map(\.pid)
        merged.exitCode = max(code, exitCode)
        merged.changeBytes = changeBytes.flatMap { a in second.changeBytes.map { a + $0 } }
        merged.change = KillOutcome.describe(merged.changeBytes, language)
        return merged
    }

    static func describe(_ bytes: Int64?, _ language: Language) -> String {
        bytes.map(Format.signedBytes) ?? L10n.text(.notMeasured, language)
    }

    static func label(_ item: KillResult.Item, _ language: Language) -> String {
        let status: L10nKey = switch item.status {
        case .terminated: .terminated
        case .stillAlive: .stillAlive
        case .refused: .refused
        case .missing: .missing
        case .unknown: .unknownStatus
        }
        let text = L10n.text(status, language)
        // Known core/FFI reason codes are translated; an unknown one stays as sent.
        let reason = item.reason.map { code in reasons[code].map { L10n.text($0, language) } ?? code }
        return reason.map { "\(text) (\($0))" } ?? text
    }

    static let reasons: [String: L10nKey] = [
        "protected_pid": .reasonProtectedPid, "not_in_snapshot": .reasonNotInSnapshot,
        "protected_or_unknown_identity": .reasonProtectedOrUnknown, "process_missing": .reasonProcessMissing,
        "identity_changed": .reasonIdentityChanged, "apple_platform_binary": .reasonPlatformBinary,
        "platform_probe_failed": .reasonPlatformProbeFailed, "no_confirmed_plan": .reasonNoConfirmedPlan,
    ]
}
