// Two-step close: confirm → SIGTERM (5 s) → result → optional force for the
// pids still alive. R16 (owner 2026-10-25): the main window shows it as a system
// sheet over the whole window, which is dimmed behind it (MainWindowView); a
// MenuBarExtra window cannot rely on sheets, so the panel shows the same card
// over the dimmed panel. Esc cancels; no button answers Return.
import SwiftUI
import WhoateKit

/// The panel's close: the same card in the middle of the dimmed panel.
struct CloseFlowView: View {
    @Bindable var model: AppModel

    var body: some View {
        if model.close != .idle && model.closeOrigin == .panel {
            ZStack {
                CloseDim()
                CloseCard(model: model)
                    .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
                    .overlay(RoundedRectangle(cornerRadius: 12).stroke(.separator))
                    .shadow(color: .black.opacity(0.3), radius: 18, y: 8)
                    .padding(14)
            }
        }
    }
}

/// What sits behind the close card: dark enough that the card stands apart in both
/// appearances (the owner missed the old 18 % veil), and it swallows clicks.
struct CloseDim: View {
    var body: some View {
        Rectangle().fill(.black.opacity(0.4)).ignoresSafeArea().contentShape(Rectangle()).onTapGesture {}
    }
}

/// One card for every step (confirm, waiting, result, force, failure), in the
/// main window's sheet and in the panel.
struct CloseCard: View {
    @Bindable var model: AppModel
    private var language: Language { model.language }
    /// A list longer than this scrolls.
    static let visibleRows = 6
    static let rowHeight: CGFloat = 26

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch model.close {
            case .confirm(let c): confirm(c)
            case .running(let c): waiting(c.title, L10n.text(.terminating, language))
            case .result(let outcome): result(outcome)
            case .forcing: waiting(L10n.text(.resultTitle, language), L10n.text(.forcing, language))
            case .failed(let message):
                Text(L10n.text(.resultTitle, language)).font(.title3.weight(.semibold))
                Text(message).font(.body).fixedSize(horizontal: false, vertical: true)
                buttons { Button(L10n.text(.done, language)) { model.cancelClose() }.keyboardShortcut(.cancelAction).rowFrame("closeButton", "esc") }
            case .idle: EmptyView()
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    private struct Line: Identifiable {
        var id: Int32 { pid }
        var pid: Int32
        var name: String
        var value: String
        var warn = false
    }

    /// Program name, memory or outcome, and the pid as trailing secondary text.
    private func processList(_ lines: [Line]) -> some View {
        let rows = VStack(spacing: 0) {
            ForEach(Array(lines.enumerated()), id: \.element.id) { index, line in
                if index > 0 { Divider() }
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    Text(line.name).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 8)
                    Text(line.value).monospacedDigit().foregroundStyle(line.warn ? Color.orange : Color.primary)
                    Text(L10n.format(.processNumber, language, String(line.pid)))
                        .font(.caption.monospacedDigit()).foregroundStyle(.secondary)
                }
                .font(.callout)
                .padding(.horizontal, 10)
                .frame(height: Self.rowHeight)
            }
        }
        return Group {
            if lines.count > Self.visibleRows {
                ScrollView { rows }.frame(height: Self.rowHeight * (CGFloat(Self.visibleRows) + 0.5))
            } else {
                rows
            }
        }
        .background(.quinary, in: RoundedRectangle(cornerRadius: 8))
    }

    private func buttons<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        HStack(spacing: 8) { Spacer(); content() }.controlSize(.large).padding(.top, 4)
    }

    /// Red and filled, yet never the default button: Return must not end anything (HIG).
    private func destructive(_ key: L10nKey, action: @escaping () async -> Void) -> some View {
        Button(L10n.text(key, language), role: .destructive) { Task { await action() } }
            .buttonStyle(.borderedProminent).tint(.red).rowFrame("closeButton", "destructive")
    }

    @ViewBuilder private func confirm(_ c: KillConfirmation) -> some View {
        Text(c.title).font(.title3.weight(.semibold)).lineLimit(2)
        // R2: how many processes and how much memory, in one plain sentence.
        Text(c.summary).font(.body).fixedSize(horizontal: false, vertical: true)
        if c.isProtected {
            Label(L10n.text(.protectedHint, language), systemImage: "lock.fill")
                .font(.callout.weight(.medium)).foregroundStyle(.secondary)
        }
        processList(c.processes.map { Line(pid: $0.pid, name: $0.name, value: $0.memory) })
        if c.estimateBytes != nil {
            Text(L10n.text(.estimateBasis, language)).font(.callout).foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
        buttons {
            Button(L10n.text(.cancel, language)) { model.cancelClose() }.keyboardShortcut(.cancelAction).rowFrame("closeButton", "esc")
            destructive(.confirmButton) { await model.confirmClose() }.disabled(c.isProtected)
        }
    }

    @ViewBuilder private func waiting(_ title: String, _ text: String) -> some View {
        Text(title).font(.title3.weight(.semibold)).lineLimit(2)
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(text).font(.body)
        }
    }

    @ViewBuilder private func result(_ outcome: KillOutcome) -> some View {
        let names = Dictionary((model.closing?.processes ?? []).map { ($0.pid, $0.name) }, uniquingKeysWith: { a, _ in a })
        Text(L10n.text(.resultTitle, language)).font(.title3.weight(.semibold))
        if !outcome.stillAlive.isEmpty {
            Text(L10n.format(.stillAliveQuestion, language, "\(outcome.stillAlive.count)")).font(.body.weight(.medium))
        }
        processList(outcome.lines.map {
            Line(pid: $0.pid, name: names[$0.pid] ?? "—", value: $0.text, warn: $0.status != .terminated)
        })
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(L10n.text(.observedChange, language)).font(.body)
                Spacer()
                Text(outcome.change).font(.headline).monospacedDigit()
            }
            Text(L10n.text(.observedBasis, language)).font(.callout).foregroundStyle(.secondary)
        }
        buttons {
            Button(L10n.text(.done, language)) { model.cancelClose() }.keyboardShortcut(.cancelAction).rowFrame("closeButton", "esc")
            if !outcome.stillAlive.isEmpty {
                destructive(.forceButton) { await model.confirmClose(force: true) }
            }
        }
    }
}
