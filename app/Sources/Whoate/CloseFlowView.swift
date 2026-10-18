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
                Text(message).font(.system(size: 13))
                HStack { Spacer(); Button(L10n.text(.done, language)) { model.cancelClose() } }
            case .idle: EmptyView()
            }
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12))
        .overlay(RoundedRectangle(cornerRadius: 12).stroke(Palette.line))
        .shadow(color: .black.opacity(0.2), radius: 16, y: 6)
    }

    /// Every pid is listed (plan step 6); a list taller than the box scrolls instead of truncating.
    private func pidList(_ pids: [Int32]) -> some View {
        scrollIfTall {
            Text(pids.map(String.init).joined(separator: "  "))
                .font(.system(size: 12, design: .monospaced)).foregroundStyle(.secondary)
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
        Text(c.title).font(.system(size: 14, weight: .semibold)).lineLimit(2)
        if c.isProtected {
            Label(L10n.text(.protectedHint, language), systemImage: "lock.fill")
                .font(.system(size: 12, weight: .medium)).foregroundStyle(Palette.watch)
        }
        VStack(alignment: .leading, spacing: 4) {
            Text(L10n.text(.confirmPIDs, language)).font(.system(size: 12))
            pidList(c.pids)
        }
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(L10n.text(.estimateLabel, language)).font(.system(size: 12))
                Spacer()
                Text(c.estimate).font(.system(size: 13, weight: .semibold)).monospacedDigit()
            }
            Text(L10n.text(.estimateBasis, language)).font(.system(size: 11)).foregroundStyle(.tertiary)
        }
        HStack {
            Spacer()
            Button(L10n.text(.cancel, language)) { model.cancelClose() }.keyboardShortcut(.cancelAction)
            Button(L10n.text(.confirmButton, language)) { Task { await model.confirmClose() } }
                .buttonStyle(.borderedProminent).tint(Palette.act).disabled(c.isProtected)
        }
    }

    @ViewBuilder private func running(_ c: KillConfirmation) -> some View {
        Text(c.title).font(.system(size: 14, weight: .semibold)).lineLimit(2)
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(L10n.text(.terminating, language)).font(.system(size: 12))
        }
    }

    @ViewBuilder private func forcing(_ outcome: KillOutcome) -> some View {
        Text(L10n.text(.resultTitle, language)).font(.system(size: 14, weight: .semibold))
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text(L10n.text(.forcing, language)).font(.system(size: 12))
        }
    }

    @ViewBuilder private func result(_ outcome: KillOutcome) -> some View {
        Text(L10n.text(.resultTitle, language)).font(.system(size: 14, weight: .semibold))
        scrollIfTall {
            VStack(alignment: .leading, spacing: 3) {
                ForEach(outcome.lines, id: \.pid) { line in
                    HStack {
                        Text(String(line.pid)).font(.system(size: 12, design: .monospaced))
                        Spacer()
                        Text(line.text).font(.system(size: 12))
                            .foregroundStyle(line.status == .terminated ? Color.secondary : Palette.watch)
                    }
                }
            }
        }
        VStack(alignment: .leading, spacing: 2) {
            HStack {
                Text(L10n.text(.observedChange, language)).font(.system(size: 12))
                Spacer()
                Text(outcome.change).font(.system(size: 13, weight: .semibold)).monospacedDigit()
            }
            Text(L10n.text(.observedBasis, language)).font(.system(size: 11)).foregroundStyle(.tertiary)
        }
        if !outcome.stillAlive.isEmpty {
            Text(L10n.format(.stillAliveQuestion, language, "\(outcome.stillAlive.count)"))
                .font(.system(size: 12, weight: .medium))
        }
        HStack {
            Spacer()
            Button(L10n.text(.done, language)) { model.cancelClose() }
            if !outcome.stillAlive.isEmpty {
                Button(L10n.text(.forceButton, language)) { Task { await model.confirmClose(force: true) } }
                    .buttonStyle(.borderedProminent).tint(Palette.act)
            }
        }
    }
}
