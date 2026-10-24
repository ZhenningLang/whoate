// Plain words for the engine's internal codes in details: agent status and
// confidence (design round 1, coder finding 3). An unknown code is shown as it
// is, never hidden. Evidence kinds are never shown since R10; the process
// relation card says how in a sentence, and the raw evidence is only copied.
import Foundation

public enum Words {
    static let status: [String: (zh: String, en: String)] = [
        "working": ("正在干活", "Working"),
        "idle": ("空闲", "Idle"),
        "waiting": ("等你回复", "Waiting for you"),
        "running": ("在跑命令", "Running a command"),
        "done": ("已完成", "Done"),
        "archived": ("已归档", "Archived"),
    ]

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
