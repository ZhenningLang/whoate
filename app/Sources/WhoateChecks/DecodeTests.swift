// QA8: Swift models decode real whoate/v1 output and cover every Schema key.
import Foundation
@testable import WhoateKit

enum DecodeTests {
    static func sampleFixtureDecodes() throws {
        let report = try JSONDecoder().decode(Report.self, from: repoFile("tests/fixtures/attribution/sample-v1.json"))
        expect(report.owners.count == 1)
        expect(report.owners[0].kind == .agentSession)
        expect(report.owners[0].agent?.firstPrompt != nil)
        expect(report.host?.pressure == .normal)
    }

    #if WHOATE_FIXTURE
    static func replayTickDecodes() throws {
        let path = repoRoot.appendingPathComponent("tests/fixtures/capture/snapshot-20261014-1940.json").path
        let engine = try require(Engine.fixture(path: path))
        guard case .value(let report) = engine.tick() else { fail("replay tick failed"); return }
        expect(report.owners.count > 0)
        expect(Engine.fixture(path: "/nonexistent/whoate.json") == nil)
    }
    #endif

    /// The in-process live engine; verify.sh points HOME/cache/rules at temp dirs.
    static func liveTickDecodes() throws {
        let engine = try require(Engine.live())
        guard case .value(let report) = engine.tick() else { fail("live tick failed"); return }
        expect(report.owners.count > 0)
        expect(report.host != nil)
    }

    /// `WHOATE_UI_AUTOMATION=dump-tick` output from the bundled app, when provided.
    static func dumpTickFileDecodes() throws {
        guard let path = ProcessInfo.processInfo.environment["WHOATE_DUMP_TICK_FILE"] else {
            return skip("WHOATE_DUMP_TICK_FILE not set (no bundle in this tier)")
        }
        let report = try JSONDecoder().decode(Report.self, from: Data(contentsOf: URL(fileURLWithPath: path)))
        expect(report.owners.count > 0)
    }

    static func schemaPropertiesAllHaveCodingKeys() throws {
        let schema = try require(JSONSerialization.jsonObject(with: repoFile("schema/whoate-v1.schema.json")) as? [String: Any])
        let defs = try require(schema["$defs"] as? [String: Any])
        func properties(_ name: String) throws -> Set<String> {
            var node = try require(defs[name] as? [String: Any])
            if let any = node["anyOf"] as? [[String: Any]] { node = try require(any.first { $0["type"] as? String == "object" }) }
            return Set(try require(node["properties"] as? [String: Any]).keys)
        }
        let cases: [(String, [String])] = [
            ("owner", Owner.CodingKeys.allCases.map(\.rawValue)),
            ("host", Host.CodingKeys.allCases.map(\.rawValue)),
            ("agent", Agent.CodingKeys.allCases.map(\.rawValue)),
            ("origin", Origin.CodingKeys.allCases.map(\.rawValue)),
            ("swarm", Swarm.CodingKeys.allCases.map(\.rawValue)),
            ("physical_estimate", PhysicalEstimate.CodingKeys.allCases.map(\.rawValue)),
            ("kernel_zones", KernelZones.CodingKeys.allCases.map(\.rawValue)),
            ("gpu_memory", GpuMemory.CodingKeys.allCases.map(\.rawValue)),
            ("part", Part.CodingKeys.allCases.map(\.rawValue)),
        ]
        for (name, keys) in cases {
            let missing = try properties(name).subtracting(keys)
            expect(missing.isEmpty, "\(name) lacks \(missing)")
        }
        let report = try require(defs["report"] as? [String: Any])
        let reportKeys = Set(try require(report["properties"] as? [String: Any]).keys)
        expect(reportKeys.subtracting(Report.CodingKeys.allCases.map(\.rawValue)).isEmpty)
    }

    static func missingOptionalFieldsAndUnknownEnumsDoNotFail() throws {
        var owner = ownerJSON("o", mem: 1, level: "urgent", kind: "future_kind")
        owner.removeValue(forKey: "history")
        owner["origin"] = ["tmux_pane": "%1", "pane_alive": NSNull(), "cwd": NSNull(), "ports": NSNull()]
        owner["agent"] = [
            "tool": "claude", "session_id": NSNull(), "name": NSNull(), "name_source": NSNull(), "title": NSNull(),
            "first_prompt": NSNull(), "first_reply": NSNull(), "status": NSNull(), "cwd": NSNull(),
            "branch": NSNull(), "model": NSNull(), "cost_usd": NSNull(), "confidence": "telepathic",
            "updated_at_ms": NSNull(), "stale": false,
        ]
        var json = reportJSON(owners: [owner], swarms: [swarmJSON("s", mem: 1, count: 1, shape: "spiral")])
        var host = json["host"] as! [String: Any]
        host["pressure"] = "meltdown"
        json["host"] = host
        let report = try decodeValue(Report.self, json)
        expect(report.owners[0].kind == .unknown)
        expect(report.owners[0].verdict.level == .unknown)
        expect(report.owners[0].agent?.confidence == .unknown)
        expect(report.owners[0].agent?.idleSeconds == nil)
        expect(report.owners[0].origin?.windowName == nil)
        expect(report.owners[0].reclaimablePhysical == nil)
        expect(report.owners[0].history.isEmpty)
        expect(report.swarms[0].shape == .unknown)
        expect(report.host?.pressure == .unknown)
        expect(report.host?.kernelZones == nil)
    }

    static func factValuesKeepTheirType() throws {
        let facts = try decodeValue([Fact].self, [
            ["key": "a", "label": "a", "value": true],
            ["key": "b", "label": "b", "value": 3],
            ["key": "c", "label": "c", "value": 2.5],
            ["key": "d", "label": "d", "value": "x"],
            ["key": "e", "label": "e", "value": ["y"]],
        ])
        expect(facts.map(\.value) == [.bool(true), .count(3), .number(2.5), .text("x"), .strings(["y"])])
    }

    static func errorEnvelopeIsAFailure() {
        let data = Data(#"{"schema":"whoate/v1","error":{"code":3,"message":"pid_not_found"}}"#.utf8)
        guard case .failure(let error) = decode(Report.self, from: data) else { fail("expected failure"); return }
        expect(error == FFIError(code: 3, message: "pid_not_found"))
    }

    static func insightsComeFromTheLibrary() {
        let insights = Engine.insights()
        expect(!insights.isEmpty)
        expect(insights.allSatisfy { !$0.cause.zh.isEmpty && !$0.cause.en.isEmpty })
    }
}
