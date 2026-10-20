// Plain words for the engine's internal codes in details: evidence kinds,
// agent status and confidence (design round 1, coder finding 3). An unknown
// code is shown as it is, never hidden.
import Foundation

public enum Words {
    static let evidence: [String: (zh: String, en: String)] = [
        "pane_ancestor": ("在 tmux pane 里运行", "Runs inside a tmux pane"),
        "agent_source": ("助手的会话记录里有它", "Listed in the agent's session record"),
        "tmux_environment": ("环境变量 TMUX_PANE 指向这个 pane", "TMUX_PANE points at this pane"),
        "runtime_home": ("开发运行时，工作目录在用户目录下", "A dev runtime working in your home folder"),
        "app_bundle": ("可执行文件在这个应用包里", "Its executable is inside this app"),
        "responsible_app": ("系统记录这个应用为它负责", "macOS lists this app as responsible for it"),
        "responsible_launcher": ("由它启动", "Started by it"),
        "responsible_terminal": ("从终端启动，终端窗口已不在", "Started from a terminal"),
        "responsible_unavailable": ("查不到负责它的应用", "No responsible app found"),
        "app_metadata": ("应用信息读取失败", "Could not read the app's info"),
        "other_uid": ("属于其他用户", "Belongs to another user"),
        "service_fallback": ("归不到具体软件，按后台服务处理", "No owner found; treated as a background service"),
        "pane_status_unknown": ("这次没读到 tmux pane 列表", "tmux pane list unavailable this time"),
    ]
    static let status: [String: (zh: String, en: String)] = [
        "working": ("正在干活", "Working"),
        "idle": ("空闲", "Idle"),
        "waiting": ("等你回复", "Waiting for you"),
        "running": ("在跑命令", "Running a command"),
        "done": ("已完成", "Done"),
    ]

    public static func evidence(_ kind: String, _ language: Language) -> String {
        evidence[kind].map { language == .zh ? $0.zh : $0.en } ?? kind
    }
    public static func status(_ code: String, _ language: Language) -> String {
        status[code].map { language == .zh ? $0.zh : $0.en } ?? code
    }
    public static func confidence(_ c: Confidence, _ language: Language) -> String {
        switch (c, language) {
        case (.certain, .zh): "确定"
        case (.certain, .en): "Certain"
        case (.inferred, .zh): "推断"
        case (.inferred, .en): "Inferred"
        case (.weak, .zh): "较弱"
        case (.weak, .en): "Weak"
        case (.unknown, .zh): "未知"
        case (.unknown, .en): "Unknown"
        }
    }
}
