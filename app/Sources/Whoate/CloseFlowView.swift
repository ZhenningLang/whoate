// Two-step close inside the panel: confirm → SIGTERM (5 s) → result →
// optional force for the pids still alive. Sheets are unreliable in a
// MenuBarExtra window, so this is an overlay card.
import SwiftUI
import WhoateKit

struct CloseFlowView: View {
    @Bindable var model: AppModel
    private var language: Language { model.language }

    var body: some View {
        if model.close != .idle {
            ZStack {
                Rectangle().fill(.black.opacity(0.18)).onTapGesture {}
                card.padding(18)
            }
        }
    }

    @ViewBuilder private var card: some View {
        VStack(alignment: .leading, spacing: 12) {
            switch model.close {
            case .confirm(let c): confirm(c)
            case .running(let c): running(c)
            case .result(let outcome): result(outcome)
            case .forcing(let outcome): forcing(outcome)
            case .failed(let message):
                Text(message).font(.body)
                HStack { Spacer(); Button(L10n.text(.done, language)) { model.cancelClose() } }
            case .idle: EmptyView()
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(.separator))
        .shadow(color: .black.opacity(0.2), radius: 16, y: 6)
    }

    /// Every pid is listed (plan step 6); a list taller than the box scrolls instead of truncating.
    private func pidList(_ pids: [Int32]) -> some View {
        scrollIfTall {
            Text(pids.map(String.init).joined(separator: "  "))
                .font(.callout.monospaced()).foregroundStyle(.secondary)
        }
    }

    private func scrollIfTall<Content: View>(@ViewBuilder _ content: () -> Content) -> some View {
        let body = content().frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
        return ViewThatFits(in: .vertical) {
            body
            ScrollView { body }.frame(height: 120)
        }
        .frame(maxHeight: 120)
    }

    @ViewBuilder private func confirm(_ c: KillConfirmation) -> some View {
        Text(c.title).font(.title3.weight(.semibold)).lineLimit(2)
        // R2: how many processes and how much memory, before the pid list.
        Text(c.summary).font(.callout.weight(.medium)).fixedSize(horizontal: false, vertical: true)
        if c.isProtected {
            Label(L10n.text(.protectedHint, language), systemImage: "lock.fill")
                .font(.callout.weight(.medium)).foregroundStyle(.secondary)
        }
        VStack(alignment: .leading, spacing: 4) {
            Text(L10n.text(.confirmPIDs, language)).font(.callout)
            pidList(c.pids)
        }
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(L10n.text(.estimateLabel, language)).font(.callout)
                Spacer()
                Text(c.estimate).font(.headline).monospacedDigit()
            }
            Text(L10n.text(.estimateBasis, language)).font(.subheadline).foregroundStyle(.tertiary)
        }
        HStack {
            Spacer()
            Button(L10n.text(.cancel, language)) { model.cancelClose() }.keyboardShortcut(.cancelAction)
            // Destructive, so never the default (primary) button: HIG Buttons.
            Button(L10n.text(.confirmButton, language), role: .destructive) { Task { await model.confirmClose() } }
                .disabled(c.isProtected)
        }
    }

    @ViewBuilder private func running(_ c: KillConfirmation) -> some View {
        Text(c.title).font(.title3.weight(.semibold)).lineLimit(2)
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(L10n.text(.terminating, language)).font(.callout)
        }
    }

    @ViewBuilder private func forcing(_ outcome: KillOutcome) -> some View {
        Text(L10n.text(.resultTitle, language)).font(.title3.weight(.semibold))
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(L10n.text(.forcing, language)).font(.callout)
        }
    }

    @ViewBuilder private func result(_ outcome: KillOutcome) -> some View {
        Text(L10n.text(.resultTitle, language)).font(.title3.weight(.semibold))
        scrollIfTall {
            VStack(alignment: .leading, spacing: 3) {
                ForEach(outcome.lines, id: \.pid) { line in
                    HStack {
                        Text(String(line.pid)).font(.callout.monospaced())
                        Spacer()
                        Text(line.text).font(.callout)
                            .foregroundStyle(line.status == .terminated ? Color.secondary : Color.orange)
                    }
                }
            }
        }
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(L10n.text(.observedChange, language)).font(.callout)
                Spacer()
                Text(outcome.change).font(.headline).monospacedDigit()
            }
            Text(L10n.text(.observedBasis, language)).font(.subheadline).foregroundStyle(.tertiary)
        }
        if !outcome.stillAlive.isEmpty {
            Text(L10n.format(.stillAliveQuestion, language, "\(outcome.stillAlive.count)"))
                .font(.callout.weight(.medium))
        }
        HStack {
            Spacer()
            Button(L10n.text(.done, language)) { model.cancelClose() }
            if !outcome.stillAlive.isEmpty {
                Button(L10n.text(.forceButton, language), role: .destructive) { Task { await model.confirmClose(force: true) } }
            }
        }
    }
}
