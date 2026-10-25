// Menu bar item: always a template image plus system-font text in the menu bar
// foreground (app/DESIGN.md §6). The alert style is still the owner's choice
// (design gate: D, a system symbol); the gallery renders every candidate.
import AppKit
import Foundation

public enum MenuBarItem {
    public enum AlertStyle: String, CaseIterable, Sendable {
        /// A: the logo gives way to a warning symbol; outline = tight, filled = critical.
        case symbol
        /// B: "!" / "‼" in front of the number.
        case prefix
        /// C: the pressure word after the number.
        case word
        /// D: alerts as in A. Until R20 it also showed a system symbol when normal (the old
        /// logo read as "…." at 18 pt); since the pie logo (owner 2026-10-25) every style shows it.
        case systemSymbol
    }

    public enum Image: Equatable, Sendable {
        case logo
        case symbol(String)
    }

    public struct Label: Equatable, Sendable {
        public var image: Image
        public var text: String
    }

    /// The menu bar image: always a template, so macOS colours it for the bar.
    /// `logo` is the bundled template PDF (nil without one: a symbol stands in).
    @MainActor public static func image(_ image: Image, logo: NSImage?) -> NSImage {
        let made: NSImage
        switch image {
        case .logo:
            made = (logo?.copy() as? NSImage) ?? NSImage(systemSymbolName: "memorychip", accessibilityDescription: "whoate")!
            made.size = NSSize(width: 18, height: 18)
        case .symbol(let name):
            made = NSImage(systemSymbolName: name, accessibilityDescription: "whoate")
                ?? NSImage(systemSymbolName: "memorychip", accessibilityDescription: "whoate")!
        }
        made.isTemplate = true
        return made
    }

    public static func label(_ host: Host?, style: AlertStyle, language: Language) -> Label {
        let used = host.map { Format.bytes($0.memUsedBytes) } ?? "—"
        let pressure = host?.pressure ?? .unknown
        guard pressure == .warn || pressure == .critical else {
            // R20: the monochrome bitten-pie template, the number after it as before.
            return Label(image: .logo, text: used)
        }
        let critical = pressure == .critical
        switch style {
        case .symbol, .systemSymbol:
            return Label(image: .symbol(critical ? "exclamationmark.triangle.fill" : "exclamationmark.triangle"), text: used)
        case .prefix:
            return Label(image: .logo, text: (critical ? "‼ " : "! ") + used)
        case .word:
            return Label(image: .logo, text: used + " " + L10n.text(critical ? .menuCritical : .menuTight, language))
        }
    }
}
