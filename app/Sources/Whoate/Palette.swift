// Colors from docs/specs/mockups (light) with dark counterparts.
import AppKit
import SwiftUI
import WhoateKit

enum Palette {
    static func dynamic(_ light: NSColor, _ dark: NSColor) -> Color {
        Color(nsColor: NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua ? dark : light
        })
    }
    static func hex(_ value: UInt32, alpha: CGFloat = 1) -> NSColor {
        NSColor(srgbRed: CGFloat(value >> 16 & 0xff) / 255, green: CGFloat(value >> 8 & 0xff) / 255,
                blue: CGFloat(value & 0xff) / 255, alpha: alpha)
    }

    static let act = dynamic(hex(0xd9622b), hex(0xff8a50))
    static let actBackground = dynamic(hex(0xfbeee6), hex(0xd9622b, alpha: 0.22))
    static let watch = dynamic(hex(0xb8860b), hex(0xe0b03c))
    static let watchBackground = dynamic(hex(0xfbf4dc), hex(0xb8860b, alpha: 0.22))
    static let okBackground = dynamic(hex(0xececf0), hex(0xffffff, alpha: 0.10))
    static let used = dynamic(hex(0x8e8e93), hex(0x8e8e93))
    static let compressed = dynamic(hex(0xc7c7cc), hex(0x5a5a5f))
    static let track = dynamic(hex(0xe5e5ea), hex(0x3a3a3c))
    static let line = dynamic(hex(0xdcdce0), hex(0x3a3a3e))
    static let flat = dynamic(hex(0xa1a1a6), hex(0x8e8e93))

    static func tint(_ level: Level) -> Color {
        switch level {
        case .act: act
        case .watch: watch
        default: .secondary
        }
    }
    static func background(_ level: Level) -> Color {
        switch level {
        case .act: actBackground
        case .watch: watchBackground
        default: okBackground
        }
    }
    static func pressure(_ pressure: Pressure?) -> NSColor? {
        switch pressure {
        case .warn: hex(0xb8860b)
        case .critical: hex(0xd9622b)
        default: nil
        }
    }
}
