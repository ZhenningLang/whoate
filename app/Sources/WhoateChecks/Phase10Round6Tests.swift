// Phase 10 round 6 (owner check 2026-10-25, spec「owner 点验反馈与裁决」R13–R14):
// QA54 the app icon in front of the main window's title, QA55 a double-clicked
// panel row opening the main window on that row.
import Foundation
@testable import WhoateKit

enum Phase10Round6Tests {
    typealias R3 = Phase10Round3Tests

    // QA54
    static func mainWindowShowsTheAppIconBeforeItsTitle() throws {
        let main = try R3.source("MainWindowView.swift"), app = try R3.source("WhoateApp.swift")
        expect(main.contains("ToolbarItem(placement: .navigation) { BrandIcon() }"), "icon in the title's place")
        // The system keeps drawing window.title: hiding it turns the toolbar compact and pulls the controls left.
        for hiding in ["titleVisibility = .hidden", "removing: .title", ".navigationTitle(\"\")"] {
            expect(!main.contains(hiding) && !app.contains(hiding), "the drawn title stays: \(hiding)")
        }
        expect(app.contains(#"Window(L10n.text(.mainWindowTitle, delegate.model.language), id: "main")"#))
        for language in [Language.zh, .en] { expect(L10n.text(.mainWindowTitle, language) == "whoate") }
        // The icon is the bundle's AppIcon.icns (bundle-app.sh copies it); no other image or text is added.
        expect(main.contains(#"Bundle.main.url(forResource: "AppIcon", withExtension: "icns")"#))
        expect(main.contains(".frame(width: 20, height: 20)"))
        let bundle = String(decoding: try repoFile("scripts/bundle-app.sh"), as: UTF8.self)
        expect(bundle.contains(#"cp "$logo/AppIcon.icns""#))
        let parsed = Automation(environment: ["WHOATE_UI_AUTOMATION": "main-window", "WHOATE_UI_APP_ICON": "/x/AppIcon.icns"])
        expect(parsed.appIcon == "/x/AppIcon.icns" && Automation.off.appIcon == nil)
    }

    // QA55
    static func doubleClickedPanelRowsOpenTheirRow() throws {
        let report = try Phase10Tests.designReport()
        let rows = Phase10Tests.rows(report)
        let panel = Phase10Tests.rows(report, limit: Panel.rowLimit)
        // Every panel row, with or without buttons, selects itself in the main window.
        for row in panel { expect(MainWindowModel.panelTarget(row, rows: rows) == row.id, row.id) }
        let swarm = try require(panel.first { $0.source == .swarm })
        expect(!swarm.closable && swarm.members.count == report.swarms[0].owners.count, "no buttons, knows its owners")
        // A process group gone from the list: its biggest listed member owner; nothing listed: window only.
        var group = swarmJSON("exe:node", mem: 9 << 30, count: 3)
        group["owners"] = [["id": "app:a", "count": 1], ["id": "app:b", "count": 2]]
        let small = try decodeValue(Report.self, reportJSON(
            owners: [ownerJSON("app:a", mem: 1 << 30), ownerJSON("app:b", mem: 2 << 30), ownerJSON("app:c", mem: 3 << 30)],
            swarms: [group]))
        let all = Panel.rows(small, sort: .memory, language: .zh, limit: .max)
        let node = try require(all.first { $0.source == .swarm })
        expect(node.members == ["app:a", "app:b"])
        let gone = all.filter { $0.id != node.id }
        expect(MainWindowModel.panelTarget(node, rows: gone) == "app:b", "the member with the most memory, not app:c")
        expect(MainWindowModel.panelTarget(node, rows: gone.filter { $0.id == "app:c" }) == nil, "no member listed: window only")
        let owner = try require(panel.first { $0.source == .owner })
        expect(MainWindowModel.panelTarget(owner, rows: rows.filter { $0.id != owner.id }) == nil, "a gone owner: window only")

        // The gesture: the whole row, after the buttons, through the R9 path and the panel put away.
        let view = try R3.source("PanelView.swift"), app = try R3.source("WhoateApp.swift")
        expect(view.contains(".contentShape(Rectangle())\n        .onTapGesture(count: 2, perform: doubleClick)"))
        expect(!view.contains("highPriorityGesture") && !view.contains("simultaneousGesture"), "buttons keep their own clicks")
        expect(view.contains("doubleClick: { openMain(model.panelTarget(row)) }"))
        expect(app.contains("openMain: { PanelActions.showMain($0, open: open, dismiss: delegate.dismissPanel) }"))

        let parsed = Automation(environment: [
            "WHOATE_UI_AUTOMATION": "panel-window",
            "WHOATE_UI_PANEL_DOUBLE_CLICKS": "4 pill app:com.brave.Browser; 6 row swarm:exe:python3.13;x row a;5 top a;-1 row a",
        ])
        expect(parsed.panelDoubleClicks == [
            .init(after: 4, part: "pill", rowID: "app:com.brave.Browser"), .init(after: 6, part: "row", rowID: "swarm:exe:python3.13"),
        ], "\(parsed.panelDoubleClicks)")
        expect(Automation(environment: ["WHOATE_UI_PANEL_DOUBLE_CLICKS": "4 row a"]).panelDoubleClicks.isEmpty, "inert without automation")
    }
}
