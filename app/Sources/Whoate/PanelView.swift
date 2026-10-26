// Menu bar panel (docs/specs/mockups/menubar.html). Also hosted in a normal
// window for automated screenshots; both use this exact view.
import SwiftUI
import WhoateKit

struct PanelView: View {
    @Bindable var model: AppModel
    let mainWindows: MainWindows
    var openMain: (String?) -> Void
    let quit: QuitAction
    /// The footer's gear (Phase 11): opens the settings window and puts the panel away.
    var openSettings: () -> Void = {}
    /// Receives the panel's window once it has one (to put it away after opening the main window).
    var foundWindow: ((NSWindow) -> Void)?

    private var language: Language { model.language }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            content
            Divider()
            footer
        }
        .frame(width: 420)
        .environment(\.curveGapLimit, model.profile.gapLimit)
        .coordinateSpace(.named(RowFrames.space))
        .overlay { CloseFlowView(model: model) }
        .background(RefreshVisibility { model.setVisible(panel: $0) }.frame(width: 0, height: 0))
        .background(WindowReporter { window in foundWindow?(window) }.frame(width: 0, height: 0))
        .onPreferenceChange(RowFrames.self) { frames in
            if let path = WhoateApp.automation.layoutFile { RowFrames.write(frames, to: path) }
            if !WhoateApp.automation.panelDoubleClicks.isEmpty { RowFrames.panel = frames }
        }
    }

    private var header: some View {
        let header = Panel.header(model.report?.host, sort: model.sort, language)
        return VStack(alignment: .leading, spacing: 8) {
            Text(header.headline).font(.title3.weight(.semibold))
            // R21: physical memory with its total and share, the compressor's part of it, swap with its total.
            if let host = model.report?.host {
                VStack(alignment: .leading, spacing: 2) {
                    Text(MemoryWording.physical(host, language) + " · " + MemoryWording.compressed(host, language))
                    Text(MemoryWording.swap(host, language))
                }
                .font(.callout).monospacedDigit().foregroundStyle(.secondary)
                .help(MemoryWording.hostHelp(language))
            }
            if let detail = header.detail {
                Text(detail).font(.callout).foregroundStyle(.secondary).monospacedDigit()
            }
            if let host = model.report?.host { PressureBar(host: host) }
            HStack(spacing: 12) {
                legend(L10n.text(.memOtherUsed, language), Palette.used)
                legend(L10n.text(.compressed, language), Palette.compressed)
                legend(L10n.text(.swap, language), Palette.swap)
                Spacer()
                Picker("", selection: $model.sort) {
                    Text(L10n.text(.sortMemory, language)).tag(SortKey.memory)
                    Text(L10n.text(.sortCPU, language)).tag(SortKey.cpu)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize().controlSize(.small)
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
    }

    private func legend(_ text: String, _ color: Color) -> some View {
        HStack(spacing: 4) {
            RoundedRectangle(cornerRadius: 1.5).fill(color).frame(width: 8, height: 8)
            Text(text)
        }
        .font(.subheadline).foregroundStyle(.secondary)
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
                // R21: what the rows' number is, in memory order.
                if model.sort == .memory {
                    Text(L10n.text(.memPanelCaption, language)).font(.caption).foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .trailing).padding(.horizontal, 16).padding(.top, 6)
                }
                ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                    if index > 0 { Divider().padding(.leading, 16) }
                    PanelRowView(row: row, language: language, sort: model.sort,
                                 openMain: openMain, close: { model.requestClose(row.id, from: .panel) },
                                 doubleClick: { openMain(model.panelTarget(row)) },
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
            Text(title).font(.headline)
            if let detail { Text(detail).font(.callout).foregroundStyle(.secondary) }
        }
        .frame(maxWidth: .infinity).padding(.vertical, 28)
    }

    /// Owner design gate 2026-10-21: quit stays a button, alone at the far left
    /// and borderless; the bordered main-window button is at the far right, with
    /// the status line between them (feedback 3; app/DESIGN.md §5). Words: R1.
    private var footer: some View {
        HStack(spacing: 12) {
            Button(L10n.text(.quit, language)) { quit() }.keyboardShortcut("q")
                .buttonStyle(.borderless).foregroundStyle(.secondary).fixedSize()
                .rowFrame("footer", "quit")
            Text(footerText).font(.callout).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                .frame(maxWidth: .infinity, alignment: .center)
            // Phase 11: settings next to the main-window button, away from quit (research F4).
            Button { openSettings() } label: { Image(systemName: "gearshape") }
                .buttonStyle(.borderless).foregroundStyle(.secondary).fixedSize()
                .help(L10n.text(.settingsTitle, language))
                .accessibilityLabel(L10n.text(.settingsTitle, language))
                .rowFrame("footer", "settings")
            Button(WindowPolicy.openLabel(mainWindowExists: mainWindows.exists, language)) { openMain(nil) }
                .buttonStyle(.bordered).fixedSize()
                .rowFrame("footer", "showMain")
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
    }

    private var footerText: String {
        Panel.footer(model.report, lastError: model.refreshError, interval: model.profile.visible, language: language)
    }
}

/// R21: physical memory on one bar (used, with the compressor's part marked) against
/// the machine's total; swap on its own thin bar against the swap files' total. Swap is
/// not physical memory and is never drawn on that bar.
struct PressureBar: View {
    let host: WhoateKit.Host
    var body: some View {
        let s = Panel.physicalSegments(host)
        VStack(spacing: 4) {
            GeometryReader { geo in
                HStack(spacing: 0) {
                    Palette.used.frame(width: geo.size.width * s.other)
                    Palette.compressed.frame(width: geo.size.width * s.compressed)
                    Spacer(minLength: 0)
                }
            }
            .frame(height: 8).background(.quaternary).clipShape(RoundedRectangle(cornerRadius: 4))
            if let swap = Panel.swapFraction(host) {
                GeometryReader { geo in
                    HStack(spacing: 0) {
                        Palette.swap.frame(width: geo.size.width * swap)
                        Spacer(minLength: 0)
                    }
                }
                .frame(height: 4).background(.quaternary).clipShape(RoundedRectangle(cornerRadius: 2))
            }
        }
    }
}

struct PanelRowView: View {
    let row: PanelRow
    let language: Language
    let sort: SortKey
    var openMain: (String?) -> Void
    var close: () -> Void
    /// R14: a double-click anywhere but a button opens the main window on this row.
    var doubleClick: () -> Void
    let expanded: Binding<Bool>

    // Curve and value share the first line; buttons own the second, so the
    // curve can never cover them.
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .center, spacing: 8) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(row.title).font(.headline).lineLimit(1).truncationMode(.middle)
                    Text(row.subtitle).font(.callout).foregroundStyle(.secondary)
                        .lineLimit(1).truncationMode(row.source == .swarm ? .tail : .middle)
                }
                Spacer(minLength: 0)
                Sparkline(points: row.history, color: Palette.tint(row.level), language: language, metric: sort,
                          hasHistory: row.source == .owner)
                    .frame(width: 64, height: 18).clipped()
                    .rowFrame(row.id, "trend")
                    .background { if WhoateApp.automation.layoutFile != nil { CurveKind(row: row) } }
                ValueText(row: row, sort: sort, language: language)
                    .frame(minWidth: 52, alignment: .trailing)
            }
            HStack(spacing: 8) {
                AdvicePill(row: row, language: language, expanded: expanded)
                    .rowFrame(row.id, "pill")
                Spacer()
                if row.closable {
                    HStack(spacing: 8) {
                        Button(L10n.text(.details, language)) { openMain(row.id) }.controlSize(.small)
                        // A bordered button ignores tint on macOS: the title itself is red (R2).
                        Button(role: .destructive, action: close) {
                            Text(L10n.text(.close, language)).foregroundStyle(.red)
                        }
                        .controlSize(.small).tint(.red)
                    }
                    .rowFrame(row.id, "buttons")
                }
            }
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
        // Buttons (details, close, the advice pill) keep their own clicks.
        .contentShape(Rectangle())
        .onTapGesture(count: 2, perform: doubleClick)
        .rowFrame(row.id, "row")
    }
}

/// The pill; with advice it also shows the entries' text on hover and on click.
/// (No @State: the SwiftUI macro plugin is missing under Command Line Tools.)
struct AdvicePill: View {
    let row: PanelRow
    let language: Language
    @Binding var expanded: Bool

    var body: some View {
        let pill = Pill(row: row)
        if row.insights.isEmpty {
            pill
        } else {
            let text = Advice.lines(row.insights, language: language)
            Button { expanded.toggle() } label: { pill }
                .buttonStyle(.plain)
                .help(text)
                .popover(isPresented: $expanded, arrowEdge: .bottom) {
                    Text(text).font(.callout).textSelection(.enabled)
                        .frame(width: 320, alignment: .leading).fixedSize(horizontal: false, vertical: true)
                        .padding(12)
                }
        }
    }
}

/// One pill for panel rows, main-window rows and details (feedback 4): symbol
/// and text in the level's system colour on a 15% tint of it; ok rows are plain.
struct Pill: View {
    let text: String
    let level: Level
    var symbol: String?

    init(text: String, level: Level, symbol: String? = nil) {
        self.text = text
        self.level = level
        self.symbol = symbol
    }
    init(row: PanelRow) { self.init(text: row.pill, level: row.pillLevel, symbol: Palette.symbol(row)) }

    var body: some View {
        HStack(spacing: 4) {
            if let symbol { Image(systemName: symbol).foregroundStyle(Palette.tint(level)) }
            Text(text).foregroundStyle(level == .act || level == .watch ? Color.primary : Color.secondary)
        }
        .font(.subheadline.weight(.medium)).lineLimit(1)
        .padding(.horizontal, 8).padding(.vertical, 2)
        .background(level == .act || level == .watch ? AnyShapeStyle(Palette.tint(level).opacity(0.15))
                                                      : AnyShapeStyle(.quaternary), in: Capsule())
    }
}

/// The row's number: memory ("≥ 596M" when some processes are unreadable, R5) or CPU (same rule, R12).
struct ValueText: View {
    let row: PanelRow
    let sort: SortKey
    let language: Language
    var body: some View {
        if sort == .memory {
            MemoryText(memory: row.memory(language)).font(.headline)
        } else {
            MemoryText(memory: row.cpu(language)).font(.headline)
        }
    }
}

/// A number from Format.memory or Format.cpu; hover tells how many processes were unreadable.
struct MemoryText: View {
    let memory: Format.Memory
    var body: some View {
        if let help = memory.help {
            Text(memory.text).monospacedDigit().help(help).accessibilityLabel(memory.text + ", " + help)
        } else {
            Text(memory.text).monospacedDigit()
        }
    }
}

/// Plain Path, not Swift Charts: this redraws every second for five rows.
/// Only the latest continuous run is drawn; a short run shows text instead.
struct Sparkline: View {
    let points: [HistoryPoint]
    let color: Color
    let language: Language
    var metric: SortKey = .memory
    /// Process groups keep no history: no curve and no "not enough samples" either.
    var hasHistory = true
    @Environment(\.curveGapLimit) private var gapLimit
    var body: some View {
        if hasHistory { curve }
    }
    private var curve: some View {
        GeometryReader { geo in
            switch Format.trend(points, width: geo.size.width, height: geo.size.height - 2, metric: metric, gapLimit: gapLimit) {
            case .insufficient:
                Text(L10n.text(.notEnoughSamples, language))
                    .font(.caption).foregroundStyle(.secondary)
                    .lineLimit(1).minimumScaleFactor(0.6)
                    .frame(width: geo.size.width, height: geo.size.height, alignment: .trailing)
            case .line(let line):
                Path { path in
                    path.move(to: CGPoint(x: line[0].x, y: line[0].y + 1))
                    for p in line.dropFirst() { path.addLine(to: CGPoint(x: p.x, y: p.y + 1)) }
                }
                .stroke(color, style: StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
            }
        }
    }
}

/// Automation only: reports whether the row draws a line or the
/// not-enough-samples text, as a "curve\t<line|insufficient>" layout entry.
struct CurveKind: View {
    let row: PanelRow
    @Environment(\.curveGapLimit) private var gapLimit
    var body: some View {
        let line = Format.trend(row.history, width: 1, height: 1, gapLimit: gapLimit) != .insufficient
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
    /// Automation only (WHOATE_UI_PANEL_DOUBLE_CLICKS): the panel's latest frames, to aim the clicks.
    nonisolated(unsafe) static var panel: [String: CGRect] = [:]
    /// Automation only (R16 check): the main window's latest frames, for the state file.
    nonisolated(unsafe) static var main: [String: CGRect] = [:]
    nonisolated(unsafe) static var sheet: [String: CGRect] = [:]
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

/// Curves split where points are further apart than this (Phase 11: it follows the
/// refresh profile). The panel and the main window set it from AppModel.profile.
private struct CurveGapLimitKey: EnvironmentKey {
    static let defaultValue = Format.gapLimit
}
extension EnvironmentValues {
    var curveGapLimit: Int64 {
        get { self[CurveGapLimitKey.self] }
        set { self[CurveGapLimitKey.self] = newValue }
    }
}
