// Phase 10 (08b): curve marks for unreadable memory (QA8), main window model
// and cards (QA9), same pills in panel and details (QA10), toolbar header
// (QA12), advice card (QA13), tmux jump (QA14), template menu bar image
// (QA15), panel put away after opening the main window (QA16), quit button
// layout in source (QA17), and histories for main window rows.
import AppKit
import Foundation
@testable import WhoateKit

enum Phase10Tests {
    static func designReport() throws -> Report { try Phase10DesignTests.designReport() }
    static func rows(_ report: Report, _ language: Language = .zh, limit: Int = .max) -> [PanelRow] {
        RowNames.apply(Panel.rows(report, sort: .memory, language: language, limit: .max), report: report, language: language)
            .prefix(limit).map { $0 }
    }
    static func points(_ times: [Int64], unknown: Int? = nil) -> [HistoryPoint] {
        times.map { HistoryPoint(timestampS: $0, memBytes: 100 + UInt64($0), cpuPct: nil, unknownMemCount: unknown) }
    }

    // QA8
    static func partlyUnreadableCurvesDrawAndCarryOneN() throws {
        let partial = points(Array(stride(from: Int64(0), through: 60, by: 20)), unknown: 3)
        guard case .line = Format.trend(partial, width: 64, height: 16) else { return fail("partly readable still draws") }
        // The number's mark is this tick's count only (QA41; round 3 conform review B1),
        // so the old history-maximum assertions moved to Phase10Round3Tests.
        expect(L10n.format(.unknownMem, .zh, "3").hasPrefix("3 个进程读不到内存"))
        expect(L10n.format(.unknownMem, .en, "3").hasPrefix("3 processes unreadable"))
        expect(Format.trend(points([0, 20]), width: 64, height: 16) == .insufficient, "20 s is too short")
        expect(Format.trend(points([0, 50]), width: 64, height: 16) == .insufficient, "two points 50 s apart are not joined")
        // The design report's Brave: unreadable now, so its row reads "≥ 4.2G" with N = 3.
        let report = try designReport()
        let brave = try require(rows(report).first { $0.id == "app:com.brave.Browser" })
        expect(brave.memory(.zh).text == "≥ 4.2G" && brave.memory(.zh).help?.hasPrefix("3 ") == true)
    }

    // QA9
    static func sidebarFiltersAndCpuOrder() throws {
        let report = try designReport()
        let all = rows(report)
        // R23 (round 10, replaces R21's physical sum): kinds add their owners' footprints (an
        // unreadable one makes it "≥"); since R24 the AI apps count under "AI & Agent", not "Apps".
        let footprint = { (owners: [Owner]) in
            Format.memory(owners.reduce(0) { $0 + $1.memBytes }, unknown: owners.reduce(0) { $0 + $1.unknownMemCount }, .zh).text
        }
        let apps = report.owners.filter { $0.kind == .app && !MainWindowModel.aiApps.contains($0.bundleID ?? "") }
        // Brave's footprint is partly unreadable: the apps' total is a lower bound.
        expect(MainWindowModel.total(all, .apps, .zh).text == footprint(apps) && footprint(apps).hasPrefix("≥"))
        let system = report.owners.filter { $0.kind == .system || $0.kind == .service }
        // Every system footprint was read: exact (R21's "≥" came from the RSS it lacked).
        expect(MainWindowModel.total(all, .system, .zh).text == footprint(system) && !footprint(system).hasPrefix("≥"))
        let cpu = Panel.rows(report, sort: .cpu, language: .zh, limit: .max)
        expect(cpu.first?.id == "agent:claude:9f2c41d7-5b0e-4c1a-9d3e-2a7f60c1b8e4", "highest CPU first, act not pinned")
        expect(cpu.firstIndex { $0.id == "orphan:948211" }! > 0, "the act orphan (0.1 %) is not on top")
    }

    static func cardsAppearOnlyWithData() throws {
        let report = try designReport()
        let bare = try decodeValue(Owner.self, ownerJSON("app:x", mem: 1, overrides: ["history": []]))
        expect(Detail.cards(bare, history: [], catalog: Advice.catalog, language: .zh).isEmpty, "no data, no card")
        let session = try require(report.owners.first { $0.kind == .agentSession && $0.agent?.nameSource == "derived" })
        let cards = Detail.cards(session, history: session.history, catalog: Advice.catalog, language: .zh)
        guard case .session(let card) = try require(cards.first) else { return fail("session card first") }
        let fields = Dictionary(uniqueKeysWithValues: card.fields.map { ($0.label, $0.value) })
        expect(fields["会话名"] == "7-74（自动命名）", fields["会话名"] ?? "")
        expect(fields["状态"] == "正在干活")
        expect(fields["空闲"] == "40 秒")
        expect(fields["位置"] == "tmux %15 · main (claude)")
        expect(fields["项目"]?.hasSuffix("atlas · fix/orphan-chain") == true)
        expect(card.prompt != nil && card.reply != nil && card.confidence == "确定")
        expect(cards.contains(.curve) && cards.contains { if case .advice = $0 { true } else { false } })
        // Since R10 the evidence is a sentence in the process relation card (Phase10Round4Tests).
        guard case .relation(let relation) = try require(cards.first(where: { if case .relation = $0 { true } else { false } }))
        else { return fail("relation card") }
        // R25: the row's launch label closes the sentence.
        expect(relation.how == "这些进程都是这个 Claude Code 会话启动的。启动位置：Ghostty · tmux main:1.0。", relation.how)
        expect(Words.status("made_up_status", .zh) == "made_up_status", "unknown codes stay visible")
        expect(Words.confidence(.weak, .en) == "Weak" && Words.status("waiting", .zh) == "等你回复")
    }

    static func systemServiceCardIsDisplayOnly() throws {
        let catalog = Advice.catalog
        let id = try require(catalog.values.first { $0.system?.command != nil }?.id)
        let owner = try decodeValue(Owner.self, ownerJSON("system:1", mem: 1, rule: id, kind: "system"))
        let cards = Detail.cards(owner, history: [], catalog: catalog, language: .zh)
        guard case .system(let card) = try require(cards.first) else { return fail("system card") }
        expect(card.command?.hasPrefix("sudo launchctl kickstart -k system/") == true)
        expect(card.permissions.contains("管理员") && card.launchd.contains("launchd") && !card.sideEffects.isEmpty)
        let actions = Detail.actions(owner)
        expect(actions.closeDisabled, "system processes: close is greyed")
        let plain = try decodeValue(Owner.self, ownerJSON("app:y", mem: 1, rule: "default"))
        expect(!Detail.cards(plain, history: [], catalog: catalog, language: .zh).contains { if case .system = $0 { true } else { false } })
        let source = String(decoding: try repoFile("app/Sources/Whoate/DetailView.swift"), as: UTF8.self)
        let systemCard = try require(source.components(separatedBy: "private func systemCard").last?
            .components(separatedBy: "private func relationCard").first)
        // R6 (owner 2026-10-21 14:3x) removed the copy button: the command is selectable text, no button.
        expect(systemCard.contains("Text(command)") && systemCard.contains(".textSelection(.enabled)"), "command text stays")
        expect(!systemCard.contains("Button("), "the system card has no button")
        for runs in ["Process(", "NSWorkspace", "NSAppleScript", "openApplication"] {
            expect(!systemCard.contains(runs), "the system card never runs anything: \(runs)")
        }
    }

    static func overviewNamesWiredZonesAndGpuReason() throws {
        var host = try require(try designReport().host)
        var overview = Detail.overview(host, language: .zh)
        expect(overview.memory.contains { $0.0 == "其中联动" && $0.1 == "5.0G（是已用的一部分）" }, "R21: part of physical memory used")
        expect(overview.zones.count == 5 && overview.zonesTitle == "内核内存区（前 5）")
        expect(overview.gpu == "2.1G" && !overview.diagnostics.isEmpty)
        host.kernelZones?.status = .stale
        host.gpuMemory = GpuMemory(status: .missing, asOf: nil, bytes: nil, source: nil, reason: "no_ioreg")
        overview = Detail.overview(host, language: .zh)
        expect(overview.zonesTitle == "内核内存区（前 5） · 不是最新")
        expect(overview.gpu == "读不到：no_ioreg")
    }

    // QA10 QA13
    static func detailsShowEverythingThePanelRowShows() throws {
        let report = try designReport()
        let panel = rows(report, limit: Panel.rowLimit)
        let main = rows(report)
        for row in panel {
            let same = try require(main.first { $0.id == row.id })
            expect(same.pill == row.pill && same.pillLevel == row.pillLevel, row.id)
            guard let owner = report.owners.first(where: { $0.id == row.id }) else { continue }
            let header = Detail.header(owner, row: same, language: .zh)
            expect(header.title == row.title && header.subtitle == row.subtitle && header.pill == row.pill)
            var advice: [Advice.Item] = []
            for case .advice(let items) in Detail.cards(owner, history: [], catalog: Advice.catalog, language: .zh) {
                advice = items
            }
            expect(advice.map(\.id) == owner.insights, "every suggestion of the pill is in the advice card")
        }
        let items = Advice.items(["brave.extensions", "no.such.id"], catalog: Advice.catalog, language: .en)
        expect(items[0].cause != nil && items[0].advice != nil && items[0].source?.hasPrefix("http") == true)
        expect(items[1] == Advice.Item(id: "no.such.id"), "unknown ids stay bare")
    }

    // QA12
    static func toolbarUsesThePanelHeader() throws {
        let source = String(decoding: try repoFile("app/Sources/Whoate/MainWindowView.swift"), as: UTF8.self)
        expect(source.contains("Panel.header(model.report?.host, sort: model.sort, language)"))
        let host = try require(try designReport().host)
        let cpu = Panel.header(host, sort: .cpu, .zh)
        expect(cpu.headline.contains("整机 CPU") && cpu.detail?.contains("1 分钟负载 6.20 / 12 核") == true)
    }

    // QA14
    static func tmuxJumpValidatesAndChoosesTheLatestClient() {
        expect(TmuxJump.validPane("%15") == "%15")
        for bad in ["15", "%", "%1;rm", "%1 2", "%١", "main:1.0", ""] { expect(TmuxJump.validPane(bad) == nil, bad) }
        expect(TmuxJump.commands(pane: "%15", tty: "/dev/ttys003") == [
            ["switch-client", "-c", "/dev/ttys003", "-t", "%15"], ["select-window", "-t", "%15"], ["select-pane", "-t", "%15"],
        ])
        let clients = TmuxJump.clients("/dev/ttys001 501 100\n/dev/ttys003 777 300\nbad line\n/dev/ttys009_9_9\n")
        expect(clients.count == 2 && TmuxJump.choose(clients)?.tty == "/dev/ttys003")
        expect(TmuxJump.choose([]) == nil)
        let parents: [Int32: Int32] = [777: 700, 700: 650, 650: 1]
        expect(TmuxJump.terminal(from: 777, parent: { parents[$0] }, isApp: { $0 == 650 }) == 650)
        expect(TmuxJump.terminal(from: 777, parent: { parents[$0] }, isApp: { _ in false }) == nil)
        // The runner receives argument arrays only; no shell, nothing run for a bad pane.
        final class Calls: @unchecked Sendable { var all: [[String]] = [] }
        let calls = Calls()
        let none = TmuxJump.jump(pane: "%1;rm", run: { calls.all.append($0); return (0, "") })
        expect(none == .invalidPane && calls.all.isEmpty)
        let empty = TmuxJump.jump(pane: "%15", run: { calls.all.append($0); return (0, "") })
        expect(empty == .noClient && calls.all == [TmuxJump.listClients])
        calls.all = []
        let done = TmuxJump.jump(pane: "%15", run: { args in
            calls.all.append(args)
            return (0, args == TmuxJump.listClients ? "/dev/ttys003 777 300\n" : "")
        }, parent: { parents[$0] }, isApp: { $0 == 650 })
        expect(done == .jumped(terminal: 650) && calls.all.count == 4)
        let source = String(decoding: try! repoFile("app/Sources/WhoateKit/TmuxJump.swift"), as: UTF8.self)
        expect(!source.contains("/bin/sh") && !source.contains("\"-c\", \""), "never through a shell")
    }

    // QA15
    @MainActor static func menuBarImageIsAlwaysATemplate() throws {
        let logo = NSImage(size: NSSize(width: 18, height: 18))
        for style in MenuBarItem.AlertStyle.allCases {
            for pressure in ["normal", "warn", "critical", "unknown"] {
                let host = try decodeValue(Host.self, ["mem_total_bytes": 1, "mem_used_bytes": 1, "compressed_bytes": 0,
                                                       "wired_bytes": 0, "swap_used_bytes": 0, "pressure": pressure])
                for h in [host, nil] {
                    let label = MenuBarItem.label(h, style: style, language: .zh)
                    expect(MenuBarItem.image(label.image, logo: logo).isTemplate, "\(style) \(pressure)")
                    expect(MenuBarItem.image(label.image, logo: nil).isTemplate)
                }
            }
        }
        let app = String(decoding: try repoFile("app/Sources/Whoate/WhoateApp.swift"), as: UTF8.self)
        expect(app.contains("style: .systemSymbol"), "owner chose style D")
        for file in ["Whoate/WhoateApp.swift", "Whoate/PanelView.swift", "Whoate/Palette.swift", "WhoateKit/MenuBarItem.swift"] {
            let text = String(decoding: try repoFile("app/Sources/" + file), as: UTF8.self)
            expect(!text.contains("isTemplate = false") && !text.contains("render(text:"), file)
        }
    }

    // QA16 QA17
    @MainActor static func openingTheMainWindowPutsThePanelAway() throws {
        var order: [String] = []
        PanelActions.showMain("orphan:1", open: { order.append("open \($0 ?? "-")") }, dismiss: { order.append("dismiss") })
        expect(order == ["open orphan:1", "dismiss"])
        let app = String(decoding: try repoFile("app/Sources/Whoate/WhoateApp.swift"), as: UTF8.self)
        expect(app.contains("openMain: { PanelActions.showMain($0, open: open, dismiss: delegate.dismissPanel) }"))
        let panel = String(decoding: try repoFile("app/Sources/Whoate/PanelView.swift"), as: UTF8.self)
        let footer = try require(panel.components(separatedBy: "private var footer: some View").last?
            .components(separatedBy: "private var footerText").first)
        let quit = try require(footer.range(of: "L10n.text(.quit, language)")), show = try require(footer.range(of: "openLabel"))
        expect(quit.lowerBound < show.lowerBound, "quit at the far left, the main-window button at the far right")
        expect(!footer.contains(".defaultAction") && !footer.contains("borderedProminent"), "quit is never the default button")
        expect(footer.contains(".buttonStyle(.borderless)") && footer.contains(".buttonStyle(.bordered)"), "styles differ")
    }

    /// Review A:B3 / CONFORM:B2: any number of same-named rows, and a suffix that
    /// collides with another row's own title, still end pairwise distinct.
    static func titlesStayUniqueForThreeOrMoreAndSuffixCollisions() throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("orphan:11", mem: 5, kind: "orphan", overrides: ["label": "node"]),
            ownerJSON("orphan:22", mem: 4, kind: "orphan", overrides: ["label": "node"]),
            ownerJSON("orphan:33", mem: 3, kind: "orphan", overrides: ["label": "node"]),
            ownerJSON("orphan:44", mem: 2, kind: "orphan", overrides: ["label": "node · 孤儿进程"]),
            ownerJSON("service:a", mem: 1, kind: "service", overrides: ["label": "node"]),
        ]))
        let titles = rows(report).map(\.title)
        expect(Set(titles).count == titles.count, "pairwise distinct: \(titles)")
        expect(titles.contains("node · 孤儿进程 · 33") || titles.contains("node · 孤儿进程 · 22"), "pid tail for the third: \(titles)")
    }

    /// CONFORM:B3: "All" counts owners only; a process group overlays them.
    static func allTotalCountsOwnersOnce() throws {
        let report = try designReport()
        // R23 (round 10): footprints, owners only (a process group never adds to it).
        let owners = Format.memory(report.owners.reduce(0) { $0 + $1.memBytes },
                                   unknown: report.owners.reduce(0) { $0 + $1.unknownMemCount }, .zh)
        // Owner 2026-10-28: "All" shows no number; the kinds still add owners only.
        expect(MainWindowModel.total(rows(report), .all, .zh).text.isEmpty, "no total for All")
        expect(!owners.text.isEmpty)
        expect(MainWindowModel.total(rows(report), .needsAction, .zh).text == "4", "filters still count rows")
    }

    /// CONFORM:B4: a session outside tmux keeps a greyed jump button; A should: late results stay with their row.
    @MainActor static func jumpButtonIsGreyedOutsideTmuxAndResultsStayWithTheirRow() throws {
        let report = try designReport()
        var opencode = try require(report.owners.first { $0.kind == .agentSession && $0.agent?.tool == "opencode" })
        let inTmux = Detail.actions(opencode)
        expect(inTmux.jumpShown && inTmux.pane == "%7")
        opencode.origin?.tmuxPane = nil
        let outside = Detail.actions(opencode)
        expect(outside.jumpShown && outside.pane == nil, "shown, greyed")
        let app = try require(report.owners.first { $0.kind == .app })
        expect(!Detail.actions(app).jumpShown, "apps have no jump button")
        let model = AppModel(language: .zh, makeEngine: { nil })
        model.selectedOwnerID = "a"
        model.selectedOwnerID = "b"
        model.finishJump("no client", for: "a")
        expect(model.notice == nil, "A's late result does not show under B")
        model.finishJump("no client", for: "b")
        expect(model.notice == "no client")
    }

    /// B nit: a CPU curve does not bridge a long stretch of points without CPU.
    static func cpuCurveBreaksWhereCpuIsMissing() {
        var pts = Phase10Tests.points([0, 20, 40, 60, 80, 100, 120, 140])
        for i in pts.indices { pts[i].cpuPct = (2...5).contains(i) ? nil : 10 }
        // 0, 20 | 120, 140: the latest run with CPU is 120–140, 20 s, too short.
        expect(Format.trend(pts, width: 10, height: 10, metric: .cpu) == .insufficient)
        // Review A round 2: CPU missing at the end of a continuous run never shows the old part as current.
        var tail = Phase10Tests.points([0, 20, 40, 60, 80, 100, 120])
        for i in tail.indices { tail[i].cpuPct = i < 3 ? Double(10 * (i + 1)) : nil }
        expect(Format.trend(tail, width: 10, height: 10, metric: .cpu) == .insufficient, "last CPU at 40 s, newest sample 120 s")
        // QA34: both mini curves draw the metric the list is sorted by.
        for file in ["PanelView.swift", "MainWindowView.swift"] {
            let text = String(decoding: (try? repoFile("app/Sources/Whoate/" + file)) ?? Data(), as: UTF8.self)
            expect(text.contains("language: language, metric: sort"), "\(file) passes the sort metric to its curve")
        }
    }

    /// CONFORM:B6: rows scrolled into view get histories, beyond the first 20.
    @MainActor static func scrolledIntoViewRowsGetHistories() async throws {
        let owners = (1...30).map { i in ownerJSON("o\(i)", mem: UInt64(100 - i), cpu: Double(100 - i), pids: [Int32(i)]) }
        let report = try decodeValue(Report.self, reportJSON(owners: owners))
        let spy = Phase08Tests.HistorySpy(report, raw: [])
        let model = AppModel(language: .en, makeEngine: { spy }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             sleep: { _ in try await Task.sleep(for: .milliseconds(5)) })
        model.setMainRow("o27", visible: true)
        model.setVisible(main: true)
        model.start()
        for _ in 0..<200 where model.report == nil { try? await Task.sleep(for: .milliseconds(5)) }
        model.stop()
        let asked = Set(spy.requested)
        expect(asked.contains("o27"), "the visible 27th row is fetched")
        expect(!asked.contains("o28"), "rows not on screen past the first 20 are not")
    }

    /// With the main window open, its first rows get full histories too (design round 1, finding 2).
    @MainActor static func mainWindowRowsGetHistories() async throws {
        let owners = (1...8).map { i in ownerJSON("o\(i)", mem: UInt64(100 - i), cpu: Double(100 - i), pids: [Int32(i)]) }
        let report = try decodeValue(Report.self, reportJSON(owners: owners))
        for main in [false, true] {
            let spy = Phase08Tests.HistorySpy(report, raw: [])
            let model = AppModel(language: .en, makeEngine: { spy }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                                 sleep: { _ in try await Task.sleep(for: .milliseconds(5)) })
            model.setVisible(panel: !main, main: main)
            model.start()
            for _ in 0..<200 where model.report == nil { try? await Task.sleep(for: .milliseconds(5)) }
            model.stop()
            expect(Set(spy.requested).count == (main ? 8 : 5), "main=\(main) asked \(spy.requested)")
        }
    }
}
