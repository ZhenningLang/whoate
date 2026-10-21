import Foundation

/// Only the full path owns a long-lived engine. A cold hidden app uses host().
public protocol RefreshEngine: Sendable {
    func tick() -> Decoded<Report>
    func setActive(_ active: Bool) -> Decoded<RefreshControl>
    /// The owner's raw history from the latest tick (no downsampling), or nil.
    func history(pid: Int32, owner: String) -> [HistoryPoint]?
    /// History plus whoate_why's process list (R4, R8), from one call.
    func detail(pid: Int32, owner: String) -> (history: [HistoryPoint], procs: [ProcBrief])?
}
extension RefreshEngine {
    public func history(pid: Int32, owner: String) -> [HistoryPoint]? { nil }
    public func detail(pid: Int32, owner: String) -> (history: [HistoryPoint], procs: [ProcBrief])? {
        history(pid: pid, owner: owner).map { ($0, []) }
    }
}

public struct RefreshControl: Decodable, Sendable {
    public var active: Bool
}

public struct HostSample: Decodable, Sendable {
    public var takenAt: String
    public var host: Host
    enum CodingKeys: String, CodingKey { case schema, takenAt = "taken_at", host }
    public init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        guard try c.decode(String.self, forKey: .schema) == "whoate/host-v1" else {
            throw DecodingError.dataCorruptedError(forKey: .schema, in: c, debugDescription: "host schema mismatch")
        }
        takenAt = try c.decode(String.self, forKey: .takenAt)
        host = try c.decode(Host.self, forKey: .host)
    }
}
