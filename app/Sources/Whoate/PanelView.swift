// Menu bar panel (docs/specs/mockups/menubar.html). Also hosted in a normal
// window for automated screenshots; both use this exact view.
import SwiftUI
import WhoateKit

struct PanelView: View {
    @Bindable var model: AppModel
    let mainWindows: MainWindows
    var openMain: (String?) -> Void
    let quit: QuitAction

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
        .coordinateSpace(.named(RowFrames.space))
        .overlay { CloseFlowView(model: model) }
        .background(RefreshVisibility { model.setVisible(panel: $0) }.frame(width: 0, height: 0))
        .onPreferenceChange(RowFrames.self) { frames in
            if let path = WhoateApp.automation.layoutFile { RowFrames.write(frames, to: path) }
        }
    }

    private var header: some View {
        let header = Panel.header(model.report?.host, sort: model.sort, language)
        return VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .firstTextBaseline) {
                Text(header.headline).font(.system(size: 15, weight: .semibold))
                Spacer()
                if let host = model.report?.host {
                    Text("\(Format.bytes(host.memUsedBytes)) / \(Format.bytes(host.memTotalBytes)) · \(L10n.text(.swap, language)) \(Format.bytes(host.swapUsedBytes))")
                        .monospacedDigit().foregroundStyle(.secondary)
                }
            }
            if let detail = header.detail {
                Text(detail).font(.system(size: 12)).foregroundStyle(.secondary).monospacedDigit()
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
                                 openMain: openMain, close: { model.requestClose(row.id) },
                                 expanded: Binding(get: { model.expandedAdvice == row.id },
                                                   set: { model.expandedAdvice = $0 ? row.id : nil }))
                }
            }
            // CPU order holds still under the pointer so a row can be clicked.
            .onHover { model.setListHovered($0) }
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
            Button(L10n.text(.quit, language)) { quit() }.controlSize(.regular).keyboardShortcut("q")
            Button(WindowPolicy.openLabel(mainWindowExists: mainWindows.exists, language)) { openMain(nil) }
                .controlSize(.regular)
        }
        .padding(.horizontal, 16).padding(.vertical, 10)
    }

    private var footerText: String {
        Panel.footer(model.report, lastError: model.refreshError, interval: AppModel.refreshInterval, language: language)
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
    let expanded: Binding<Bool>

    // Curve and value share the first line; buttons own the second, so the
    // curve can never cover them.
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 10) {
                VStack(alignment: .leading, spacing: 3) {
                    Text(row.title).font(.system(size: 13, weight: .semibold)).lineLimit(1).truncationMode(.middle)
                    Text(row.subtitle).font(.system(size: 12)).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(.middle)
                }
                Spacer(minLength: 0)
                Sparkline(points: row.history, color: row.level == .ok ? Palette.flat : Palette.tint(row.level),
                          language: language)
                    .frame(width: 64, height: 18).clipped()
                    .rowFrame(row.id, "trend")
                    .background { if WhoateApp.automation.layoutFile != nil { CurveKind(row: row) } }
                Text(sort == .memory ? Format.bytes(row.memBytes) : Format.cpu(row.cpuPct))
                    .font(.system(size: 13, weight: .semibold)).monospacedDigit()
                    .frame(minWidth: 44, alignment: .trailing)
            }
            HStack(spacing: 6) {
                AdvicePill(row: row, language: language, expanded: expanded)
                Spacer()
                if row.closable {
                    HStack(spacing: 6) {
                        Button(L10n.text(.details, language)) { openMain(row.id) }.controlSize(.small)
                        Button(L10n.text(.close, language), action: close)
                            .controlSize(.small).buttonStyle(.borderedProminent).tint(Palette.act)
                    }
                    .rowFrame(row.id, "buttons")
                }
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
    }
}

/// The pill; with advice it also shows the entries' text on hover and on click.
/// (No @State: the SwiftUI macro plugin is missing under Command Line Tools.)
struct AdvicePill: View {
    let row: PanelRow
    let language: Language
    @Binding var expanded: Bool

    var body: some View {
        let pill = Pill(text: row.pill, level: row.pillLevel)
        if row.insights.isEmpty {
            pill
        } else {
            let text = Advice.lines(row.insights, language: language)
            Button { expanded.toggle() } label: { pill }
                .buttonStyle(.plain)
                .help(text)
                .popover(isPresented: $expanded, arrowEdge: .bottom) {
                    Text(text).font(.system(size: 12)).textSelection(.enabled)
                        .frame(width: 320, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                        .padding(12)
                }
        }
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
/// Only the latest continuous run is drawn; a short run shows text instead.
struct Sparkline: View {
    let points: [HistoryPoint]
    let color: Color
    let language: Language
    var body: some View {
        GeometryReader { geo in
            switch Format.trend(points, width: geo.size.width, height: geo.size.height - 2) {
            case .insufficient:
                Text(L10n.text(.notEnoughSamples, language))
                    .font(.system(size: 10)).foregroundStyle(.secondary)
                    .lineLimit(1).minimumScaleFactor(0.6)
                    .frame(width: geo.size.width, height: geo.size.height, alignment: .trailing)
            case .line(let line):
                Path { path in
                    path.move(to: CGPoint(x: line[0].x, y: line[0].y + 1))
                    for p in line.dropFirst() { path.addLine(to: CGPoint(x: p.x, y: p.y + 1)) }
                }
                .stroke(color, lineWidth: 1.6)
            }
        }
    }
}

/// Automation only: reports whether the row draws a line or the
/// not-enough-samples text, as a "curve\t<line|insufficient>" layout entry.
struct CurveKind: View {
    let row: PanelRow
    var body: some View {
        let line = Format.trend(row.history, width: 1, height: 1) != .insufficient
        Color.clear.rowFrame(row.id, "curve\t" + (line ? "line" : "insufficient"))
    }
}

/// Automation only (WHOATE_UI_LAYOUT_FILE): panel row part frames for the
/// button/curve overlap check, plus each row's curve kind with the write time.
struct RowFrames: PreferenceKey {
    static let space = "panel"
    static let defaultValue: [String: CGRect] = [:]
    static func reduce(value: inout [String: CGRect], nextValue: () -> [String: CGRect]) {
        value.merge(nextValue()) { $1 }
    }
    static func write(_ frames: [String: CGRect], to path: String) {
        let now = Date().timeIntervalSince1970
        let lines = frames.sorted { $0.key < $1.key }.map { key, r in
            key.split(separator: "\t").dropFirst().first == "curve" ? "\(key)\t\(now)\n"
                : "\(key)\t\(r.minX)\t\(r.minY)\t\(r.width)\t\(r.height)\n"
        }
        try? lines.joined().write(toFile: path, atomically: true, encoding: .utf8)
    }
}

extension View {
    func rowFrame(_ id: String, _ part: String) -> some View {
        background(GeometryReader { geo in
            Color.clear.preference(key: RowFrames.self,
                                   value: ["\(id)\t\(part)": geo.frame(in: .named(RowFrames.space))])
        })
    }
}
