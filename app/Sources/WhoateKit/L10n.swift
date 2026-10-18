// Interface strings in Chinese and English. The language is resolved in the
// same order as whoate-core's Language::system() so the engine's diagnosis
// text and the interface never mix languages.
import Foundation

public enum Language: Sendable, Equatable {
    case zh, en

    public static func from(locale: String) -> Language {
        locale.trimmingCharacters(in: .whitespaces).lowercased().hasPrefix("zh") ? .zh : .en
    }

    /// LC_ALL → LC_MESSAGES → global AppleLanguages → LANG (default "en").
    public static func resolve(environment: [String: String], appleLanguages: [String]?) -> Language {
        for key in ["LC_ALL", "LC_MESSAGES"] {
            if let value = environment[key], !value.isEmpty { return from(locale: value) }
        }
        if let first = appleLanguages?.first(where: { !$0.isEmpty }) { return from(locale: first) }
        return from(locale: environment["LANG"] ?? "en")
    }

    /// Reads the global domain like `defaults read -g AppleLanguages`.
    public static func system() -> Language {
        let global = UserDefaults.standard.persistentDomain(forName: UserDefaults.globalDomain)
        return resolve(
            environment: ProcessInfo.processInfo.environment,
            appleLanguages: global?["AppleLanguages"] as? [String]
        )
    }
}

public enum L10nKey: String, CaseIterable, Sendable {
    case pressureNormal, pressureWarn, pressureCritical, pressureUnknown
    case used, compressed, swap, sortMemory, sortCPU
    case refreshEvery, systemProcsAgo, systemProcsMissing, openWhoate, quit
    case details, close, normal, systemProcess, systemService
    case considerClosing, possibleLeak, growing, idleFor, possibleRunaway, sharedAcross, watch, eachAbout, adviceOne
    case adviceCount, processCount, paneClosed, ports, copies
    case engineFailed, engineRetrying, loading, noOwners
    case confirmTitle, confirmPIDs, estimateLabel, estimateUnknown, estimateBasis
    case confirmButton, cancel, protectedHint, terminating
    case resultTitle, terminated, stillAlive, refused, missing, unknownStatus
    case observedChange, notMeasured, observedBasis
    case stillAliveQuestion, forceButton, done, killFailed, forcing, refreshFailed
    case reasonProtectedPid, reasonNotInSnapshot, reasonProtectedOrUnknown, reasonProcessMissing
    case reasonIdentityChanged, reasonPlatformBinary, reasonPlatformProbeFailed, reasonNoConfirmedPlan
    case mainWindowTitle, mainWindowPending
}

public enum L10n {
    static let table: [L10nKey: (zh: String, en: String)] = [
        .pressureNormal: ("内存充足", "Memory OK"),
        .pressureWarn: ("内存偏紧", "Memory tight"),
        .pressureCritical: ("内存紧张", "Memory critical"),
        .pressureUnknown: ("内存状态未知", "Memory state unknown"),
        .used: ("已用", "Used"),
        .compressed: ("压缩", "Compressed"),
        .swap: ("swap", "swap"),
        .sortMemory: ("内存", "Memory"),
        .sortCPU: ("CPU", "CPU"),
        .refreshEvery: ("每 %@ 秒刷新", "Refreshes every %@ s"),
        .systemProcsAgo: ("系统进程 %@ 秒前", "system processes %@ s ago"),
        .systemProcsMissing: ("系统进程暂缺", "system processes unavailable"),
        .openWhoate: ("打开 whoate", "Open whoate"),
        .quit: ("退出", "Quit"),
        .details: ("详情", "Details"),
        .close: ("关闭", "Close"),
        .normal: ("正常", "Normal"),
        .systemProcess: ("系统进程", "System process"),
        .systemService: ("系统服务", "System service"),
        .considerClosing: ("建议关闭", "Consider closing"),
        .possibleLeak: ("疑似泄漏", "Possible leak"),
        .growing: ("还在涨", "Still growing"),
        .idleFor: ("空闲 %@", "Idle %@"),
        .possibleRunaway: ("疑似失控", "Possible runaway"),
        .sharedAcross: ("关掉闲置归属可一起释放", "Freed with idle owners"),
        .watch: ("留意", "Watch"),
        .eachAbout: ("单个约 %@", "~%@ each"),
        .adviceOne: ("有 1 条建议", "1 suggestion"),
        .adviceCount: ("有 %@ 条建议", "%@ suggestions"),
        .processCount: ("%@ 个进程", "%@ processes"),
        .paneClosed: ("启动它的 pane 已关闭", "launching pane closed"),
        .ports: ("端口 %@", "port %@"),
        .copies: ("%@ 份", "%@ copies"),
        .engineFailed: ("whoate 引擎启动失败", "whoate engine failed to start"),
        .engineRetrying: ("下个刷新周期重试", "Retrying next refresh"),
        .loading: ("正在采样…", "Sampling…"),
        .noOwners: ("没有可显示的归属", "Nothing to show"),
        .confirmTitle: ("关闭「%@」？", "Close “%@”?"),
        .confirmPIDs: ("将向这些进程发送结束信号：", "These processes will be asked to quit:"),
        .estimateLabel: ("预计腾出物理内存", "Estimated physical memory freed"),
        .estimateUnknown: ("无法预估（%@ 个进程测不到）", "Cannot estimate (%@ unmeasured)"),
        .estimateBasis: ("按驻留内存粗估，含共享页，不保证", "RSS estimate incl. shared pages; not guaranteed"),
        .confirmButton: ("结束进程", "Quit processes"),
        .cancel: ("取消", "Cancel"),
        .protectedHint: ("系统进程，勿关", "System process; do not terminate"),
        .terminating: ("已发送结束信号，最多等 5 秒…", "Signal sent; waiting up to 5 s…"),
        .resultTitle: ("关闭结果", "Result"),
        .terminated: ("已结束", "terminated"),
        .stillAlive: ("仍未退出", "still running"),
        .refused: ("已拒绝", "refused"),
        .missing: ("已不存在", "not found"),
        .unknownStatus: ("未知", "unknown"),
        .observedChange: ("实际内存变化", "Observed memory change"),
        .notMeasured: ("未测到", "not measured"),
        .observedBasis: ("关闭前后整机已用内存之差，其他软件也会影响", "Host used before minus after; other software affects it"),
        .stillAliveQuestion: ("%@ 个进程仍未退出，强制结束？", "%@ processes are still running. Force quit?"),
        .forceButton: ("强制结束", "Force quit"),
        .done: ("完成", "Done"),
        .killFailed: ("关闭失败：%@", "Close failed: %@"),
        .forcing: ("已发送强制结束信号，正在确认…", "Force quit sent; checking…"),
        .refreshFailed: ("刷新失败：%@，显示的是上次结果", "refresh failed: %@; showing the last result"),
        .reasonProtectedPid: ("受保护的进程", "protected process"),
        .reasonNotInSnapshot: ("不在本次采样里", "not in the latest sample"),
        .reasonProtectedOrUnknown: ("身份不明或受保护", "unknown or protected identity"),
        .reasonProcessMissing: ("进程已退出", "process already exited"),
        .reasonIdentityChanged: ("pid 已换成别的进程", "pid now belongs to another process"),
        .reasonPlatformBinary: ("macOS 自带程序", "macOS system binary"),
        .reasonPlatformProbeFailed: ("无法确认是否为系统程序", "could not check for a system binary"),
        .reasonNoConfirmedPlan: ("不在已确认的关闭列表里", "not in the confirmed close list"),
        .mainWindowTitle: ("whoate", "whoate"),
        .mainWindowPending: ("三栏主窗口在下一期提供，当前列出全部归属。", "The three-column window arrives next; all owners are listed for now."),
    ]

    public static func text(_ key: L10nKey, _ language: Language) -> String {
        guard let pair = table[key] else { return key.rawValue }
        return language == .zh ? pair.zh : pair.en
    }
    public static func format(_ key: L10nKey, _ language: Language, _ args: String...) -> String {
        var result = text(key, language)
        for arg in args {
            guard let range = result.range(of: "%@") else { break }
            result.replaceSubrange(range, with: arg)
        }
        return result
    }
}
