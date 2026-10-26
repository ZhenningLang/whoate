// Phase 10 round 9 (owner check 2026-10-26 11:53, spec 末节 R22): QA65 an app
// opened with `open` from a tmux pane reads as the app, never as "the split it
// ran in is closed". The core gives it `app_bundle` plus weak `tmux_launch`.
import Foundation
@testable import WhoateKit

enum Phase10Round9Tests {
    static func openedApp() throws -> Owner {
        try decodeValue(Owner.self, ownerJSON("app:com.pilotmoon.scroll-reverser", mem: 22 << 20, pids: [62664], overrides: [
            "label": "Scroll Reverser",
            "evidence": [["kind": "app_bundle", "detail": "com.pilotmoon.scroll-reverser", "confidence": "certain"],
                         ["kind": "tmux_launch", "detail": "%505", "confidence": "weak"]],
            "origin": ["cwd": "/Users/demo", "tmux_pane": NSNull(), "pane_alive": NSNull()]]))
    }

    // QA65
    static func appOpenedFromAPaneReadsAsTheApp() throws {
        let app = try openedApp()
        let (zh, guess) = Detail.how(app, owners: [app], language: .zh)
        expect(zh == "Scroll Reverser 自己的进程。它是在 tmux 分屏里打开的，但由 macOS 启动，不归分屏管。" && !guess, zh)
        let (en, _) = Detail.how(app, owners: [app], language: .en)
        expect(en == "Scroll Reverser's own processes. It was opened from a tmux split, but macOS launched it, so the split does not own it.", en)
        for how in [zh, en] {
            for word in ["已经关闭", "已关闭", "is closed", "孤儿", "orphan", "%505"] { expect(!how.contains(word), "\(word): \(how)") }
        }
        let tree = Detail.relation(app, procs: [Phase10Round3Tests.proc(62664, 1, "Scroll Reverser", member: true, mem: 22 << 20)],
                                   report: nil, language: .zh)
        let text = tree.lines.map(\.name).joined(separator: "\n")
        expect(!text.contains(L10n.text(.relationPaneClosed, .zh)), text)
        let report = try decodeValue(Report.self, reportJSON(owners: [ownerJSON(app.id, mem: 22 << 20, pids: [62664], overrides: [
            "label": app.label, "evidence": [["kind": "app_bundle", "detail": "x", "confidence": "certain"],
                                             ["kind": "tmux_launch", "detail": "%505", "confidence": "weak"]]])]))
        let row = try require(Phase10Tests.rows(report).first)
        expect(!MainWindowModel.matches(row, .orphans), "not under orphans")
        // Control: a real orphan from a closed pane still says so.
        let orphan = try decodeValue(Owner.self, ownerJSON("orphan:7", mem: 1, kind: "orphan", pids: [7], overrides: [
            "label": "node", "evidence": [["kind": "tmux_environment", "detail": "%9", "confidence": "certain"]],
            "origin": ["tmux_pane": "%9", "pane_alive": false]]))
        expect(Detail.how(orphan, owners: [orphan], language: .zh).0.contains("它原来所在的分屏已经关闭。"))
    }
}
