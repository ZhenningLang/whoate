// Thin Swift wrapper over crates/whoate-ffi/include/whoate.h. All engine calls
// go through one serial queue because the Rust engine is not thread-safe.
import CWhoate
import Foundation

public enum Decoded<Value: Sendable>: Sendable {
    case value(Value)
    case failure(FFIError)
}

private struct ErrorEnvelope: Decodable { var error: FFIError? }

public func decode<Value: Decodable & Sendable>(_ type: Value.Type, from data: Data) -> Decoded<Value> {
    let decoder = JSONDecoder()
    if let envelope = try? decoder.decode(ErrorEnvelope.self, from: data), let error = envelope.error {
        return .failure(error)
    }
    do {
        return .value(try decoder.decode(type, from: data))
    } catch {
        return .failure(FFIError(code: 1, message: "decode: \(error)"))
    }
}

/// Takes ownership of a string returned by the library.
func take(_ pointer: UnsafeMutablePointer<CChar>?) -> Data {
    guard let pointer else { return Data() }
    defer { whoate_string_free(pointer) }
    return Data(bytes: pointer, count: strlen(pointer))
}

public final class Engine: RefreshEngine, @unchecked Sendable {
    private let handle: OpaquePointer
    private let queue = DispatchQueue(label: "whoate.engine", qos: .utility)

    private init(_ handle: OpaquePointer) { self.handle = handle }
    deinit { whoate_engine_free(handle) }

    /// NULL from the library (sampler, config or knowledge base failed) is nil.
    public static func live() -> Engine? {
        whoate_engine_new().map(Engine.init)
    }
    #if WHOATE_FIXTURE
    /// Debug Rust library only (WHOATE_FFI_PROFILE=debug): replay a fixture.
    public static func fixture(path: String) -> Engine? {
        whoate_engine_new_from_fixture(path).map(Engine.init)
    }
    #endif

    public func tickData() -> Data { queue.sync { take(whoate_engine_tick(handle)) } }
    public func whyData(pid: Int32) -> Data { queue.sync { take(whoate_why(handle, pid)) } }
    public func tick() -> Decoded<Report> { decode(Report.self, from: tickData()) }
    public func why(pid: Int32) -> Decoded<Report> { decode(Report.self, from: whyData(pid: pid)) }
    /// whoate_why reuses the latest tick (no new sample) and keeps every point.
    public func history(pid: Int32, owner: String) -> [HistoryPoint]? {
        guard case .value(let report) = why(pid: pid) else { return nil }
        return report.owners.first { $0.id == owner }?.history
    }
    public func setActive(_ active: Bool) -> Decoded<RefreshControl> {
        queue.sync { decode(RefreshControl.self, from: take(whoate_engine_set_active(handle, active ? 1 : 0))) }
    }
    public static func host() -> Decoded<HostSample> {
        decode(HostSample.self, from: take(whoate_host_json()))
    }

    /// Blocks up to five seconds when force is false. Call off the main thread.
    public static func kill(pids: [Int32], force: Bool) -> (code: Int32, result: Decoded<KillResult>) {
        var err: UnsafeMutablePointer<CChar>?
        let code = pids.withUnsafeBufferPointer {
            whoate_kill($0.baseAddress, Int32($0.count), force ? 1 : 0, &err)
        }
        return (code, decode(KillResult.self, from: take(err)))
    }

    public static func insights() -> [Insight] {
        struct Envelope: Decodable { var insights: [Insight] }
        if case .value(let envelope) = decode(Envelope.self, from: take(whoate_insights_json())) {
            return envelope.insights
        }
        return []
    }
}
