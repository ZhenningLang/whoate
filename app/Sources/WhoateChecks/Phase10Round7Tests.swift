// Phase 10 round 7 (owner checks 2026-10-25, spec 末节 R16–R19): QA57 the close
// confirmation as one sheet over the whole main window; QA59 git's fsmonitor in
// plain words (R17, GUI side); QA60 spread process groups say who uses them (R19);
// QA61 the bitten-pie logo in the menu bar (R20).
import AppKit
import Foundation
@testable import WhoateKit

enum Phase10Round7Tests {
    typealias R3 = Phase10Round3Tests

    // QA57
    static func closeConfirmationListsProgramsInPlainWords() throws {
        let estimate: [String: Any] = ["bytes": 4 << 20, "unknown_count": 0, "basis": "rss", "reason": NSNull()]
        var json = ownerJSON("orphan:7", mem: 9 << 20, kind: "orphan", pids: [7, 8, 9], overrides: ["label": "Python",
            "reclaimable_physical": estimate,
            "parts": [["name": "python3", "mem_bytes": 6 << 20, "process_count": 1, "pids": [7], "unknown_mem_count": 0],
                      ["name": "node", "mem_bytes": 3 << 20, "process_count": 2, "pids": [8, 9], "unknown_mem_count": 0]]])
        let owner = try decodeValue(Owner.self, json)
        let zh = KillConfirmation(owner: owner, language: .zh)
        expect(zh.title == "关闭「Python」？", zh.title)
        expect(zh.summary == "会结束 3 个进程，预计腾出约 4M 内存。", zh.summary)
        // Without whoate_why: names from the parts, memory only where a part is one process.
        expect(zh.processes.map(\.name) == ["python3", "node", "node"], "\(zh.processes)")
        expect(zh.processes.map(\.memory) == ["6M", "未测到", "未测到"], "\(zh.processes)")
        // With whoate_why: readable names and each process's memory, biggest first.
        let procs = [ProcBrief(pid: 8, ppid: 7, comm: "node", name: "Node.js", inOwner: true, memBytes: 5 << 20),
                     ProcBrief(pid: 7, ppid: 1, comm: "Python", inOwner: true, memBytes: 2 << 20)]
        let named = KillConfirmation(owner: owner, language: .en, procs: procs)
        expect(named.processes.map(\.pid) == [8, 7, 9] && named.processes.map(\.name) == ["Node.js", "Python", "node"])
        expect(named.processes.map(\.memory) == ["5M", "2M", "not measured"], "\(named.processes)")
        expect(named.summary == "Ends 3 processes and frees about 4M of memory.", named.summary)
        json["reclaimable_physical"] = NSNull()
        let unknown = KillConfirmation(owner: try decodeValue(Owner.self, json), language: .zh)
        expect(unknown.summary == "会结束 3 个进程，腾出多少内存无法预估。", unknown.summary)
        expect(L10n.text(.estimateBasis, .zh) == "按常驻内存估算，含共享部分，实际腾出的可能更少。")
        expect(L10n.format(.processNumber, .zh, "24897") == "进程号 24897")
    }

    // QA57
    static func closeConfirmationIsOneSheetOverTheWholeWindow() throws {
        let flow = try R3.source("CloseFlowView.swift"), main = try R3.source("MainWindowView.swift")
        let panel = try R3.source("PanelView.swift"), app = try R3.source("WhoateApp.swift")
        // The main window: a system sheet (centred by macOS), the three columns dimmed behind it.
        expect(main.contains(".sheet(isPresented: closeSheet)") && main.contains("CloseCard(model: model)"))
        expect(main.contains(".overlay { if closing { CloseDim()"), "dim over the split view")
        expect(!main.contains("DetailView(model: model)\n                .overlay"), "no longer only over the details")
        expect(main.contains(".interactiveDismissDisabled(busy)"), "stays up while a signal is out")
        // The panel keeps its own card (no sheets in a MenuBarExtra), same CloseCard, only for its own closes.
        expect(panel.contains(".overlay { CloseFlowView(model: model) }") && flow.contains("model.closeOrigin == .panel"))
        expect(panel.contains("model.requestClose(row.id, from: .panel)"))
        // Buttons: Esc on Cancel / Done, red filled destructive, Return bound to nothing.
        expect(flow.components(separatedBy: ".keyboardShortcut(.cancelAction)").count - 1 == 3, "cancel, done, failed done")
        expect(!flow.contains(".defaultAction") && !flow.contains(#".keyboardShortcut(.return"#), "Return ends nothing")
        expect(flow.contains("role: .destructive") && flow.contains(".buttonStyle(.borderedProminent).tint(.red)"))
        expect(flow.contains("destructive(.confirmButton)") && flow.contains("destructive(.forceButton)"))
        // Readable secondary text, never tertiary; the list scrolls past six rows.
        expect(!flow.contains(".tertiary"), "secondary text at least secondaryLabel")
        expect(flow.contains("static let visibleRows = 6"))
        // Automation that presses keys sends no signal.
        expect(app.contains("model.kill = { pids, _ in") && app.contains("killAttempts.add(pids.count)"))

        MainActor.assumeIsolated {
            let model = AppModel(language: .zh, makeEngine: { nil })
            expect(model.closeOrigin == .main)
        }
        let parsed = Automation(environment: ["WHOATE_UI_AUTOMATION": "main-window", "WHOATE_UI_CONFIRM": "selected",
                                              "WHOATE_UI_CLOSE_AT": "4", "WHOATE_UI_CLOSE_STEP": "force",
                                              "WHOATE_UI_ESC_AFTER": "8.5", "WHOATE_UI_RETURN_AFTER": "6"])
        expect(parsed.closeAt == 4 && parsed.closeStep == "force" && parsed.escAfter == 8.5 && parsed.returnAfter == 6)
        expect(Automation(environment: ["WHOATE_UI_ESC_AFTER": "1"]).escAfter == nil, "inert without automation")
    }

    static func gitMonitor(repo: Bool, pids: [Int32]) throws -> Owner {
        let evidence: [String: Any] = ["kind": "git_fsmonitor", "detail": repo ? "/Users/demo/Projects/Forge/.git" : "repository unreadable",
                                       "confidence": repo ? "certain" : "weak"]
        return try decodeValue(Owner.self, ownerJSON(
            // The core's id holds an opaque key, never the path (review round 7 B1).
            repo ? "service:git-fsmonitor:5b1d0e3c9a7f2e44" : "service:git-fsmonitor", mem: 4 << 20,
            kind: "service", pids: pids, overrides: [
                "label": repo ? "git fsmonitor · Forge" : "git fsmonitor", "evidence": [evidence],
                "origin": ["cwd": repo ? "/Users/demo/Projects/Forge" as Any : NSNull(), "tmux_pane": NSNull()]]))
    }

    // QA59
    static func gitMonitorReadsAsItsRepository() throws {
        let forge = try gitMonitor(repo: true, pids: [1, 2, 3, 4, 5])
        expect(RowNames.title(forge, .zh) == "git 文件监视 · Forge" && RowNames.title(forge, .en) == "git fsmonitor · Forge")
        expect(Panel.subtitle(forge, .zh) == "/Users/demo/Projects/Forge · ×5（含工作树）", Panel.subtitle(forge, .zh))
        let (how, guess) = Detail.how(forge, owners: [forge], language: .zh)
        expect(how == "这是 git 为 Forge 开的文件监视后台，git 故意让它脱离终端运行。" && !guess, how)
        expect(!how.contains("分屏") && !how.contains("pane"))
        let report = try decodeValue(Report.self, reportJSON(owners: [ownerJSON(
            forge.id, mem: 4 << 20, kind: "service", pids: [1, 2, 3, 4, 5], overrides: [
                "label": forge.label, "evidence": [["kind": "git_fsmonitor", "detail": "/x/.git", "confidence": "certain"]],
                "origin": ["cwd": "/Users/demo/Projects/Forge"]])]))
        let row = try require(Phase10Tests.rows(report).first)
        let header = Detail.header(forge, row: row, language: .zh)
        expect(header.verdict == "git 自带的后台程序，让 git status 更快。关了也会在下次用 git 时自动重开，一般不用管。")
        expect(!MainWindowModel.matches(row, .needsAction) && !MainWindowModel.matches(row, .orphans)
               && MainWindowModel.matches(row, .system), "normal, under system, never an orphan")
        expect(!row.closable)
        let lost = try gitMonitor(repo: false, pids: [9])
        expect(RowNames.title(lost, .zh) == "git 文件监视" && Panel.subtitle(lost, .zh) == "仓库读不到")
        expect(Detail.how(lost, owners: [lost], language: .zh).0.contains("仓库读不到"))
    }

    static func webkitReport(extraSwarms: [[String: Any]] = []) throws -> Report {
        var webkit = swarmJSON(#"["com.apple.WebKit"]"#, mem: 5_948 << 20, count: 16, cpu: 48)
        webkit["label"] = "com.apple.WebKit"
        webkit["verdict"] = ["level": "watch", "rule": "swarm-spread",
                             "text": "各归属分别启动 com.apple.WebKit，共 16 份 5948.7 MiB；关掉闲置归属能一起释放"]
        webkit["owners"] = [
            ["id": "app:com.apple.mail", "count": 4, "mem_bytes": 2_150 << 20, "cpu_pct": 30.0],
            ["id": "app:com.kingsoft.wpsoffice", "count": 3, "mem_bytes": 1_200 << 20, "cpu_pct": 8.0],
            ["id": "app:com.raycast.macos", "count": 3, "mem_bytes": 900 << 20],
            ["id": "app:agentsview", "count": 3, "mem_bytes": 1_000 << 20, "cpu_pct": 6.0],
            ["id": "service:7101", "count": 1, "mem_bytes": 300 << 20], ["id": "service:7102", "count": 1, "mem_bytes": 200 << 20],
            ["id": "service:7103", "count": 1],
        ]
        let owners = [("app:com.apple.mail", "Mail"), ("app:com.kingsoft.wpsoffice", "wpsoffice"),
                      ("app:com.raycast.macos", "Raycast"), ("app:agentsview", "AgentsView")].map {
            ownerJSON($0.0, mem: 3 << 30, overrides: ["label": $0.1])
        } + (7101...7103).map { ownerJSON("service:\($0)", mem: 1, kind: "service", overrides: ["label": "com.apple.WebKit"]) }
        return try decodeValue(Report.self, reportJSON(owners: owners, swarms: [webkit] + extraSwarms))
    }

    // QA60
    static func spreadGroupsAreNormalAndSayWhoUsesThem() throws {
        let report = try webkitReport(extraSwarms: [swarmJSON("burst", mem: 1 << 30, count: 30, shape: "burst")])
        let rows = Phase10Tests.rows(report)
        let spread = try require(rows.first { $0.id == #"swarm:["com.apple.WebKit"]"# })
        let burst = try require(rows.first { $0.id == "swarm:burst" })
        // Not in "needs action", grey pill; a burst (possible runaway) still is.
        expect(!MainWindowModel.matches(spread, .needsAction) && MainWindowModel.matches(burst, .needsAction))
        expect(spread.pill == "多个应用在用" && spread.pillLevel == .ok && spread.level == .ok)
        expect(MainWindowModel.total(rows, .needsAction) == "1", MainWindowModel.total(rows, .needsAction))
        expect(spread.title == "网页内容进程（WebKit） × 16", spread.title)
        expect(spread.subtitle == "16 个进程：Mail 4、wpsoffice 3、AgentsView 3…", spread.subtitle)

        let swarm = report.swarms[0]
        let owners = Dictionary(report.owners.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let users = Spread.users(swarm, owners: owners, language: .zh)
        // One line per recognised app, the three launchd-adopted ones merged; most memory first.
        expect(users.map(\.name) == ["Mail", "wpsoffice", "AgentsView", "Raycast", "没认出是哪个应用"], "\(users.map(\.name))")
        expect(users.last?.count == 3 && users.last?.memBytes == 500 << 20 && users.last?.id == nil)
        let memory = users.map { $0.memBytes ?? 0 }
        expect(memory == memory.sorted(by: >), "by memory")
        expect(users.compactMap(\.id) == ["app:com.apple.mail", "app:com.kingsoft.wpsoffice", "app:agentsview", "app:com.raycast.macos"])
        let summary = Spread.summary(swarm, users: users, language: .zh)
        // The app count is the list's recognised lines; the rest is said apart (review round 7 B).
        expect(summary == "Mail、wpsoffice、AgentsView、Raycast 各自用 macOS 的网页组件显示内容（另有 3 个进程没认出是哪个应用），每个页面或窗口一个进程，一共 16 个，占 5.8G。这是正常现象。", summary)
        let advice = Spread.advice(users, owners: owners, language: .zh)
        expect(advice == "想省内存，就关掉占得最多又暂时不用的那个应用，比如 Mail（2.1G）。", advice)
        let header = Detail.header(swarm, row: spread, language: .zh)
        expect(header.kindLine == "进程群 · 16 个进程" && header.verdict == nil, header.kindLine)
        for text in [summary, advice, header.kindLine, header.title, header.subtitle, spread.pill] + users.map(\.name) {
            for raw in ["com.apple.", "[\"", "MiB", "归属", "份"] { expect(!text.contains(raw), "\(raw) in \(text)") }
        }
        // Without memory per owner (older core): the general advice, the list by processes.
        var old = report.swarms[0]
        old.owners = old.owners.map { var m = $0; m.memBytes = nil; m.cpuPct = nil; return m }
        let plain = Spread.users(old, owners: owners, language: .zh)
        expect(Spread.advice(plain, owners: owners, language: .zh) == "想省内存，就关掉暂时不用的应用，它开的这些进程会跟着退出。")
        expect(plain.map(\.count) == [4, 3, 3, 3, 3], "\(plain.map(\.count))")
        // The view: the list rows open the owner (R9 path), the merged one opens nothing.
        let view = try R3.source("DetailView.swift")
        expect(view.contains("Button { select(id) } label: { userLine(user, link: true) }"))
        expect(view.contains("select: { model.select($0) })"))
        let helper = try decodeValue(Swarm.self, swarmJSON("com.example.Helper", mem: 1, count: 1))
        expect(Spread.name(helper, .zh) == "Helper")
        try unrecognisedOwnersAreOneLine()
    }

    /// Review round 7 (CONFORM:B2, A [should]): services and system processes with any
    /// label, and owners missing from the report, are all the one "not recognised" line.
    static func unrecognisedOwnersAreOneLine() throws {
        var webkit = swarmJSON(#"["com.apple.WebKit"]"#, mem: 600 << 20, count: 6)
        webkit["label"] = "com.apple.WebKit"
        webkit["owners"] = [
            ["id": "app:mail", "count": 1, "mem_bytes": 400 << 20],
            ["id": "service:1", "count": 1, "mem_bytes": 100 << 20], ["id": "service:2", "count": 1, "mem_bytes": 100 << 20],
            ["id": "system:3", "count": 1, "mem_bytes": 50 << 20],
            ["id": "app:gone", "count": 1, "mem_bytes": 30 << 20], ["id": "agent:claude:gone", "count": 1, "mem_bytes": 20 << 20],
        ]
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("app:mail", mem: 1 << 30, overrides: ["label": "Mail"]),
            ownerJSON("service:1", mem: 1, kind: "service", overrides: ["label": "com.apple.helperA"]),
            ownerJSON("service:2", mem: 1, kind: "service", overrides: ["label": "helper-b"]),
            ownerJSON("system:3", mem: 1, kind: "system", overrides: ["label": "WindowServer"]),
        ], swarms: [webkit]))
        let owners = Dictionary(report.owners.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let users = Spread.users(report.swarms[0], owners: owners, language: .zh)
        expect(users.map(\.name) == ["Mail", "没认出是哪个应用"], "\(users.map(\.name))")
        expect(users[1].count == 5 && users[1].memBytes == 300 << 20 && users[1].id == nil)
        let summary = Spread.summary(report.swarms[0], users: users, language: .zh)
        // One app "uses" (not "each"); none recognised says so instead of "more" (review round 7 B).
        expect(summary.hasPrefix("Mail 用 macOS 的网页组件显示内容（另有 5 个进程没认出是哪个应用）"), summary)
        let none = Spread.users(report.swarms[0], owners: [:], language: .zh)
        let noneSummary = Spread.summary(report.swarms[0], users: none, language: .zh)
        expect(none.count == 1 && noneSummary.hasPrefix("多个应用各自用 macOS 的网页组件显示内容（都没认出是哪个应用），"), noneSummary)
        expect(Spread.summary(report.swarms[0], users: users, language: .en).hasPrefix("Mail uses macOS's web component"))
    }

    // QA61
    @MainActor static func menuBarShowsTheMonochromePie() throws {
        let pdf = repoRoot.appendingPathComponent("assets/logo/menubar-template.pdf")
        let logo = try require(NSImage(contentsOf: pdf))
        expect(logo.size == NSSize(width: 18, height: 18), "\(logo.size)")
        // AppKit really draws it (rsvg's own PDF of the masked SVG drew nothing): black on transparent only.
        let rep = try require(NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: 72, pixelsHigh: 72, bitsPerSample: 8,
                                               samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                                               bytesPerRow: 0, bitsPerPixel: 0))
        NSGraphicsContext.saveGraphicsState()
        NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
        logo.draw(in: NSRect(x: 0, y: 0, width: 72, height: 72))
        NSGraphicsContext.restoreGraphicsState()
        var opaque = 0, clear = 0, coloured = 0
        for y in 0..<72 {
            for x in 0..<72 {
                let c = try require(rep.colorAt(x: x, y: y))
                if c.alphaComponent > 0.5 { opaque += 1 } else if c.alphaComponent == 0 { clear += 1 }
                if c.alphaComponent > 0.5, max(c.redComponent, c.greenComponent, c.blueComponent) > 0.1 { coloured += 1 }
            }
        }
        // The pie covers about two fifths of the square (measured 2295 of 5184); the corners stay clear.
        expect(opaque > 1500 && opaque < 3000 && clear > 2000 && coloured == 0, "opaque=\(opaque) clear=\(clear) coloured=\(coloured)")
        let image = MenuBarItem.image(.logo, logo: logo)
        expect(image.isTemplate && image.size == NSSize(width: 18, height: 18))
        // Normal pressure shows the pie in every style; alerts keep their symbols.
        let host = try decodeValue(Host.self, ["mem_total_bytes": 1, "mem_used_bytes": 1, "compressed_bytes": 0,
                                               "wired_bytes": 0, "swap_used_bytes": 0, "pressure": "normal"])
        for style in MenuBarItem.AlertStyle.allCases { expect(MenuBarItem.label(host, style: style, language: .zh).image == .logo) }
        // Reproducible: the icon script draws, traces and packs every size.
        let script = String(decoding: try repoFile("scripts/bundle-app.sh"), as: UTF8.self)
        expect(script.contains(#"potrace "$tmp/template.bmp" -b pdf -W 18pt -H 18pt"#) && script.contains("for size in 16 32 128 256 512"))
    }
}
