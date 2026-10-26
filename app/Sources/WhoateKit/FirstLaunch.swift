// First launch (Phase 11, 第一印象): open the main window once and point at the
// menu bar item. Automation never runs it unless WHOATE_UI_FIRST_LAUNCH=1, so
// earlier phases' checks (fresh wrapper bundles every run) are unaffected.
import AppKit
import Foundation

public enum FirstLaunch {
    @MainActor public static func shouldRun(store: any SettingsStore, automation: Bool, environment: [String: String]) -> Bool {
        guard store.bool(.firstLaunchDone) != true else { return false }
        return !automation || environment["WHOATE_UI_FIRST_LAUNCH"] == "1"
    }

    /// Recorded before anything is shown, so a crash cannot repeat it.
    @MainActor public static func markDone(_ store: any SettingsStore) { store.set(true, for: .firstLaunchDone) }

    /// The status item's own window when it is on a screen and not under the notch
    /// (research F11); nil means the hint goes into the main window instead.
    @MainActor public static func statusItemWindow(_ windows: [NSWindow] = NSApp.windows) -> NSWindow? {
        windows.first { window in
            guard String(describing: type(of: window)).contains("NSStatusBarWindow"), window.isVisible,
                  let screen = window.screen else { return false }
            return visible(window.frame, screen: screen.frame,
                           beside: [screen.auxiliaryTopLeftArea, screen.auxiliaryTopRightArea].compactMap { $0 })
        }
    }

    /// Laid out, on the screen, and (with a notch) wholly left or right of it. The
    /// auxiliary areas are the menu bar's visible parts; measured, the item's window
    /// stands a point taller than them, so only the horizontal span is compared.
    public static func visible(_ frame: NSRect, screen: NSRect, beside notch: [NSRect]) -> Bool {
        guard frame.width > 0, frame.height > 0, screen.intersects(frame) else { return false }
        return notch.isEmpty || notch.contains { $0.minX <= frame.minX && frame.maxX <= $0.maxX }
    }
}
