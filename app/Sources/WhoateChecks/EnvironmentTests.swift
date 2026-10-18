// QA10 language/strings, QA12 PATH repair, automation entry parsing.
@testable import WhoateKit

enum EnvironmentTests {
    static func everyKeyHasBothLanguages() {
        for key in L10nKey.allCases {
            let pair = L10n.table[key]
            expect(pair != nil, "\(key) missing")
            expect(!(pair?.zh.isEmpty ?? true) && !(pair?.en.isEmpty ?? true), "\(key) empty")
        }
        expect(L10n.format(.adviceCount, .en, "3") == "3 suggestions")
    }

    static func languageOrderMatchesCore() {
        let apple = ["en-US"]
        expect(Language.resolve(environment: ["LC_ALL": "zh_CN.UTF-8", "LC_MESSAGES": "en_US", "LANG": "en"], appleLanguages: apple) == .zh)
        expect(Language.resolve(environment: ["LC_ALL": "", "LC_MESSAGES": "zh_CN", "LANG": "en"], appleLanguages: apple) == .zh)
        expect(Language.resolve(environment: ["LANG": "en_US"], appleLanguages: ["zh-Hans-CN", "en"]) == .zh)
        expect(Language.resolve(environment: ["LANG": "zh_CN"], appleLanguages: ["en-US"]) == .en)
        expect(Language.resolve(environment: ["LANG": "zh_CN"], appleLanguages: nil) == .zh)
        expect(Language.resolve(environment: [:], appleLanguages: []) == .en)
    }

    static func pathIsAppendedNeverReplaced() {
        expect(completedPath("/usr/bin:/bin:/usr/sbin:/sbin") == "/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:/usr/local/bin")
        expect(completedPath("/opt/homebrew/bin:/usr/bin") == "/opt/homebrew/bin:/usr/bin:/usr/local/bin")
        expect(completedPath("/x:/usr/local/bin:/opt/homebrew/bin") == "/x:/usr/local/bin:/opt/homebrew/bin")
        expect(completedPath(nil) == "/opt/homebrew/bin:/usr/local/bin")
    }

    static func automationIsInertUnlessEnabled() {
        expect(!Automation(environment: ["WHOATE_UI_APPEARANCE": "dark", "WHOATE_UI_CONFIRM": "x"]).isActive)
        expect(!Automation(environment: ["WHOATE_UI_AUTOMATION": ""]).isActive)
        let a = Automation(environment: [
            "WHOATE_UI_AUTOMATION": "panel-window, dump-tick", "WHOATE_UI_APPEARANCE": "dark",
            "WHOATE_UI_CONFIRM": "first", "WHOATE_UI_WINDOW_FILE": "/tmp/w",
        ])
        expect(a.panelWindow && a.dumpTick && !a.noPathRepair)
        expect(a.appearance == .dark && a.confirm == "first" && a.windowFile == "/tmp/w")
    }
}
