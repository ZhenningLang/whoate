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
    case mainWindowTitle, mainWindowPending, showMain
    case cpuHeadline, cpuMissing, cpuLoad, cpuIdle, cpuModerate, cpuBusy
    case cpuReasonFirst, cpuReasonReset, cpuReasonRead, cpuReasonNoTicks, cpuReasonAbsent
    case notEnoughSamples, adviceCause, adviceAdvice, adviceSource
    case kindApp, kindOrphan, kindService, kindSystem, ownedBy, sessionsOf
    case categoryAll, categoryAgents, categoryApps, categoryOrphans, categorySystem, filterNeedsAction, filterGrowing
    case sectionOwners, sectionFilters, ownersCount, sortedBy, last30Minutes, unknownMem, unknownCPU, menuTight, menuCritical
    case kindPane, overview, memoryUsed, wired, kernelZones, gpuMemory, diagnostics, notFresh
    case session, sessionName, autoNamed, conversation, status, location, project, confidence
    case relation, curve30, curveCPU30, parts, advice, processesInSession, jumpToPane, copyInfo, copied
    case notInTmux, closeProcesses, systemDoNotClose, swarmOwners, noSelection
    case noSession, idle, unavailable, gpuMissing, noTerminalApp, noTmuxClient, jumpFailed
    case systemCard, restartNotRun, facts, linuxNote
    case confirmSummary, confirmSummaryUnknown
    case paneSessions, paneSessionsLine, andMore, codexSession, byFolder, other
    case relationHow, relationGuess, relationRest, relationStarter, relationMarked, relationPane, relationPaneOnly, relationWindow
    case relationPaneClosed, relationApp, relationSession, relationMCP, relationExited, copyEvidence
    case howOwnApp, howSession, howCodex, howPane, howLauncher, howNoSession, howPaneClosed, howOrphan
    case howResponsible, howOtherUser, howService, howDefault
    case copyHeader, copyOwner, copySession, copyKind, copyKindAgent, copyMemory, copyUnknown, copyCwd
    case copyTmux, copyWindow, copyReason, copySuggestion, copyProcesses, copyMore, copyNoArgv
    case lastActive
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
        // R1 (owner 2026-10-21): one word for the main window in both states.
        .openWhoate: ("完整面板", "Full Panel"),
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
        .showMain: ("完整面板", "Full Panel"),
        .cpuHeadline: ("整机 CPU %@", "CPU %@"),
        .cpuMissing: ("整机 CPU 暂缺：%@", "CPU unavailable: %@"),
        .cpuLoad: ("1 分钟负载 %@ / %@ 核", "1-min load %@ / %@ cores"),
        .cpuIdle: ("空闲", "Idle"),
        .cpuModerate: ("偏忙", "Fairly busy"),
        .cpuBusy: ("繁忙", "Busy"),
        .cpuReasonFirst: ("刚开始采样", "first sample"),
        .cpuReasonReset: ("计数器回退", "counter reset"),
        .cpuReasonRead: ("读取失败", "read failed"),
        .cpuReasonNoTicks: ("采样间隔太短", "interval too short"),
        .cpuReasonAbsent: ("引擎未提供", "not reported"),
        .notEnoughSamples: ("采样不足", "Not enough samples"),
        .adviceCause: ("原因：%@", "Cause: %@"),
        .adviceAdvice: ("建议：%@", "Advice: %@"),
        .adviceSource: ("出处：%@", "Source: %@"),
        .kindApp: ("桌面应用", "App"),
        .kindOrphan: ("孤儿进程", "Orphan"),
        .kindService: ("后台服务", "Background service"),
        .kindSystem: ("系统", "System"),
        .ownedBy: ("分属 %@ 个归属：%@", "across %@ owners: %@"),
        .sessionsOf: ("%@ 会话", "%@ sessions"),
        .categoryAll: ("全部", "All"),
        .categoryAgents: ("助手会话", "Agent sessions"),
        .categoryApps: ("应用", "Apps"),
        .categoryOrphans: ("孤儿", "Orphans"),
        .categorySystem: ("系统", "System"),
        .filterNeedsAction: ("需要处理", "Needs attention"),
        .filterGrowing: ("在涨", "Growing"),
        .sectionOwners: ("归属", "Owners"),
        .sectionFilters: ("筛选", "Filters"),
        .ownersCount: ("%@ 个归属", "%@ owners"),
        .sortedBy: ("按%@", "by %@"),
        .last30Minutes: ("最近 30 分钟", "Last 30 minutes"),
        .unknownMem: ("%@ 个进程读不到内存", "%@ processes unreadable"),
        .unknownCPU: ("%@ 个进程读不到 CPU", "CPU unreadable for %@ processes"),
        .menuTight: ("偏紧", "tight"),
        .menuCritical: ("紧张", "critical"),
        .kindPane: ("tmux pane", "tmux pane"),
        .overview: ("系统概览", "Overview"),
        .memoryUsed: ("已用", "Used"),
        .wired: ("联动内存", "Wired"),
        .kernelZones: ("内核内存区（前 5）", "Kernel zones (top 5)"),
        .gpuMemory: ("显存", "GPU memory"),
        .diagnostics: ("诊断", "Diagnostics"),
        .notFresh: ("不是最新", "not fresh"),
        .session: ("会话", "Session"),
        .sessionName: ("会话名", "Name"),
        .autoNamed: ("自动命名", "auto-named"),
        .conversation: ("对话", "Conversation"),
        .status: ("状态", "Status"),
        .location: ("位置", "Location"),
        .project: ("项目", "Project"),
        .confidence: ("可信度", "Confidence"),
        .relation: ("进程关系", "Process tree"),
        .curve30: ("内存 · 最近 30 分钟", "Memory · last 30 minutes"),
        .curveCPU30: ("CPU · 最近 30 分钟", "CPU · last 30 minutes"),
        .parts: ("组成", "Parts"),
        .advice: ("建议", "Suggestions"),
        .processesInSession: ("这个会话下的进程", "Processes in this session"),
        .jumpToPane: ("跳到这个 pane", "Go to pane"),
        .copyInfo: ("复制相关信息", "Copy Details"),
        .copied: ("已复制", "Copied"),
        .notInTmux: ("不在 tmux 里", "Not in tmux"),
        .closeProcesses: ("关闭 %@ 个进程…", "Close %@ processes…"),
        .systemDoNotClose: ("系统进程，勿关", "System process; do not close"),
        .swarmOwners: ("分布", "Distribution"),
        .noSelection: ("在中间列表选一行查看详情", "Select a row to see details"),
        .noSession: ("无会话", "no session"),
        .idle: ("空闲", "Idle"),
        .unavailable: ("暂缺", "Unavailable"),
        .gpuMissing: ("读不到：%@", "Unavailable: %@"),
        .noTerminalApp: ("已切到 pane，未找到终端应用", "Switched the pane; no terminal app found"),
        .noTmuxClient: ("没有连着的 tmux 客户端，先在终端里 attach", "No attached tmux client; attach in a terminal first"),
        .jumpFailed: ("跳转失败：%@", "Jump failed: %@"),
        .systemCard: ("怎么重启（whoate 不会执行）", "How to restart (whoate never runs it)"),
        .restartNotRun: ("没有可用的重启命令", "No restart command"),
        .facts: ("实测数据", "Measured facts"),
        .linuxNote: ("Docker 数字按 Linux 口径统计", "Docker figures use Linux accounting"),
        .confirmSummary: ("将结束 %@ 个进程，预计腾出 %@", "Ends %@ processes, frees about %@"),
        .confirmSummaryUnknown: ("将结束 %@ 个进程，预计腾出：无法预估", "Ends %@ processes; memory freed: cannot estimate"),
        .paneSessions: ("这个 pane 里的助手会话", "Agent sessions in this pane"),
        .paneSessionsLine: ("会话：%@", "Sessions: %@"),
        .andMore: ("等 %@ 个", "%@ in all"),
        .codexSession: ("Codex 会话", "Codex session"),
        .byFolder: ("按工作目录分组", "By working folder"),
        .other: ("其他", "Other"),
        // R10 (owner 2026-10-24): plain words only; no variable names, pane ids or bundle ids.
        .relationHow: ("怎么判断的：%@", "How whoate decided: %@"),
        .relationGuess: ("这是推测，可能不准。", "This is a guess and may be wrong."),
        .relationRest: ("其余 %@ 个进程 共 %@", "%@ more processes, %@ in all"),
        .relationStarter: ("启动它的进程", "The process that started it"),
        .relationMarked: ("← 就是它", "← this one"),
        .relationPane: ("tmux 分屏 %@", "tmux split %@"),
        .relationPaneOnly: ("tmux 分屏", "tmux split"),
        .relationWindow: ("（%@ 窗口）", " (%@ window)"),
        .relationPaneClosed: ("tmux 分屏（已关闭）", "tmux split (closed)"),
        .relationApp: ("%@（桌面应用）", "%@ (app)"),
        .relationSession: ("「%@」", "“%@”"),
        .relationMCP: ("外接小工具 %@", "MCP tool %@"),
        .relationExited: ("已退出", "exited"),
        .copyEvidence: ("原始证据", "Raw evidence"),
        .howOwnApp: ("%@ 自己的进程。", "%@'s own processes."),
        .howSession: ("这些进程都是这个 %@ 会话启动的。", "This %@ session started these processes."),
        .howCodex: ("Codex 给这些进程标记了这个会话。", "Codex marked these processes with this session."),
        .howPane: ("这些进程都是在这个分屏里启动的。", "These processes were all started in this split."),
        .howLauncher: ("启动它的进程已经退出，macOS 记录是 %@ 让它运行的。",
                       "The process that started it has exited; macOS records that %@ had it run."),
        .howNoSession: ("这些进程是 %@ 启动的，但没对上具体会话。", "%@ started these processes, but no session matched."),
        .howPaneClosed: ("它原来所在的分屏已经关闭。", " The split it ran in is closed."),
        .howOrphan: ("启动它的程序已经退出，macOS 收养了这些进程。",
                     "The program that started it has exited; macOS adopted these processes."),
        .howResponsible: ("macOS 记录是 %@ 让它运行的。", "macOS records that %@ had it run."),
        .howOtherUser: ("属于系统账户的进程。", "Processes of a system account."),
        .howService: ("macOS 的后台服务，归不到具体软件。", "A macOS background service that belongs to no app."),
        .howDefault: ("按进程的父子关系归到这里。", "Grouped here by parent and child processes."),
        .copyHeader: ("whoate 归属信息 · %@ · 帮我判断保留还是关闭。不含对话内容。",
                      "whoate owner info · %@ · Help me decide whether to keep or close it. No conversation content."),
        .copyOwner: ("软件/会话", "Software/session"),
        .copySession: ("%@ 会话「%@」", "%@ session “%@”"),
        .copyKind: ("种类", "Kind"),
        .copyKindAgent: ("助手会话（%@）", "Agent session (%@)"),
        .copyMemory: ("内存", "Memory"),
        .copyUnknown: ("（%@ 个进程读不到内存，只计读得到的）", " (%@ processes unreadable; counted the rest)"),
        .copyCwd: ("工作目录", "Working folder"),
        .copyTmux: ("tmux", "tmux"),
        .copyWindow: ("窗口「%@」", "window “%@”"),
        .copyReason: ("归属理由", "Why grouped here"),
        .copySuggestion: ("建议", "Suggestion"),
        .copyProcesses: ("进程（%@ 个）", "Processes (%@)"),
        .copyMore: ("另有 %@ 个未列出", "%@ more not listed"),
        .copyNoArgv: ("命令行不可用", "Command lines unavailable"),
        .lastActive: ("最近活动", "Last active"),
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
