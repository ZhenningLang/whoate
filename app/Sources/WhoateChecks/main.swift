// Assertion runner used instead of `swift test`: under Command Line Tools the
// swift-testing macro plugin fails to load in about half of the builds
// (phases/05_gui HANDOFF). Exit status is non-zero on any failure or when no
// check ran.
import Foundation

nonisolated(unsafe) var failures: [String] = []
nonisolated(unsafe) var skipReason: String?

/// Marks the running check as skipped: it is reported, never counted as passed.
func skip(_ reason: String) { skipReason = reason }

func fail(_ message: String, file: StaticString = #fileID, line: UInt = #line) {
    failures.append("\(file):\(line) \(message)")
}
func expect(_ condition: @autoclosure () throws -> Bool, _ message: @autoclosure () -> String = "",
            file: StaticString = #fileID, line: UInt = #line) rethrows {
    if try !condition() { fail("expectation failed \(message())", file: file, line: line) }
}
struct RequireFailed: Error {}
func require<T>(_ value: T?, file: StaticString = #fileID, line: UInt = #line) throws -> T {
    guard let value else {
        fail("required value was nil", file: file, line: line)
        throw RequireFailed()
    }
    return value
}

var checks: [(String, () throws -> Void)] = [
    ("DecodeTests.sampleFixtureDecodes", DecodeTests.sampleFixtureDecodes),
    ("DecodeTests.liveTickDecodes", DecodeTests.liveTickDecodes),
    ("DecodeTests.dumpTickFileDecodes", DecodeTests.dumpTickFileDecodes),
    ("DecodeTests.schemaPropertiesAllHaveCodingKeys", DecodeTests.schemaPropertiesAllHaveCodingKeys),
    ("DecodeTests.missingOptionalFieldsAndUnknownEnumsDoNotFail", DecodeTests.missingOptionalFieldsAndUnknownEnumsDoNotFail),
    ("DecodeTests.factValuesKeepTheirType", DecodeTests.factValuesKeepTheirType),
    ("DecodeTests.errorEnvelopeIsAFailure", DecodeTests.errorEnvelopeIsAFailure),
    ("DecodeTests.insightsComeFromTheLibrary", DecodeTests.insightsComeFromTheLibrary),
    ("PanelTests.rowsSortByMemoryMixSwarmsAndDoNotPinAct", PanelTests.rowsSortByMemoryMixSwarmsAndDoNotPinAct),
    ("PanelTests.cpuSortPutsUnknownLastAndShowsDash", PanelTests.cpuSortPutsUnknownLastAndShowsDash),
    ("PanelTests.confirmationListsEveryPidAndEstimatesOnlyFromReclaimable", PanelTests.confirmationListsEveryPidAndEstimatesOnlyFromReclaimable),
    ("PanelTests.systemOwnersAreProtected", PanelTests.systemOwnersAreProtected),
    ("PanelTests.resultShowsSignedChangeAndUnmeasured", PanelTests.resultShowsSignedChangeAndUnmeasured),
    ("PanelTests.forceStepReplacesStillAliveAndAddsChanges", PanelTests.forceStepReplacesStillAliveAndAddsChanges),
    ("PanelTests.forceCannotBeSentTwice", PanelTests.forceCannotBeSentTwice),
    ("PanelTests.footerShowsRefreshFailure", PanelTests.footerShowsRefreshFailure),
    ("PanelTests.pillsAndSubtitlesUseRealFields", PanelTests.pillsAndSubtitlesUseRealFields),
    ("PanelTests.headerPressureAndSystemAge", PanelTests.headerPressureAndSystemAge),
    ("PanelTests.formatting", PanelTests.formatting),
    ("EnvironmentTests.everyKeyHasBothLanguages", EnvironmentTests.everyKeyHasBothLanguages),
    ("EnvironmentTests.languageOrderMatchesCore", EnvironmentTests.languageOrderMatchesCore),
    ("EnvironmentTests.pathIsAppendedNeverReplaced", EnvironmentTests.pathIsAppendedNeverReplaced),
    ("EnvironmentTests.automationIsInertUnlessEnabled", EnvironmentTests.automationIsInertUnlessEnabled),
]
#if WHOATE_FIXTURE
checks.append(("DecodeTests.replayTickDecodes", DecodeTests.replayTickDecodes))
#endif

let filter = CommandLine.arguments.dropFirst().first
var passed = 0, failed = 0, skipped = 0
for (name, check) in checks where filter.map({ name.contains($0) }) ?? true {
    failures = []
    skipReason = nil
    do { try check() } catch { if !(error is RequireFailed) { fail("threw \(error)") } }
    if failures.isEmpty, let skipReason {
        skipped += 1
        print("– \(name) skipped: \(skipReason)")
    } else if failures.isEmpty {
        passed += 1
        print("✔ \(name)")
    } else {
        failed += 1
        print("✘ \(name)")
        failures.forEach { print("    \($0)") }
    }
}
print("WHOATE_CHECKS passed=\(passed) failed=\(failed) skipped=\(skipped)")
exit(failed == 0 && passed > 0 ? 0 : 1)
