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
        expect(zh.summary == "会结束 3 个进程，预计腾出约 4M 物理内存。", zh.summary)
        // Without whoate_why: names from the parts, memory only where a part is one process.
        expect(zh.processes.map(\.name) == ["python3", "node", "node"], "\(zh.processes)")
        expect(zh.processes.map(\.memory) == ["6M", "未测到", "未测到"], "\(zh.processes)")
        // With whoate_why: readable names and each process's memory, biggest first.
        let procs = [ProcBrief(pid: 8, ppid: 7, comm: "node", name: "Node.js", inOwner: true, memBytes: 5 << 20),
                     ProcBrief(pid: 7, ppid: 1, comm: "Python", inOwner: true, memBytes: 2 << 20)]
        let named = KillConfirmation(owner: owner, language: .en, procs: procs)
        expect(named.processes.map(\.pid) == [8, 7, 9] && named.processes.map(\.name) == ["Node.js", "Python", "node"])
        expect(named.processes.map(\.memory) == ["5M", "2M", "not measured"], "\(named.processes)")
        expect(named.summary == "Ends 3 processes and frees about 4M of physical memory.", named.summary)
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
        expect(MainWindowModel.total(rows, .needsAction, .zh).text == "1", MainWindowModel.total(rows, .needsAction, .zh).text)
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
        expect(summary == "Mail、wpsoffice、AgentsView、Raycast 各自用 macOS 的网页组件显示内容（另有 3 个进程没认出是哪个应用），每个页面或窗口一个进程，一共 16 个，占用 5.8G。这是正常现象。", summary)
        let advice = Spread.advice(users, owners: owners, language: .zh)
        expect(advice == "想省内存，就关掉占得最多又暂时不用的那个应用，比如 Mail（占用 2.1G）。", advice)
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

    // QA63 (round 8, R21): every memory number says what it is.
    static func memoryNumbersSayWhatTheyAre() throws {
        let host = try decodeValue(Host.self, ["mem_total_bytes": 24 << 30, "mem_used_bytes": 19 << 30,
                                               "compressed_bytes": 9 << 30, "wired_bytes": 4 << 30,
                                               "swap_used_bytes": 9_728 << 20, "swap_total_bytes": 10 << 30, "pressure": "warn"])
        expect(MemoryWording.physical(host, .zh) == "物理内存 19.0G / 24.0G · 79%", MemoryWording.physical(host, .zh))
        expect(MemoryWording.compressed(host, .zh) == "其中压缩 9.0G")
        expect(MemoryWording.swap(host, .zh) == "swap 9.5G / 10.0G · 95%", MemoryWording.swap(host, .zh))
        expect(MemoryWording.hostLine(host, .en) == "Physical memory 19.0G / 24.0G · 79% · incl. compressed 9.0G · swap 9.5G / 10.0G · 95%")
        var old = host
        old.swapTotalBytes = nil
        expect(MemoryWording.swap(old, .zh) == "swap 已用 9.5G", "an older core: used only")
        expect(MemoryWording.hostHelp(.zh).contains("macOS 会按需扩大") && MemoryWording.hostHelp(.zh).contains("是已用的一部分"))
        // One label per line, the value without a second one (review round 8 B [should]).
        let overview = Detail.overview(host, language: .zh).memory.map { "\($0.0) \($0.1)" }
        expect(overview == ["物理内存 19.0G / 24.0G · 79%", "其中压缩 9.0G（是已用的一部分）", "其中联动 4.0G（是已用的一部分）",
                            "swap 9.5G / 10.0G · 95%（macOS 会按需扩大 swap）"], "\(overview)")

        // One owner: physical part (RSS, at most the total), the rest compressed or swapped.
        let rss: (UInt64?) -> [String: Any] = { bytes in
            ["bytes": bytes.map { $0 as Any } ?? NSNull(), "unknown_count": bytes == nil ? 1 : 0, "basis": "rss", "reason": NSNull()]
        }
        let owner = try decodeValue(Owner.self, ownerJSON("app:a", mem: 10 << 30, overrides: ["reclaimable_physical": rss(4 << 30)]))
        expect(MemoryWording.splitLine(owner, .zh) == .init(text: "（物理内存约 4.0G + 压缩/swap 约 6.0G）", help: nil))
        expect(MemoryWording.splitLine(owner, .en).text == " (~4.0G physical + ~6.0G compressed/swap)")
        // Owner 2026-10-28: every main-window memory number reads "total (physical + compressed/swap)":
        // the row beside its number, the detail inline after the header's footprint, a kind on hover.
        let splitReport = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("app:a", mem: 10 << 30, overrides: ["reclaimable_physical": rss(4 << 30)])]))
        let splitRows = Phase10Tests.rows(splitReport)
        let splitRow = try require(splitRows.first { $0.id == "app:a" })
        expect(MemoryWording.parts(splitRow) == "(4.0G + 6.0G)", MemoryWording.parts(splitRow) ?? "nil")
        expect(MemoryWording.inline(splitRow, .zh) == "（物理内存约 4.0G + 压缩/swap 约 6.0G）")
        expect(MainWindowModel.total(splitRows, .apps, .zh).help == "物理内存约 4.0G + 压缩/swap 约 6.0G",
               MainWindowModel.total(splitRows, .apps, .zh).help ?? "nil")
        expect(Detail.header(owner, row: splitRow, language: .zh).memorySplit == nil, "a split footprint is shown inline")
        // Version: which code and when, so two builds of one version differ (owner 2026-10-28).
        let about = AboutInfo.make(info: ["CFBundleShortVersionString": "0.1.0", "CFBundleVersion": "0.1.0",
                                          "WhoateCommit": "e12f515", "WhoateBuiltAt": "2026-10-28 23:22"], language: .zh)
        expect(about.build == "e12f515 · 2026-10-28 23:22" && about.short == "v0.1.0 · e12f515", "\(about)")
        expect(AboutInfo.make(info: nil, language: .zh).short == "开发版")
        // Owner 2026-10-29: in CPU order the sidebar adds the rows' CPU, per core like a row.
        let cpuReport = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("app:c1", mem: 1 << 30, overrides: ["cpu_pct": 93.0]),
            ownerJSON("app:c2", mem: 1 << 30, overrides: ["cpu_pct": 22.0])]))
        let cpuApps = MainWindowModel.total(Phase10Tests.rows(cpuReport), .apps, .zh, sort: .cpu)
        expect(cpuApps.text == "115%" && cpuApps.help == L10n.text(.cpuKindHelp, .zh), "\(cpuApps)")
        expect(MainWindowModel.total(Phase10Tests.rows(cpuReport), .apps, .zh).text == "2.0G", "memory order unchanged")
        expect(L10n.text(.cpuSectionOwners, .zh) == "归属 · CPU")
        // Owner 2026-10-29: a titled session's tool comes first, so a narrow list (which cuts the
        // middle) still says it is claude; a title that already starts with the tool is not repeated.
        let designRows = Phase10Tests.rows(try Phase10Tests.designReport())
        let titled = try require(designRows.first { $0.id == "agent:claude:9f2c41d7-5b0e-4c1a-9d3e-2a7f60c1b8e4" })
        expect(titled.subtitle.hasPrefix("claude · Ghostty · tmux main:1.0"), titled.subtitle)
        // A title that only starts with the word keeps the tool (owner's "CLAUDE_KICKOFF 交接任务").
        let kickoff = try decodeValue(Report.self, reportJSON(owners: [
            try Phase08Tests.session("agent:claude:k", name: "7-f2", source: "derived", title: "CLAUDE_KICKOFF 交接任务")]))
        let kickoffRow = try require(Phase10Tests.rows(kickoff).first)
        expect(kickoffRow.title == "CLAUDE_KICKOFF 交接任务" && kickoffRow.subtitle.hasPrefix("claude"), "\(kickoffRow.title) | \(kickoffRow.subtitle)")
        for row in designRows where row.kind == .agentSession && row.title.lowercased().hasPrefix("kilo · ") {
            expect(!row.subtitle.lowercased().hasPrefix("kilo"), "not repeated: \(row.title) | \(row.subtitle)")
        }
        // RSS counts shared pages: the estimate is capped at the footprint (min(RSS, footprint)).
        let shared = try decodeValue(Owner.self, ownerJSON("app:b", mem: 1 << 30, overrides: ["reclaimable_physical": rss(3 << 30)]))
        expect(MemoryWording.split(shared) == .init(physical: 1 << 30, outside: 0, total: 1 << 30), "capped at the footprint")
        // Review round 8 (A:B2, B:B2, CONFORM:B2): the split keeps the header's precision.
        // Some footprints unread: a lower bound, no split (a difference from it would claim too much).
        let partial = try decodeValue(Owner.self, ownerJSON("app:c", mem: 4_300 << 20, pids: [1, 2, 3, 4],
                                                            overrides: ["unknown_mem_count": 3, "reclaimable_physical": rss(1 << 30)]))
        expect(MemoryWording.split(partial).physical == nil && MemoryWording.split(partial).outside == nil)
        expect(MemoryWording.splitLine(partial, .zh) == .init(text: "合计（占用）≥ 4.2G · 有进程读不到，拆不出物理内存和压缩/swap",
                                                             help: "3 个进程读不到内存"), "\(MemoryWording.splitLine(partial, .zh))")
        expect(MemoryWording.splitLine(partial, .en).text.hasPrefix("Total (footprint) ≥ 4.2G · some processes unreadable"))
        // None read: never "0B".
        let none = try decodeValue(Owner.self, ownerJSON("system:1", mem: 0, kind: "system", pids: [1, 2],
                                                         overrides: ["unknown_mem_count": 2, "reclaimable_physical": rss(nil)]))
        expect(MemoryWording.splitLine(none, .zh) == .init(text: "合计（占用）读不到", help: "2 个进程读不到内存"))
        expect(MemoryWording.splitLine(none, .en).text == "Total (footprint) not readable")
        // Only the RSS unread: the total is exact, the physical part is not known.
        let noRSS = try decodeValue(Owner.self, ownerJSON("app:d", mem: 1 << 30, overrides: ["reclaimable_physical": rss(nil)]))
        expect(MemoryWording.splitLine(noRSS, .zh) == .init(text: "合计（占用）1.0G · 物理内存部分读不到", help: nil))
        expect(MemoryWording.splitLine(noRSS, .en).text == "Total (footprint) 1.0G · physical part not readable")
        // The detail line and the header number agree (design report Brave: ≥ 4.2G, 3 unread).
        let design = try Phase10Tests.designReport()
        let braveOwner = try require(design.owners.first { $0.id == "app:com.brave.Browser" })
        let braveRow = try require(Phase10Tests.rows(design).first { $0.id == braveOwner.id })
        let line = try require(Detail.header(braveOwner, row: braveRow, language: .zh).memorySplit)
        expect(line.text.contains(braveRow.memory(.zh).text) && line.help == braveRow.memory(.zh).help, line.text)
        // R23 (owner 2026-10-26, replaces R21's physical sidebar): the kinds add the footprints
        // their list shows, the section says so, and "all" says why it may exceed physical memory.
        let report = try Phase10Tests.designReport()
        let rows = Phase10Tests.rows(report)
        let all = MainWindowModel.total(rows, .all, .zh)
        let owners = rows.filter { $0.source == .owner }
        expect(all.text == Format.memory(owners.reduce(0) { $0 + $1.memBytes },
                                         unknown: owners.reduce(0) { $0 + $1.unknownMemCount }, .zh).text, all.text)
        expect(all.text.hasPrefix("≥ "), "the design report's system owner is unreadable: \(all.text)")
        expect(all.help?.hasPrefix(L10n.text(.memAllHelp, .zh)) == true && all.help?.contains("读不到内存") == true, all.help ?? "")
        expect(L10n.text(.memAllHelp, .zh).contains("可以超过物理内存") && !L10n.text(.memAllHelp, .zh).contains("含压缩"))
        expect(MemoryWording.hostHelp(.zh).contains("左栏和列表都是各归属的占用") && !MemoryWording.hostHelp(.zh).contains("比已用少"))
        expect(L10n.text(.memSectionOwners, .zh) == "归属 · 占用" && L10n.text(.memSectionOwners, .en) == "Owners · footprint")
        // Defined, not "includes" (owner turned that wording down): 占用 = 物理内存 + 压缩/swap.
        expect(L10n.format(.memListHeader, .zh, "6") == "6 个归属 · 按占用排序")
        expect(L10n.text(.memPanelCaption, .zh) == "占用 = 物理内存 + 压缩/swap")
        for key in [L10nKey.memListHeader, .memPanelCaption, .memProcessCaption] {
            expect(!L10n.text(key, .zh).contains("含压缩"), "\(key)")
        }
        expect(L10n.text(.curve30, .zh).hasPrefix("占用"))

        // No view formats a host or owner number by hand any more: they go through MemoryWording
        // or sit under a label (card titles carry " · 占用", rows a column header).
        for (file, text) in try R3.viewSources() {
            for raw in ["host.memUsedBytes", "host.swapUsedBytes", "host.compressedBytes", "L10n.text(.memoryUsed"] {
                expect(!text.contains(raw), "\(file): \(raw)")
            }
        }
        let detail = try R3.source("DetailView.swift"), main = try R3.source("MainWindowView.swift")
        expect(detail.components(separatedBy: "L10n.text(.memCardSuffix, language)").count - 1 == 5, "five memory cards say 占用")
        expect(detail.contains("Text(L10n.text(.memFootprint, language))") && detail.contains("header.memorySplit"))
        expect(main.contains("Section(L10n.text(model.sort == .cpu ? .cpuSectionOwners : .memSectionOwners, language))") && main.contains("MemoryWording.physical(host, language)")
               && main.contains("Text(MemoryWording.swap(host, language))"), "the toolbar's two lines")
        // The list defines its number under the count line, as the panel does above its rows.
        expect(main.contains("Text(L10n.text(.memPanelCaption, language))"))
        let panel = try R3.source("PanelView.swift"), flow = try R3.source("CloseFlowView.swift")
        expect(panel.contains("Text(L10n.text(.memPanelCaption, language))"))
        expect(flow.contains("Text(L10n.text(.memProcessCaption, language))"))
    }

    /// Review round 8 (B:B1, spec R21 「断言文本里没有孤立的 NN.NG」): every memory number
    /// the views show, with the block title it sits under; returns those whose own text
    /// and block title both lack a kind word.
    static func unlabelled(_ shown: [(block: String, text: String)], _ language: Language) -> [String] {
        // The core's own units too ("9.70 GiB", "+500 MiB"; review round 8 B:B1).
        let number = try! NSRegularExpression(pattern: #"\d+(?:\.\d+)?\s?(?:[KMGT]i?B?|B)(?![A-Za-z])"#)
        let words = language == .zh ? ["物理内存", "压缩", "swap", "占用", "联动", "显存", "内核内存区"]
            : ["physical", "compressed", "swap", "footprint", "wired", "gpu", "kernel"]
        return shown.compactMap { block, text in
            let hasNumber = number.firstMatch(in: text, range: NSRange(text.startIndex..., in: text)) != nil
            let said = (block + " " + text).lowercased()
            return hasNumber && !words.contains { said.contains($0.lowercased()) } ? "[\(block)] \(text)" : nil
        }
    }

    // QA63 (review round 8 B:B1)
    static func everyShownMemoryNumberHasALabel() throws {
        expect(!unlabelled([("", "4.2G")], .zh).isEmpty && unlabelled([("", "占用 4.2G")], .zh).isEmpty, "the check itself")
        // A verdict sentence in verdict/rules.rs's own format is caught, and shown labelled.
        for (raw, language) in [("启动它的 pane 已关闭（时长未知），占 9.70 GiB", Language.zh), ("近 10 分钟 +500 MiB，还在涨", .zh),
                                ("Its launching pane is closed (duration unknown); using 9.70 GiB", .en),
                                ("+500 MiB over 10 minutes and still growing", .en)] {
            expect(!unlabelled([("", raw)], language).isEmpty, "caught: \(raw)")
            expect(unlabelled([("", MemoryWording.verdict(raw, language))], language).isEmpty, MemoryWording.verdict(raw, language))
        }
        let report = try Phase10Tests.designReport()
        let host = try require(report.host)
        let owners = Dictionary(report.owners.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        for language in [Language.zh, .en] {
            let t = { (key: L10nKey) in L10n.text(key, language) }
            let rows = Phase10Tests.rows(report, language)
            let list = L10n.format(.memListHeader, language, "\(rows.count)") + " " + t(.memPanelCaption)
            var shown: [(block: String, text: String)] = [
                ("", MemoryWording.physical(host, language)), ("", MemoryWording.compressed(host, language)),
                ("", MemoryWording.swap(host, language)), ("", MemoryWording.hostLine(host, language)),
            ]
            let overview = Detail.overview(host, language: language)
            shown += overview.memory.map { ($0.0, $0.1) }
            shown += overview.zones.map { (overview.zonesTitle ?? "", $0.1) }
            shown += overview.gpu.map { [(t(.gpuMemory), $0)] } ?? []
            shown += [MainCategory.all, .tmux, .agents, .apps, .orphans, .system].map {
                (t(.memSectionOwners), MainWindowModel.total(rows, $0, language).text)
            }
            for row in rows {
                shown += [(list, row.memory(language).text), (list, row.subtitle), ("", t(.memFootprint) + " " + row.memory(language).text)]
                shown.append((t(.memPanelCaption), row.memory(language).text))
            }
            let suffix = t(.memCardSuffix)
            for owner in report.owners {
                let row = try require(rows.first { $0.id == owner.id })
                let header = Detail.header(owner, row: row, language: language)
                shown += [("", t(.memFootprint) + " " + row.memory(language).text), ("", header.memorySplit?.text ?? ""),
                          ("", header.verdict ?? "")]
                for card in Detail.cards(owner, history: [], catalog: [:], language: language, report: report) {
                    switch card {
                    case .relation(let r):
                        let block = t(.relation) + suffix
                        shown += (r.lines + r.rest).compactMap { $0.memory.map { (block, $0.text) } }
                        shown += r.restSummary.map { [(block, $0)] } ?? []
                    case .parts(let parts):
                        let block = t(owner.agent != nil ? .processesInSession : .parts) + suffix
                        shown += parts.map { (block, Format.memory($0.memBytes, unknown: $0.unknownMemCount, language).text) }
                    case .folders(let groups):
                        shown += groups.map { (t(.byFolder) + suffix, Format.memory($0.memBytes, unknown: $0.unknown, language).text) }
                    case .paneSessions(let sessions):
                        shown += sessions.map { (t(.paneSessions) + suffix, $0.memory.text) }
                    default: break
                    }
                }
                let close = KillConfirmation(owner: owner, language: language)
                shown.append(("", close.summary))
                shown += close.processes.map { (t(.memProcessCaption), $0.memory) }
            }
            for swarm in report.swarms {
                if let row = rows.first(where: { $0.id == swarm.id }) {
                    shown.append(("", Detail.header(swarm, row: row, language: language).verdict ?? ""))
                }
                let users = Spread.users(swarm, owners: owners, language: language)
                shown += users.map { (t(.spreadUsers) + suffix, $0.memBytes.map(Format.bytes) ?? "—") }
                shown += [("", Spread.summary(swarm, users: users, language: language)),
                          ("", Spread.advice(users, owners: owners, language: language))]
            }
            let offenders = unlabelled(shown, language)
            expect(offenders.isEmpty, "\(language): \(offenders)")
            expect(shown.count > 100, "the check saw the views' numbers: \(shown.count)")
        }
        // The one registered exception (app/DESIGN.md §12): the menu bar number has no room for a word.
        let bar = MenuBarItem.label(host, style: .systemSymbol, language: .zh)
        expect(!unlabelled([("", bar.text)], .zh).isEmpty, "menu bar number: the documented exception")
    }

    /// Review round 8 B:B1: the core's verdict sentences in the GUI's units, with the label.
    static func verdictSentencesGetUnitsAndTheLabel() throws {
        let cases: [(String, Language, String)] = [
            ("启动它的 pane 已关闭（时长未知），占 9.70 GiB", .zh, "启动它的 pane 已关闭（时长未知），占用 9.7G"),
            ("无人管理的开发进程，占 1.25 GiB", .zh, "无人管理的开发进程，占用 1.2G"),
            ("近 10 分钟 +500 MiB，还在涨", .zh, "近 10 分钟 +500M（占用），还在涨"),
            ("启动后 10 分钟内涨到 9.7G，之后不再变化。", .zh, "启动后 10 分钟内涨到 9.7G（占用），之后不再变化。"),
            ("父进程 [1] 在 60s 内拉起了 20 个相同的 node，合计 104.0 MiB，疑似失控", .zh, "父进程 [1] 在 60s 内拉起了 20 个相同的 node，合计 104M（占用），疑似失控"),
            ("Its launching pane is closed (duration unknown); using 9.70 GiB", .en, "Its launching pane is closed (duration unknown); footprint 9.7G"),
            ("+500 MiB over 10 minutes and still growing", .en, "+500M (footprint) over 10 minutes and still growing"),
            ("空闲 7200 秒，可关", .zh, "空闲 7200 秒，可关"),
            ("占用 9.7G，已标好", .zh, "占用 9.7G，已标好"),
            // Review round 8 B: an English label already there; a name with "4K" in it.
            ("footprint 9.7G and steady", .en, "footprint 9.7G and steady"),
            ("父进程 [1] 在 60s 内拉起了 20 个相同的 4K Video Downloader，合计 104.0 MiB，疑似失控", .zh,
             "父进程 [1] 在 60s 内拉起了 20 个相同的 4K Video Downloader，合计 104M（占用），疑似失控"),
        ]
        for (raw, language, want) in cases {
            expect(MemoryWording.verdict(raw, language) == want, "\(raw) → \(MemoryWording.verdict(raw, language))")
        }
        // The detail header shows it so; the core's text itself is untouched (CLI / JSON).
        let owner = try decodeValue(Owner.self, ownerJSON("orphan:1", mem: 9 << 30, level: "act", rule: "orphan-heavy", kind: "orphan",
                                                          overrides: ["verdict": ["level": "act", "rule": "orphan-heavy",
                                                                                  "text": "无人管理的开发进程，占 9.00 GiB"]]))
        let report = try decodeValue(Report.self, reportJSON(owners: [ownerJSON("orphan:1", mem: 9 << 30, kind: "orphan")]))
        let row = try require(Phase10Tests.rows(report).first)
        expect(Detail.header(owner, row: row, language: .zh).verdict == "无人管理的开发进程，占用 9.0G")
        expect(owner.verdict.text == "无人管理的开发进程，占 9.00 GiB")
    }
}

