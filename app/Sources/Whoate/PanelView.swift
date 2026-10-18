// Menu bar panel (docs/specs/mockups/menubar.html). Also hosted in a normal
// window for automated screenshots; both use this exact view.
import SwiftUI
import WhoateKit

struct PanelView: View {
    @Bindable var model: AppModel
    var openMain: (String?) -> Void

    private var language: Language { model.language }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider().overlay(Palette.line)
            content
            Divider().overlay(Palette.line)
            footer
        }
        .frame(width: 420)
        .overlay { CloseFlowView(model: model) }
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(Panel.headline(model.report?.host, language)).font(.system(size: 15, weight: .semibold))
                Spacer()
                if let host = model.report?.host {
                    Text("\(Format.bytes(host.memUsedBytes)) / \(Format.bytes(host.memTotalBytes)) · \(L10n.text(.swap, language)) \(Format.bytes(host.swapUsedBytes))")
                        .monospacedDigit().foregroundStyle(.secondary)
                }
            }
            if let host = model.report?.host { PressureBar(host: host) }
            HStack(spacing: 12) {
                legend(L10n.text(.used, language), Palette.used)
                legend(L10n.text(.compressed, language), Palette.compressed)
                legend(L10n.text(.swap, language), Palette.watch)
                Spacer()
                Picker("", selection: $model.sort) {
                    Text(L10n.text(.sortMemory, language)).tag(SortKey.memory)
                    Text(L10n.text(.sortCPU, language)).tag(SortKey.cpu)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize().controlSize(.small)
            }
        }
        .padding(.horizontal, 16).padding(.top, 14).padding(.bottom, 10)
    }

    private func legend(_ text: String, _ color: Color) -> some View {
        HStack(spacing: 4) {
            RoundedRectangle(cornerRadius: 1.5).fill(color).frame(width: 8, height: 8)
            Text(text)
        }
        .font(.system(size: 11)).foregroundStyle(.secondary)
    }

    @ViewBuilder private var content: some View {
        let rows = model.rows
        if model.engineFailed && model.report == nil {
            message(L10n.text(.engineFailed, language), detail: L10n.text(.engineRetrying, language))
        } else if model.report == nil {
            message(L10n.text(.loading, language), detail: nil)
        } else if rows.isEmpty {
            message(L10n.text(.noOwners, language), detail: nil)
        } else {
            VStack(spacing: 0) {
                ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                    if index > 0 { Divider().overlay(Palette.line.opacity(0.6)) }
                    PanelRowView(row: row, language: language, sort: model.sort,
                                 openMain: openMain, close: { model.requestClose(row.id) })
                }
            }
        }
    }

    private func message(_ title: String, detail: String?) -> some View {
        VStack(spacing: 4) {
            Text(title).font(.system(size: 13, weight: .medium))
            if let detail { Text(detail).font(.system(size: 12)).foregroundStyle(.secondary) }
        }
        .frame(maxWidth: .infinity).padding(.vertical, 28)
    }

    private var footer: some View {
        HStack {
            Text(footerText).font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
            Spacer()
            Button(L10n.text(.openWhoate, language)) { openMain(nil) }.controlSize(.regular)
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
    }

    private var footerText: String {
        Panel.footer(model.report, lastError: model.lastError, interval: AppModel.refreshInterval, language: language)
    }
}

struct PressureBar: View {
    let host: WhoateKit.Host
    var body: some View {
        let s = Panel.pressureSegments(host)
        GeometryReader { geo in
            HStack(spacing: 0) {
                Palette.used.frame(width: geo.size.width * s.used)
                Palette.compressed.frame(width: geo.size.width * s.compressed)
                Palette.watch.frame(width: geo.size.width * s.swap)
                Spacer(minLength: 0)
            }
        }
        .frame(height: 8).background(Palette.track).clipShape(RoundedRectangle(cornerRadius: 4))
    }
}

struct PanelRowView: View {
    let row: PanelRow
    let language: Language
    let sort: SortKey
    var openMain: (String?) -> Void
    var close: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(row.title).font(.system(size: 13, weight: .semibold)).lineLimit(1).truncationMode(.middle)
                    Text(row.subtitle).font(.system(size: 12)).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                }
                Spacer(minLength: 0)
                VStack(alignment: .trailing, spacing: 5) {
                    Text(sort == .memory ? Format.bytes(row.memBytes) : Format.cpu(row.cpuPct))
                        .font(.system(size: 13, weight: .semibold)).monospacedDigit()
                    Sparkline(points: row.history, color: row.level == .ok ? Palette.flat : Palette.tint(row.level))
                        .frame(width: 64, height: 18).clipped()
                }
            }
            HStack(spacing: 6) {
                Pill(text: row.pill, level: row.pillLevel)
                Spacer()
                if row.closable {
                    Button(L10n.text(.details, language)) { openMain(row.id) }.controlSize(.small)
                    Button(L10n.text(.close, language), action: close)
                        .controlSize(.small).buttonStyle(.borderedProminent).tint(Palette.act)
                }
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }
}

struct Pill: View {
    let text: String
    let level: Level
    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .semibold)).lineLimit(1)
            .foregroundStyle(Palette.tint(level))
            .padding(.horizontal, 8).padding(.vertical, 2)
            .background(Palette.background(level), in: Capsule())
    }
}

/// Plain Path, not Swift Charts: this redraws every second for five rows.
struct Sparkline: View {
    let points: [HistoryPoint]
    let color: Color
    var body: some View {
        Canvas { context, size in
            for segment in Format.sparkline(points, width: size.width, height: size.height - 2) {
                var path = Path()
                path.move(to: CGPoint(x: segment[0].x, y: segment[0].y + 1))
                for p in segment.dropFirst() { path.addLine(to: CGPoint(x: p.x, y: p.y + 1)) }
                if segment.count == 1 {
                    path.addEllipse(in: CGRect(x: segment[0].x - 1.5, y: segment[0].y - 0.5, width: 3, height: 3))
                }
                context.stroke(path, with: .color(color), lineWidth: 1.6)
            }
        }
    }
}
