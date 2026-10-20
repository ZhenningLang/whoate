// System colors and symbols by meaning (app/DESIGN.md §3, §8). No colour of
// our own: every value here is a macOS system colour that adapts to light,
// dark and increased contrast, and every level also has a symbol or word.
import SwiftUI
import WhoateKit

enum Palette {
    /// act = system orange, watch = system yellow, everything else secondary label.
    static func tint(_ level: Level) -> Color {
        switch level {
        case .act: .orange
        case .watch: .yellow
        default: .secondary
        }
    }

    /// Second signal next to the colour; nil for ok rows.
    static func symbol(_ row: PanelRow) -> String? {
        switch row.pillLevel {
        case .act: "exclamationmark.triangle.fill"
        case .watch where row.level == .ok: "lightbulb.fill"
        case .watch: "exclamationmark.circle.fill"
        default: nil
        }
    }

    /// Memory bar: used and compressed in system gray, swap in system yellow.
    static let used = Color(nsColor: .systemGray)
    static let compressed = Color(nsColor: .systemGray).opacity(0.45)
    static let swap = Color.yellow
}
