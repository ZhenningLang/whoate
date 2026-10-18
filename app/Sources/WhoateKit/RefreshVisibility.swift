// Observe the actual containing NSWindow, not application activation. The same
// adapter serves MenuBarExtra, the main window and the opt-in screenshot host.
import AppKit
import SwiftUI

public struct RefreshVisibility: NSViewRepresentable {
    private let changed: @MainActor (Bool) -> Void
    public init(changed: @escaping @MainActor (Bool) -> Void) { self.changed = changed }
    public func makeNSView(context: Context) -> VisibilityView { VisibilityView(changed: changed) }
    public func updateNSView(_ view: VisibilityView, context: Context) {
        Task { @MainActor in view.publish() }
    }
    public static func dismantleNSView(_ view: VisibilityView, coordinator: ()) { view.disconnect() }

    @MainActor public final class VisibilityView: NSView {
        private let changed: @MainActor (Bool) -> Void
        private var tokens: [NSObjectProtocol] = []
        private var visible: NSKeyValueObservation?
        private var last: Bool?
        private var generation: UInt64 = 0
        public init(changed: @escaping @MainActor (Bool) -> Void) {
            self.changed = changed
            super.init(frame: .zero)
        }
        required init?(coder: NSCoder) { fatalError("init(coder:) unavailable") }
        public func disconnect() {
            removeObservers()
            send(false)
        }
        private func removeObservers() {
            generation &+= 1
            tokens.forEach { NotificationCenter.default.removeObserver($0) }
            tokens = []
            visible = nil
        }
        nonisolated private func enqueue(_ generation: UInt64, closing: Bool = false) {
            Task { @MainActor [weak self] in
                guard let self, self.generation == generation else { return }
                if closing { self.send(false) } else { self.publish() }
            }
        }
        public override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            removeObservers()
            let current = generation
            if let window {
                visible = window.observe(\.isVisible, options: [.initial, .new]) { [weak self] _, _ in
                    self?.enqueue(current)
                }
                for name in [NSWindow.didChangeOcclusionStateNotification, NSWindow.didMiniaturizeNotification,
                             NSWindow.didDeminiaturizeNotification, NSWindow.didBecomeKeyNotification] {
                    tokens.append(NotificationCenter.default.addObserver(forName: name, object: window, queue: .main) { [weak self] _ in
                        self?.enqueue(current)
                    })
                }
                tokens.append(NotificationCenter.default.addObserver(forName: NSWindow.willCloseNotification, object: window, queue: .main) { [weak self] _ in
                    self?.enqueue(current, closing: true)
                })
            }
            publish()
        }
        public func publish() { send(window.map { $0.isVisible && !$0.isMiniaturized } ?? false) }
        private func send(_ value: Bool) {
            guard value != last else { return }
            last = value
            changed(value)
        }
    }
}
