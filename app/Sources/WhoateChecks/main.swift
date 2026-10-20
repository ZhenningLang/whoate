// Assertion runner used instead of `swift test`: under Command Line Tools the
// swift-testing macro plugin fails to load in about half of the builds
// (phases/05_gui HANDOFF). Exit status is non-zero on any failure or when no
// check ran.
import AppKit
import Foundation
import WhoateKit

// Some checks create real NSWindows; never show up as a foreground app.
NSApplication.shared.setActivationPolicy(.accessory)

// QA14 integration (tests/gui/tmux_jump.py): one real jump through WhoateKit's
// executor against the tmux server in TMUX_TMPDIR, then exit.
if let pane = ProcessInfo.processInfo.environment["WHOATE_TMUX_JUMP"] {
    let run = TmuxJump.processRunner()
    let (status, clients) = run(TmuxJump.listClients)
    let outcome = TmuxJump.jump(pane: pane, run: run)
    print("TMUX_JUMP \(outcome) list_clients_status=\(status) clients=\(TmuxJump.clients(clients).count) "
        + "tmpdir_set=\(ProcessInfo.processInfo.environment["TMUX_TMPDIR"] != nil)")
    if case .jumped = outcome { exit(0) }
    exit(1)
}

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
    ("Phase08Tests.derivedSessionsAreTitledAndSubtitledInOrder", Phase08Tests.derivedSessionsAreTitledAndSubtitledInOrder),
    ("Phase08Tests.cpuHeaderShowsUsageLoadAndBusyness", Phase08Tests.cpuHeaderShowsUsageLoadAndBusyness),
    ("Phase08Tests.adviceTextIsVisibleForEachEntry", Phase08Tests.adviceTextIsVisibleForEachEntry),
    ("Phase08Tests.curveDrawsOnlyTheLatestContinuousRun", Phase08Tests.curveDrawsOnlyTheLatestContinuousRun),
    ("Phase08Tests.automationTimersAreInertUnlessEnabled", Phase08Tests.automationTimersAreInertUnlessEnabled),
    ("Phase08Tests.hostCpuDecodesAndMatchesTheSchema", Phase08Tests.hostCpuDecodesAndMatchesTheSchema),
    ("Phase08Tests.englishTextHasNoCJK", Phase08Tests.englishTextHasNoCJK),
    ("Phase08Round3Tests.cpuAverageUsesOnlyTheLastTenSeconds", Phase08Round3Tests.cpuAverageUsesOnlyTheLastTenSeconds),
    ("Phase08Round3Tests.curveGapIsFortyFiveSecondsAndWindowThirtyMinutes", Phase08Round3Tests.curveGapIsFortyFiveSecondsAndWindowThirtyMinutes),
    ("Phase10DesignTests.sameNamedAppsGetDistinctTitlesAndAKindWord", Phase10DesignTests.sameNamedAppsGetDistinctTitlesAndAKindWord),
    ("Phase10DesignTests.swarmSubtitleNamesItsMainOwners", Phase10DesignTests.swarmSubtitleNamesItsMainOwners),
    ("Phase10DesignTests.menuBarItemIsNeverColoured", Phase10DesignTests.menuBarItemIsNeverColoured),
    ("Phase10DesignTests.mainWindowCategoriesAndAutomationSelection", Phase10DesignTests.mainWindowCategoriesAndAutomationSelection),
    ("Phase10DesignTests.detailCurveLeavesGapsAndCpuModeDrawsCpu", Phase10DesignTests.detailCurveLeavesGapsAndCpuModeDrawsCpu),
    ("Phase10Tests.partlyUnreadableCurvesDrawAndCarryOneN", Phase10Tests.partlyUnreadableCurvesDrawAndCarryOneN),
    ("Phase10Tests.sidebarFiltersAndCpuOrder", Phase10Tests.sidebarFiltersAndCpuOrder),
    ("Phase10Tests.cardsAppearOnlyWithData", Phase10Tests.cardsAppearOnlyWithData),
    ("Phase10Tests.systemServiceCardIsDisplayOnly", Phase10Tests.systemServiceCardIsDisplayOnly),
    ("Phase10Tests.overviewNamesWiredZonesAndGpuReason", Phase10Tests.overviewNamesWiredZonesAndGpuReason),
    ("Phase10Tests.detailsShowEverythingThePanelRowShows", Phase10Tests.detailsShowEverythingThePanelRowShows),
    ("Phase10Tests.toolbarUsesThePanelHeader", Phase10Tests.toolbarUsesThePanelHeader),
    ("Phase10Tests.tmuxJumpValidatesAndChoosesTheLatestClient", Phase10Tests.tmuxJumpValidatesAndChoosesTheLatestClient),
    ("Phase10Tests.titlesStayUniqueForThreeOrMoreAndSuffixCollisions", Phase10Tests.titlesStayUniqueForThreeOrMoreAndSuffixCollisions),
    ("Phase10Tests.allTotalCountsOwnersOnce", Phase10Tests.allTotalCountsOwnersOnce),
    ("Phase10Tests.cpuCurveBreaksWhereCpuIsMissing", Phase10Tests.cpuCurveBreaksWhereCpuIsMissing),
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
let asyncChecks: [(String, @MainActor () async throws -> Void)] = [
    ("RefreshTests.hiddenStaysPausedAndVisibleUsesOneEngine", RefreshTests.hiddenStaysPausedAndVisibleUsesOneEngine),
    ("RefreshTests.failuresPreserveBothGoodValuesAndIndependentErrors", RefreshTests.failuresPreserveBothGoodValuesAndIndependentErrors),
    ("RefreshTests.stopDropsLateTickAndRestartKeepsOneEngine", RefreshTests.stopDropsLateTickAndRestartKeepsOneEngine),
    ("RefreshTests.realWindowAdapterPublishesShowOrderOutAndClose", RefreshTests.realWindowAdapterPublishesShowOrderOutAndClose),
    ("RefreshTests.nativeHostDecodesTheNewAbi", RefreshTests.nativeHostDecodesTheNewAbi),
    ("RefreshTests.oldCloseCannotOverwriteAReattachedVisibleWindow", RefreshTests.oldCloseCannotOverwriteAReattachedVisibleWindow),
    ("RefreshTests.visibleCadenceIncludesWorkRatherThanAddingTwoSeconds", RefreshTests.visibleCadenceIncludesWorkRatherThanAddingTwoSeconds),
    ("Phase08Tests.panelAndMainWindowShareOneSort", Phase08Tests.panelAndMainWindowShareOneSort),
    ("Phase08Tests.panelCurvesUseRawHistoryNotTheTickCopy", Phase08Tests.panelCurvesUseRawHistoryNotTheTickCopy),
    ("Phase08Tests.mainWindowSwitchesPolicyAndIsRaisedNotDuplicated", { try Phase08Tests.mainWindowSwitchesPolicyAndIsRaisedNotDuplicated() }),
    ("Phase08Tests.quitGoesThroughOneInjectedTerminate", { try Phase08Tests.quitGoesThroughOneInjectedTerminate() }),
    ("Phase08Round3Tests.cpuOrderHoldsStillWhenLeadersAlternate", Phase08Round3Tests.cpuOrderHoldsStillWhenLeadersAlternate),
    ("Phase08Round3Tests.cpuOrderLetsARealJumpThrough", Phase08Round3Tests.cpuOrderLetsARealJumpThrough),
    ("Phase08Round3Tests.hoverFreezesCpuOrderOnly", Phase08Round3Tests.hoverFreezesCpuOrderOnly),
    ("Phase08Round3Tests.hiddenFullTicksEvery30SecondsPaused", Phase08Round3Tests.hiddenFullTicksEvery30SecondsPaused),
    ("Phase10Tests.menuBarImageIsAlwaysATemplate", { try Phase10Tests.menuBarImageIsAlwaysATemplate() }),
    ("Phase10Tests.openingTheMainWindowPutsThePanelAway", { try Phase10Tests.openingTheMainWindowPutsThePanelAway() }),
    ("Phase10Tests.mainWindowRowsGetHistories", Phase10Tests.mainWindowRowsGetHistories),
    ("Phase10Tests.jumpButtonIsGreyedOutsideTmuxAndResultsStayWithTheirRow", { try Phase10Tests.jumpButtonIsGreyedOutsideTmuxAndResultsStayWithTheirRow() }),
    ("Phase10Tests.scrolledIntoViewRowsGetHistories", Phase10Tests.scrolledIntoViewRowsGetHistories),
]
for (name, check) in asyncChecks where filter.map({ name.contains($0) }) ?? true {
    failures = []
    do { try await check() } catch { fail("threw \(error)") }
    if failures.isEmpty { passed += 1; print("✔ \(name)") }
    else { failed += 1; print("✘ \(name)"); failures.forEach { print("    \($0)") } }
}
print("WHOATE_CHECKS passed=\(passed) failed=\(failed) skipped=\(skipped)")
exit(failed == 0 && passed > 0 ? 0 : 1)
