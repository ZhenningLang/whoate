// Phase 11 settings window. Names carry the QA id (qa<N>_…) for verify.sh.
// Nothing here touches UserDefaults.standard, a real login item, the real
// notification centre or a real PATH directory: suites and directories live
// under WHOATE_CHECKS_TMP (verify.sh's scratch) or the per-user temp folder.
import AppKit
import Foundation
@testable import WhoateKit

enum Phase11Tests {
    typealias R3 = Phase10Round3Tests
    static func scratch(_ name: String) throws -> URL {
        let base = ProcessInfo.processInfo.environment["WHOATE_CHECKS_TMP"] ?? NSTemporaryDirectory()
        let dir = URL(fileURLWithPath: base).appendingPathComponent("phase11-\(name)-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    // MARK: QA4 store and isolation

    @MainActor static func qa4_eachKeyReadsBackFromTheSameSuiteAndBadValuesFallBack() throws {
        let dir = try scratch("qa4")
        defer { try? FileManager.default.removeItem(at: dir) }
        let suite = dir.appendingPathComponent("suite").path
        let first = AppSettings(store: UserDefaultsStore(try require(UserDefaults(suiteName: suite))))
        expect(first.appearance == .system && first.language == .system && first.menuBarDisplay == .iconAndNumber
            && first.refreshProfile == .standard && !first.notificationsEnabled && first.notifyPressure && first.notifyGrowing,
            "defaults")
        first.appearance = .dark
        first.language = .en
        first.menuBarDisplay = .iconOnly
        first.refreshProfile = .powerSaving
        first.notificationsEnabled = true
        first.notifyPressure = false
        first.notifyGrowing = false
        FirstLaunch.markDone(first.store)
        let again = AppSettings(store: UserDefaultsStore(try require(UserDefaults(suiteName: suite))))
        expect(again.appearance == .dark && again.language == .en && again.menuBarDisplay == .iconOnly
            && again.refreshProfile == .powerSaving && again.notificationsEnabled && !again.notifyPressure && !again.notifyGrowing,
            "a new store on the same suite reads every key back")
        expect(again.store.bool(.firstLaunchDone) == true)
        // Unknown values read as defaults, never a crash.
        let odd = MemoryStore()
        odd.set("purple", for: .appearance)
        odd.set("fr", for: .language)
        odd.set("tiny", for: .menuBarDisplay)
        odd.set("turbo", for: .refreshProfile)
        odd.set("yes", for: .notificationsEnabled)
        let bad = AppSettings(store: odd)
        expect(bad.appearance == .system && bad.language == .system && bad.menuBarDisplay == .iconAndNumber
            && bad.refreshProfile == .standard && !bad.notificationsEnabled)
        // The four environments (research F6): automation never gets the real domain.
        let choose = SettingsStores.choose(environment:)
        expect(choose([:]).kind == .standard)
        expect(choose(["WHOATE_UI_DEFAULTS": suite]).kind == .standard, "without automation the variable is ignored")
        expect(choose(["WHOATE_UI_AUTOMATION": "menubar", "WHOATE_UI_DEFAULTS": suite]) == (.suite, suite, false))
        expect(choose(["WHOATE_UI_AUTOMATION": "menubar", "WHOATE_UI_DEFAULTS": "relative/suite"]) == (.memory, nil, true),
               "relative path: memory, reported as defaults_suite_rejected")
        expect(choose(["WHOATE_UI_AUTOMATION": "menubar"]) == (.memory, nil, false))
        expect(SettingsStores.make(environment: ["WHOATE_UI_AUTOMATION": "menubar"]).kind == .memory)
        // WHOATE_UI_SET goes through the same setters.
        let applied = AppSettings(store: MemoryStore())
        var keys: [SettingsKey] = []
        applied.changed = { keys.append($0) }
        applied.apply("appearance=dark,language=en,menuBarDisplay=iconOnly,refreshProfile=powerSaving,bogus=1,appearance=neon")
        expect(applied.appearance == .dark && applied.language == .en && applied.menuBarDisplay == .iconOnly
            && applied.refreshProfile == .powerSaving)
        expect(keys == [.appearance, .language, .menuBarDisplay, .refreshProfile], "\(keys)")
    }

    @MainActor static func qa20_firstLaunchRunsOnceAndNeverUnderPlainAutomation() {
        let store = MemoryStore()
        expect(FirstLaunch.shouldRun(store: store, automation: false, environment: [:]))
        expect(!FirstLaunch.shouldRun(store: store, automation: true, environment: [:]), "earlier phases' runs stay as they were")
        expect(FirstLaunch.shouldRun(store: store, automation: true, environment: ["WHOATE_UI_FIRST_LAUNCH": "1"]))
        FirstLaunch.markDone(store)
        expect(!FirstLaunch.shouldRun(store: store, automation: false, environment: [:]))
        // Where the hint can point (measured on a notched screen: aux 848,950,663,32; the item 872,949,64,33).
        let screen = NSRect(x: 0, y: 0, width: 1512, height: 982)
        let right = NSRect(x: 848, y: 950, width: 663, height: 32), left = NSRect(x: 0, y: 950, width: 663, height: 32)
        expect(FirstLaunch.visible(NSRect(x: 872, y: 949, width: 64, height: 33), screen: screen, beside: [left, right]))
        expect(!FirstLaunch.visible(NSRect(x: 700, y: 949, width: 64, height: 33), screen: screen, beside: [left, right]), "under the notch")
        expect(!FirstLaunch.visible(NSRect(x: 0, y: 0, width: 41, height: 0), screen: screen, beside: []), "not laid out yet")
        expect(!FirstLaunch.visible(NSRect(x: 0, y: -33, width: 64, height: 33), screen: screen, beside: []), "off screen")
        expect(FirstLaunch.visible(NSRect(x: 1300, y: 958, width: 64, height: 24), screen: screen, beside: []), "no notch")
        expect(!FirstLaunch.shouldRun(store: store, automation: true, environment: ["WHOATE_UI_FIRST_LAUNCH": "1"]))
    }

    // MARK: QA5 appearance

    @MainActor static func qa5_appearanceFollowsTheSettingUnlessAutomationSetsOne() {
        let name = AppearanceApplier.appearanceName(setting:automation:)
        expect(name(.system, nil) == nil && name(.light, nil) == .aqua && name(.dark, nil) == .darkAqua)
        for setting in AppearanceSetting.allCases {
            expect(name(setting, .light) == .aqua && name(setting, .dark) == .darkAqua, "automation wins over \(setting)")
        }
        let settings = AppSettings(store: MemoryStore())
        var calls: [SettingsKey] = []
        settings.changed = { calls.append($0) }
        settings.appearance = .dark
        expect(calls == [.appearance], "applied once per change: \(calls)")
    }

    // MARK: QA6 language

    final class LanguageSpy: RefreshEngine, @unchecked Sendable {
        private let lock = NSLock()
        private var languages: [Language] = []
        let report: Report
        init(_ report: Report) { self.report = report }
        func tick() -> Decoded<Report> { .value(report) }
        func setActive(_ active: Bool) -> Decoded<RefreshControl> { .value(RefreshControl(active: active)) }
        func setLanguage(_ language: Language) -> Decoded<LanguageControl> {
            lock.withLock { languages.append(language) }
            return .value(LanguageControl(language: language == .zh ? "zh" : "en"))
        }
        var told: [Language] { lock.withLock { languages } }
    }

    @MainActor static func qa6_languageChangesTheTextAtOnceAndTellsTheEngineOnce() async throws {
        let report = try decodeValue(Report.self, reportJSON(owners: [
            ownerJSON("app:grow", mem: 3 << 30, level: "watch", rule: "fast-growth"),
        ]))
        let spy = LanguageSpy(report)
        let model = AppModel(language: .zh, makeEngine: { spy }, sleep: { try await Task.sleep(for: .seconds($0 / 100)) })
        model.setVisible(panel: true)
        model.start()
        defer { model.stop() }
        await RefreshTests.wait { model.report != nil && !spy.told.isEmpty }
        expect(spy.told == [.zh], "the engine is told the interface language before its first tick: \(spy.told)")
        let zhRow = try require(model.rows.first)
        let zhCategory = MainCategory.growing.title(model.language)
        model.language = .en
        await RefreshTests.wait { spy.told.count == 2 }
        expect(spy.told == [.zh, .en], "one call, resolved to en: \(spy.told)")
        let enRow = try require(model.rows.first)
        expect(zhRow.pill != enRow.pill && enRow.pill == L10n.text(.growing, .en), "\(zhRow.pill) → \(enRow.pill)")
        expect(MainCategory.growing.title(model.language) == L10n.text(.filterGrowing, .en) && zhCategory != MainCategory.growing.title(.en))
        expect(Panel.footer(nil, lastError: nil, interval: 2, language: model.language) == "Refreshes every 2 s")
        let tight = try decodeValue(WhoateKit.Host.self, ["mem_total_bytes": 8, "mem_used_bytes": 4, "compressed_bytes": 0, "wired_bytes": 0,
                                                "swap_used_bytes": 0, "pressure": "warn"])
        expect(MenuBarItem.label(tight, style: .word, language: model.language).text.hasSuffix(L10n.text(.menuTight, .en)))
        // "跟随系统" resolves first; the engine never hears "system".
        let settings = AppSettings(store: MemoryStore())
        expect(settings.resolvedLanguage(system: { .zh }) == .zh && settings.resolvedLanguage(system: { .en }) == .en)
        settings.language = .en
        expect(settings.resolvedLanguage(system: { .zh }) == .en)
        // The app menu item: its title from L10n, ⌘, as its shortcut.
        expect(L10n.text(.settingsMenu, .zh) == "设置…" && L10n.text(.settingsMenu, .en) == "Settings…")
        let app = try R3.slice(try R3.source("WhoateApp.swift"), from: "CommandGroup(replacing: .appSettings)", to: "\n            }\n")
        expect(app.contains("L10n.text(.settingsMenu, delegate.model.language)") && app.contains(".keyboardShortcut(\",\")"), app)
    }

    /// Review C:B1: only the settings window open (hidden cadence), a full sample 15 s ago.
    /// The change reaches the engine on the next sample, not at the next 30 s full tick.
    @MainActor static func qa6_hiddenLanguageChangeReachesTheEngineAtOnce() async throws {
        typealias R = Phase08Round3Tests
        let clock = R.Clock(), trace = R.Trace()
        let spy = LanguageSpy(try decodeValue(Report.self, reportJSON(owners: [ownerJSON("o", mem: 5)])))
        let model = AppModel(language: .zh, makeEngine: { spy }, sampleHost: { .failure(FFIError(code: 1, message: "x")) },
                             trace: { trace.append(clock.now, $0) }, now: { clock.now }, sleep: { try await clock.sleep($0) })
        clock.hold(at: 12)
        model.start()
        defer { model.stop() }
        await RefreshTests.wait { clock.now >= 12 && trace.entries.last?.0 == 10 }
        expect(spy.told == [.zh] && trace.entries.filter { $0.1 == "hidden-full" }.map(\.0) == [0], "\(trace.entries)")
        clock.hold(at: 17)
        model.language = .en
        await RefreshTests.wait { spy.told.count == 2 }
        expect(spy.told == [.zh, .en], "\(spy.told)")
        expect(trace.entries.filter { $0.1 == "hidden-full" }.map(\.0) == [0, 15], "a paused full tick right away: \(trace.entries)")
    }

    // MARK: QA7 menu bar display

    @MainActor static func qa7_iconOnlyDropsTheTextAndKeepsTheImage() throws {
        func host(_ pressure: String) throws -> WhoateKit.Host {
            try decodeValue(WhoateKit.Host.self, ["mem_total_bytes": 48 << 30, "mem_used_bytes": 41 << 30, "compressed_bytes": 0,
                                        "wired_bytes": 0, "swap_used_bytes": 0, "pressure": pressure])
        }
        let (normal, warn, critical) = (try host("normal"), try host("warn"), try host("critical"))
        let hosts: [WhoateKit.Host?] = [normal, warn, critical, nil]
        for h in hosts {
            for style in MenuBarItem.AlertStyle.allCases {
                let old = MenuBarItem.label(h, style: style, language: .zh)
                expect(MenuBarItem.label(h, style: style, display: .iconAndNumber, language: .zh) == old, "default unchanged")
                let icon = MenuBarItem.label(h, style: style, display: .iconOnly, language: .zh)
                expect(icon.text.isEmpty && icon.image == old.image, "\(style) \(String(describing: h?.pressure))")
                expect(MenuBarItem.image(icon.image, logo: nil).isTemplate)
            }
            let spoken = MenuBarItem.accessibilityLabel(h, language: .en)
            expect(spoken.contains(h.map { Format.bytes($0.memUsedBytes) } ?? "—"), spoken)
        }
        expect(MenuBarItem.label(warn, style: .systemSymbol, display: .iconOnly, language: .zh).image
            == .symbol("exclamationmark.triangle"))
        expect(MenuBarItem.label(critical, style: .systemSymbol, display: .iconOnly, language: .zh).image
            == .symbol("exclamationmark.triangle.fill"))
        expect(MenuBarItem.label(normal, style: .systemSymbol, display: .iconOnly, language: .zh).image == .logo)
    }

    // MARK: QA8 refresh profiles

    final class SleepLog: @unchecked Sendable {
        private let lock = NSLock()
        private var values: [Double] = []
        func add(_ v: Double) { lock.withLock { values.append(v) } }
        var all: [Double] { lock.withLock { values } }
    }

    @MainActor static func qa8_powerSavingDoublesTheCadenceFromTheNextSleep() async throws {
        let standard = RefreshProfile.standard, saving = RefreshProfile.powerSaving
        expect((standard.visible, standard.hidden, standard.hiddenFull, standard.gapLimit) == (2, 5, 30, 45))
        expect((saving.visible, saving.hidden, saving.hiddenFull, saving.gapLimit) == (4, 10, 60, 90))
        expect(AppModel.refreshInterval == 2 && AppModel.hiddenInterval == 5 && AppModel.hiddenFullInterval == 30
            && Format.gapLimit == 45, "the standard numbers did not change")
        let spy = try RefreshTests.Spy()
        let log = SleepLog()
        // Real sleeps: a switch must cut the current one short.
        let model = AppModel(language: .zh, makeEngine: { spy.make() }, sampleHost: { spy.host() },
                             sleep: { log.add($0); try await Task.sleep(for: .seconds($0)) })
        model.setVisible(panel: true)
        model.start()
        defer { model.stop() }
        await RefreshTests.wait { !log.all.isEmpty }
        let first = try require(log.all.first)
        expect(first > 1 && first <= 2, "standard visible: \(first)")
        let switched = ProcessInfo.processInfo.systemUptime
        model.profile = .powerSaving
        await RefreshTests.wait { log.all.count >= 2 }
        let waited = ProcessInfo.processInfo.systemUptime - switched
        let next = try require(log.all.dropFirst().first)
        expect(next > 3 && next <= 4, "power saving visible: \(next)")
        expect(waited < 1, "the 2 s sleep did not have to finish: \(waited)")
        expect(Panel.footer(nil, lastError: nil, interval: model.profile.visible, language: .zh) == "每 4 秒刷新")
        // Hidden points 60 s apart: one run when power saving, broken apart at the standard limit.
        let points = Phase08Tests.points([0, 60, 120, 180])
        expect(Format.runs(points, gapLimit: saving.gapLimit).count == 1)
        expect(Format.runs(points).count == 4)
        guard case .line = Format.trend(points, width: 10, height: 10, gapLimit: saving.gapLimit) else {
            return fail("power saving joins 60 s points")
        }
        expect(Format.trend(points, width: 10, height: 10) == .insufficient)
    }

    // MARK: QA9 open at login

    @MainActor static func qa9_openAtLoginShowsTheSystemsStatus() throws {
        let app = "/Applications/whoate.app"
        func show(_ status: LoginItemStatus, path: String = app, error: String? = nil, _ language: Language = .zh) -> LoginItemPresentation {
            .make(status: status, bundlePath: path, error: error, language: language)
        }
        expect(show(.enabled) == LoginItemPresentation(isOn: true, canToggle: true, message: "已开启", showsSystemSettingsButton: false))
        expect(show(.notRegistered).message == "已关闭" && !show(.notRegistered).isOn)
        expect(show(.notFound) == show(.notRegistered), "never registered reads as off, not an error")
        let approval = show(.requiresApproval)
        expect(approval.message == "需要你在系统设置里批准" && approval.showsSystemSettingsButton)
        let failed = show(.notRegistered, error: "Operation not permitted")
        expect(!failed.isOn && failed.message == "已关闭\n没能更改开机启动：Operation not permitted", failed.message ?? "")
        let moved = show(.enabled, path: "/Users/someone/Downloads/whoate.app")
        expect(moved.canToggle && moved.message?.contains(L10n.text(.loginMoveNote, .zh)) == true)
        let translocated = show(.notFound, path: "/private/var/folders/x/AppTranslocation/ABC/d/whoate.app")
        expect(!translocated.canToggle && translocated.message == L10n.text(.loginTranslocated, .zh))
        let bare = show(.notFound, path: "/Users/someone/whoate/app/.build/debug")
        expect(!bare.canToggle && bare.message == L10n.text(.loginOnlyApp, .zh))
        // The toggle registers once, unregisters once; a failure leaves the system's status.
        let fake = FakeLoginItem()
        let model = SettingsModel(settings: AppSettings(store: MemoryStore()), login: fake, notifier: FakeNotifier(),
                                  bundlePath: app, info: nil, cli: .init(bundled: nil, candidates: [], pathDirs: []))
        model.setLogin(true)
        expect(fake.calls == ["register"] && model.loginPresentation(.zh).isOn)
        model.setLogin(false)
        expect(fake.calls == ["register", "unregister"] && !model.loginPresentation(.zh).isOn)
        fake.fails = true
        model.setLogin(true)
        expect(!model.loginPresentation(.zh).isOn && model.loginError == FakeLoginItem.Failure().errorDescription,
               "the switch goes back to what the system says")
        fake.status = .requiresApproval
        model.reload()
        expect(model.loginPresentation(.zh).showsSystemSettingsButton, "re-read when the window comes back")
        model.login.openSystemSettings()
        expect(fake.calls.last == "openSystemSettings")
        // Automation's fake from WHOATE_UI_LOGIN_FAKE.
        expect(FakeLoginItem(environment: "enabled").status == .enabled && FakeLoginItem(environment: "approval").status == .requiresApproval)
        expect(FakeLoginItem(environment: nil).status == .notFound && FakeLoginItem(environment: "error").fails)
    }

    // MARK: QA10 notifications

    static func row(_ id: String, rule: String, title: String? = nil, mem: UInt64 = 2 << 30) -> PanelRow {
        PanelRow(id: id, source: .owner, title: title ?? id, memBytes: mem, cpuPct: nil, subtitle: "/Users/x/secret --token abc",
                 level: .watch, pill: "", pillLevel: .watch, history: [], closable: false, rule: rule)
    }

    static func host(_ pressure: String) throws -> WhoateKit.Host {
        try decodeValue(WhoateKit.Host.self, ["mem_total_bytes": 48 << 30, "mem_used_bytes": 41 << 30, "compressed_bytes": 0,
                                    "wired_bytes": 0, "swap_used_bytes": 0, "pressure": pressure])
    }

    @MainActor static func qa10_notificationsAskOnceThrottleAndOpenTheirRow() async throws {
        // 1–2: permission only when switched on; refused turns it back off.
        let notifier = FakeNotifier()
        let settings = AppSettings(store: MemoryStore())
        let model = SettingsModel(settings: settings, login: FakeLoginItem(), notifier: notifier, bundlePath: "/Applications/whoate.app",
                                  info: nil, cli: .init(bundled: nil, candidates: [], pathDirs: []))
        expect(notifier.requests == 0)
        await model.setNotifications(false)
        expect(notifier.requests == 0, "off asks nothing")
        await model.setNotifications(true)
        expect(notifier.requests == 1 && settings.notificationsEnabled && !model.notificationsDenied)
        await model.setNotifications(true)
        expect(notifier.requests == 1, "already granted: not asked again")
        let refusing = FakeNotifier(grant: .denied)
        let denied = SettingsModel(settings: AppSettings(store: MemoryStore()), login: FakeLoginItem(), notifier: refusing,
                                   bundlePath: "/Applications/whoate.app", info: nil, cli: .init(bundled: nil, candidates: [], pathDirs: []))
        await denied.setNotifications(true)
        expect(!denied.settings.notificationsEnabled && denied.notificationsDenied && refusing.requests == 1)
        expect(NotificationSwitch.settingsURL(bundleID: "io.github.zhenninglang.whoate")?.absoluteString
            == "x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=io.github.zhenninglang.whoate")

        // 3: pressure, D4 = a (each level its own object), 30 minutes per object.
        var policy = NotificationPolicy(enabled: true)
        let minute = 60.0
        let (normal, warn, critical) = (try host("normal"), try host("warn"), try host("critical"))
        expect(policy.host(normal, at: 0, language: .zh) == nil)
        expect(policy.host(warn, at: 1, language: .zh)?.title == "内存偏紧")
        expect(policy.host(warn, at: 2, language: .zh) == nil, "still tight: no repeat")
        expect(policy.host(normal, at: 3, language: .zh) == nil)
        expect(policy.host(warn, at: 29 * minute, language: .zh) == nil, "within 30 minutes")
        expect(policy.host(critical, at: 29 * minute + 1, language: .zh)?.title == "内存紧张", "tight → critical")
        expect(policy.host(normal, at: 30 * minute, language: .zh) == nil)
        expect(policy.host(warn, at: 31 * minute + 1, language: .zh)?.object == "warn", "after 31 minutes")

        // 4: rows entering the growing filter.
        var rows = NotificationPolicy(enabled: true)
        let a = row("app:a", rule: "fast-growth"), b = row("app:b", rule: "leak-suspect"), calm = row("app:a", rule: "default")
        expect(rows.rows([a], at: 0, language: .zh).map(\.ownerID) == ["app:a"])
        expect(rows.rows([a], at: 10, language: .zh).isEmpty, "still growing: not again")
        expect(rows.rows([calm], at: 20, language: .zh).isEmpty)
        expect(rows.rows([a], at: 29 * minute, language: .zh).isEmpty, "re-entered within 30 minutes")
        expect(rows.rows([a, b], at: 29 * minute + 1, language: .zh).map(\.ownerID) == ["app:b"], "another row still notifies")
        expect(rows.rows([calm], at: 30 * minute, language: .zh).isEmpty)
        expect(rows.rows([a], at: 31 * minute, language: .zh).map(\.ownerID) == ["app:a"])

        // 5: a kind switched off, or everything off, sends nothing.
        var noGrowing = NotificationPolicy(enabled: true, pressure: true, growing: false)
        expect(noGrowing.rows([a], at: 0, language: .zh).isEmpty && noGrowing.host(warn, at: 0, language: .zh) != nil)
        var noPressure = NotificationPolicy(enabled: true, pressure: false, growing: true)
        expect(noPressure.host(critical, at: 0, language: .zh) == nil && noPressure.rows([a], at: 0, language: .zh).count == 1)
        var off = NotificationPolicy(enabled: false)
        expect(off.host(critical, at: 0, language: .zh) == nil && off.rows([a], at: 0, language: .zh).isEmpty)

        // 6: the text names the row and its memory only.
        var text = NotificationPolicy(enabled: true)
        let pathy = row("app:p", rule: "fast-growth", title: "node /Users/someone/project/server.js")
        let notes = text.rows([pathy], at: 0, language: .en) + [text.host(critical, at: 0, language: .en)].compactMap { $0 }
        expect(notes.count == 2)
        expect(NotificationPolicy.name("Claude --token SECRET") == "Claude")
        expect(NotificationPolicy.name("deploy API_KEY=abc tail") == "deploy")
        expect(NotificationPolicy.name("see https://example.invalid/x/run") == "see")
        expect(NotificationPolicy.name("psql postgres://admin:hunter2@db") == "psql", "no password from a URL")
        expect(NotificationPolicy.name("curl u:p@host") == "curl")
        expect(NotificationPolicy.name("run\t--token\tSECRET") == "run", "tabs split words too")
        expect(NotificationPolicy.name("--only-options") == "whoate", "never an empty title")
        for note in notes {
            let words = note.title + " " + note.body
            expect(!words.contains("/") && !words.contains("http") && !words.contains("--"), words)
        }

        // 7: a click opens the main window once, on its row when the row is still there.
        var opened: [String?] = []
        NotificationClick.open(ownerID: "app:a", rows: [a, b]) { opened.append($0) }
        NotificationClick.open(ownerID: "app:gone", rows: [a]) { opened.append($0) }
        NotificationClick.open(ownerID: nil, rows: [a]) { opened.append($0) }
        expect(opened == ["app:a", nil, nil], "\(opened)")
        var clicked: [String?] = []
        notifier.onClick = { clicked.append($0) }
        var fresh = NotificationPolicy(enabled: true)
        notifier.post(try require(fresh.rows([a], at: 0, language: .zh).first))
        notifier.click(latest: .growing)
        expect(clicked == ["app:a"])
    }

    // MARK: QA11 command line install

    static func executable(_ url: URL, _ script: String = "#!/bin/sh\necho 'whoate 9.9.9'\n") throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try script.write(to: url, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
    }

    /// The real places a link could land; none may change during the checks.
    static func realLinks() -> [String: String] {
        let fm = FileManager.default
        var out: [String: String] = [:]
        for path in ["/opt/homebrew/bin/whoate", NSHomeDirectory() + "/.local/bin/whoate", "/usr/local/bin/whoate"] {
            out[path] = (try? fm.destinationOfSymbolicLink(atPath: path)) ?? ((try? fm.attributesOfItem(atPath: path)) != nil ? "file" : "-")
        }
        return out
    }

    @MainActor static func qa11_installLinksIntoTheFirstWritableDirectoryOnPath() throws {
        let before = realLinks()
        let root = try scratch("qa11")
        let fm = FileManager.default
        let a = root.appendingPathComponent("a").path, b = root.appendingPathComponent("b").path
        let other = root.appendingPathComponent("other").path
        defer {
            for dir in [a, b] { try? fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir) }
            try? fm.removeItem(at: root)
            expect(realLinks() == before, "nothing outside the scratch directory changed")
        }
        for dir in [a, b, other] { try fm.createDirectory(atPath: dir, withIntermediateDirectories: true) }
        let bundled = root.appendingPathComponent("whoate.app/Contents/Helpers/whoate")
        try executable(bundled)
        let cli = bundled.path
        let version = { (_: String) in "whoate 9.9.9" as String? }

        // 1. first candidate writable and on PATH.
        expect(CommandLineTool.install(bundled: cli, candidates: [a, b], pathDirs: [a, b]) == .installed(path: a + "/whoate"))
        expect((try? fm.destinationOfSymbolicLink(atPath: a + "/whoate")) == cli)
        expect(CommandLineTool.detect(pathDirs: [a, b], bundled: cli, version: version)
            == .installed(path: a + "/whoate", source: .app, version: "whoate 9.9.9"))
        // Installing again replaces only our own link.
        expect(CommandLineTool.install(bundled: cli, candidates: [a, b], pathDirs: [a, b]) == .installed(path: a + "/whoate"))
        try fm.removeItem(atPath: a + "/whoate")

        // A candidate that is not on PATH is skipped.
        expect(CommandLineTool.install(bundled: cli, candidates: [a, b], pathDirs: [b]) == .installed(path: b + "/whoate"))
        try fm.removeItem(atPath: b + "/whoate")

        // 2. first not writable, second is.
        try fm.setAttributes([.posixPermissions: 0o555], ofItemAtPath: a)
        expect(CommandLineTool.install(bundled: cli, candidates: [a, b], pathDirs: [a, b]) == .installed(path: b + "/whoate"))
        try fm.removeItem(atPath: b + "/whoate")

        // 3. neither writable: nothing created, a command to copy.
        try fm.setAttributes([.posixPermissions: 0o555], ofItemAtPath: b)
        guard case .manual(let command) = CommandLineTool.install(bundled: cli, candidates: [a, b], pathDirs: [a, b]) else {
            return fail("expected a manual command")
        }
        expect(command.contains(cli) && command.contains("~/.local/bin") && !command.contains("sudo"), command)
        let (leftA, leftB) = (try fm.contentsOfDirectory(atPath: a), try fm.contentsOfDirectory(atPath: b))
        expect(leftA.isEmpty && leftB.isEmpty)
        for dir in [a, b] { try fm.setAttributes([.posixPermissions: 0o755], ofItemAtPath: dir) }

        // 4. a whoate already on PATH (Homebrew or another): installed, path, source, version.
        try executable(URL(fileURLWithPath: other + "/whoate"))
        expect(CommandLineTool.detect(pathDirs: [a, other], bundled: cli, version: version)
            == .installed(path: other + "/whoate", source: .other, version: "whoate 9.9.9"))
        expect(CommandLineTool.source("/opt/homebrew/Cellar/whoate/1.0.0/bin/whoate", bundled: cli) == .homebrew)
        expect(CommandLineTool.source("/Applications/Foo.app/Contents/Helpers/whoate", bundled: cli) == .other,
               "only whoate.app's helper is ours (review A, round 2)")

        // 5. a dangling link: broken, reinstall replaces it.
        try fm.createSymbolicLink(atPath: a + "/whoate", withDestinationPath: root.appendingPathComponent("moved/whoate.app/Contents/Helpers/whoate").path)
        expect(CommandLineTool.detect(pathDirs: [a], bundled: cli, version: version) == .broken(path: a + "/whoate"))
        expect(CommandLineTool.install(bundled: cli, candidates: [a], pathDirs: [a]) == .installed(path: a + "/whoate"))
        try fm.removeItem(atPath: a + "/whoate")

        // 5b. a dangling link that never was whoate.app's: left alone (review C:B2).
        let foreign = root.appendingPathComponent("another-tool/bin/whoate").path
        try fm.createSymbolicLink(atPath: a + "/whoate", withDestinationPath: foreign)
        expect(CommandLineTool.install(bundled: cli, candidates: [a], pathDirs: [a]) == .conflict(path: a + "/whoate"))
        expect((try? fm.destinationOfSymbolicLink(atPath: a + "/whoate")) == foreign, "the link is unchanged")
        try fm.removeItem(atPath: a + "/whoate")

        // 6. another file named whoate: left alone.
        try executable(URL(fileURLWithPath: a + "/whoate"), "#!/bin/sh\necho mine\n")
        expect(CommandLineTool.install(bundled: cli, candidates: [a], pathDirs: [a]) == .conflict(path: a + "/whoate"))
        expect((try? String(contentsOfFile: a + "/whoate", encoding: .utf8)) == "#!/bin/sh\necho mine\n")
        try fm.removeItem(atPath: a + "/whoate")

        // 7. no CLI in the app: no button (the tab shows the reason).
        expect(CommandLineTool.detect(pathDirs: [a], bundled: nil, version: version) == .noBundledCLI)
        expect(CommandLineTool.detect(pathDirs: [a], bundled: cli, version: version) == .notInstalled)
        expect(CommandLineTool.bundledCLI(bundle: .main) == nil, "the checks binary carries no CLI")

        // The user's PATH from a login shell: the marker line, past any login noise; never -i.
        let noisy = "Last login: today\nwelcome to zsh\n\n__WHOATE_PATH__/opt/homebrew/bin:/Users/x/.local/bin::/usr/bin\n"
        expect(CommandLineTool.parsePath(noisy) == ["/opt/homebrew/bin", "/Users/x/.local/bin", "/usr/bin"])
        expect(CommandLineTool.parsePath("no marker here") == nil)
        let args = CommandLineTool.shellArguments()
        expect(args.first == "-lc" && !args.contains { $0.hasPrefix("-") && $0.contains("i") }, "\(args)")
        expect(CommandLineTool.candidates(home: "/Users/x") == ["/opt/homebrew/bin", "/Users/x/.local/bin"])
    }

    // MARK: QA12 copy for AI

    @MainActor static func qa12_copyForAIIsTheBundledCLIsAgentGuide() async throws {
        let skill = try repoFile("skills/whoate/SKILL.md")
        let env = ProcessInfo.processInfo.environment
        let built = repoRoot.appendingPathComponent("target/debug/whoate").path
        let cli = env["WHOATE_CHECKS_CLI"] ?? built
        if env["WHOATE_CHECKS_CLI"] == nil, !FileManager.default.isExecutableFile(atPath: cli) {
            return skip("no built CLI (set WHOATE_CHECKS_CLI)")
        }
        print("    qa12 cli=\(cli)")
        let model = SettingsModel(settings: AppSettings(store: MemoryStore()), login: FakeLoginItem(), notifier: FakeNotifier(),
                                  bundlePath: "/Applications/whoate.app", info: nil, cli: .init(bundled: cli, candidates: [], pathDirs: []))
        expect(model.canCopyGuide)
        let guide = try require(await model.agentGuide())
        expect(Data(guide.utf8) == skill, "agent-guide bytes equal skills/whoate/SKILL.md")
        let none = SettingsModel(settings: AppSettings(store: MemoryStore()), login: FakeLoginItem(), notifier: FakeNotifier(),
                                 bundlePath: "/Applications/whoate.app", info: nil, cli: .init(bundled: nil, candidates: [], pathDirs: []))
        let noGuide = await none.agentGuide()
        expect(!none.canCopyGuide && noGuide == nil, "no CLI: the button is disabled")
        for language in [Language.zh, .en] {
            expect(L10n.text(.cliAgentUsage, language).contains("whoate top --json"))
        }
    }

    // MARK: QA13 about

    static func qa13_aboutShowsVersionLinksAndNoUpdateCheck() throws {
        let info = AboutInfo.make(info: ["CFBundleShortVersionString": "1.2.3", "CFBundleVersion": "1.2.3"], language: .zh)
        expect(info.version == "1.2.3" && info.build == "1.2.3")
        let dev = AboutInfo.make(info: nil, language: .zh)
        expect(dev.version == "开发版" && dev.build == "开发版" && dev.isDevelopment, "no Info.plist keys: development build, never empty")
        expect(!info.isDevelopment)
        expect(AboutInfo.make(info: [:], language: .en).version == "development build")
        expect(info.license.contains("MIT") && AboutInfo.make(info: nil, language: .en).license.contains("MIT"))
        expect(info.repository == "https://github.com/ZhenningLang/whoate" && info.issues == info.repository + "/issues")
        expect(info.updateLines.contains { $0.contains("brew upgrade whoate") }
            && info.updateLines.contains { $0.contains("brew upgrade --cask whoate") })
        // Owner 2026-10-27: no support entries yet, so no support area.
        expect(Links.support.isEmpty && info.support.isEmpty)
        // The four combinations with stand-in data (never shipped).
        let paypal = SupportItem(name: "Link EN", target: .link("https://example.invalid/en"), audience: .en)
        let wechat = SupportItem(name: "QR ZH", target: .qrCode(resource: "zh.png"), audience: .zh)
        let both = SupportItem(name: "Link all", target: .link("https://example.invalid/all"), audience: .all)
        let qrAll = SupportItem(name: "QR all", target: .qrCode(resource: "all.png"), audience: .all)
        let shown = { (items: [SupportItem], language: Language) in
            AboutInfo.make(info: nil, support: items, language: language).support.map(\.name)
        }
        expect(shown([paypal, both], .en) == ["Link EN", "Link all"], "links only")
        expect(shown([paypal, both], .zh) == ["Link all"], "an en link is not shown in Chinese")
        expect(shown([wechat, qrAll], .zh) == ["QR ZH", "QR all"], "QR codes only")
        expect(shown([wechat, qrAll], .en) == ["QR all"])
        expect(shown([paypal, wechat, both, qrAll], .zh) == ["QR ZH", "Link all", "QR all"], "both kinds")
        expect(shown([paypal, wechat, both, qrAll], .en) == ["Link EN", "Link all", "QR all"])
        expect(shown([], .zh).isEmpty && shown([], .en).isEmpty, "none")
        // A QR code is found where bundle-app.sh puts it; a missing file finds nothing.
        let dir = try scratch("qa13")
        defer { try? FileManager.default.removeItem(at: dir) }
        let app = dir.appendingPathComponent("Fake.app/Contents")
        let support = app.appendingPathComponent("Resources/Support")
        try FileManager.default.createDirectory(at: support, withIntermediateDirectories: true)
        try PropertyListSerialization.data(fromPropertyList: ["CFBundleIdentifier": "test.whoate.qa13"], format: .xml, options: 0)
            .write(to: app.appendingPathComponent("Info.plist"))
        try Data([0x89, 0x50, 0x4E, 0x47]).write(to: support.appendingPathComponent("zh.png"))
        let bundle = try require(Bundle(url: dir.appendingPathComponent("Fake.app")))
        expect(SupportItem.imageURL("zh.png", bundle: bundle)?.lastPathComponent == "zh.png")
        expect(SupportItem.imageURL("missing.png", bundle: bundle) == nil)
        expect(SupportItem.imageURL("zh.png") == nil, "the checks binary ships no support image")
        // whoate never checks for updates online (U-NG-7).
        for key in L10nKey.allCases {
            for language in [Language.zh, .en] {
                let text = L10n.text(key, language)
                expect(!text.contains("检查更新") && !text.lowercased().contains("check for update"), "\(key)")
            }
        }
    }

    // MARK: QA15 text

    static let phase11Keys: [L10nKey] = {
        let all = L10nKey.allCases
        guard let first = all.firstIndex(of: .settingsMenu) else { return [] }
        return Array(all[first...])
    }()

    static func qa15_newTextIsInBothLanguagesWithoutCJKInEnglish() {
        expect(phase11Keys.count >= 60, "\(phase11Keys.count) Phase 11 keys")
        let cjk = { (s: String) in s.unicodeScalars.contains { (0x3000...0x303F).contains($0.value) || (0x3400...0x9FFF).contains($0.value)
            || (0xFF00...0xFFEF).contains($0.value) } }
        for key in L10nKey.allCases {
            let zh = L10n.text(key, .zh), en = L10n.text(key, .en)
            expect(L10n.table[key] != nil && !zh.isEmpty && !en.isEmpty, "\(key) missing")
            expect(!cjk(en), "\(key) English has CJK: \(en)")
        }
        for tab in SettingsTab.allCases { expect(!L10n.text(tab.title, .en).isEmpty && !L10n.text(tab.title, .zh).isEmpty) }
    }
}
