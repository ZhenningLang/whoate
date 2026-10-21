// Three-column main window (app/DESIGN.md): sidebar of kinds and filters,
// the panel's own rows in the middle, details on the right. Selection, sort
// and the window/activation plumbing (MainWindows) are shared with the panel.
import AppKit
import SwiftUI
import WhoateKit

struct MainWindowView: View {
    @Bindable var model: AppModel
    let mainWindows: MainWindows

    private var language: Language { model.language }

    var body: some View {
        NavigationSplitView {
            SidebarView(model: model)
                .navigationSplitViewColumnWidth(min: 180, ideal: 200, max: 260)
        } content: {
            OwnerListView(model: model)
                .navigationSplitViewColumnWidth(min: 320, ideal: 360, max: 480)
        } detail: {
            DetailView(model: model)
                .overlay { CloseFlowView(model: model) }
        }
        .toolbar {
            ToolbarItemGroup(placement: .primaryAction) {
                HostToolbar(model: model)
                // Same model.sort as the panel: switching here switches there.
                Picker("", selection: $model.sort) {
                    Text(L10n.text(.sortMemory, language)).tag(SortKey.memory)
                    Text(L10n.text(.sortCPU, language)).tag(SortKey.cpu)
                }
                .pickerStyle(.segmented).labelsHidden().fixedSize()
            }
        }
        .frame(minWidth: 960, minHeight: 600)
        .coordinateSpace(.named(RowFrames.space))
        .onPreferenceChange(RowFrames.self) { frames in
            // Its own file: the panel window writes WHOATE_UI_LAYOUT_FILE itself.
            if let path = WhoateApp.automation.layoutFile { RowFrames.write(frames, to: path + ".main") }
        }
        .background(RefreshVisibility { model.setVisible(main: $0) }.frame(width: 0, height: 0))
        .background(WindowReporter { window in
            mainWindows.register(window)
            automationMainWindow(window)
        }.frame(width: 0, height: 0))
    }
}

/// Toolbar: pressure pill and used/total/swap; in CPU mode the panel's CPU header.
struct HostToolbar: View {
    let model: AppModel
    var body: some View {
        let language = model.language
        let header = Panel.header(model.report?.host, sort: model.sort, language)
        HStack(spacing: 12) {
            Pill(text: header.headline, level: Self.level(model.report?.host?.pressure, sort: model.sort),
                 symbol: Self.symbol(model.report?.host?.pressure, sort: model.sort))
            if model.sort == .cpu, let detail = header.detail {
                Text(detail).font(.callout).monospacedDigit().foregroundStyle(.secondary)
            } else if let host = model.report?.host {
                Text("\(L10n.text(.memoryUsed, language)) \(Format.bytes(host.memUsedBytes)) / \(Format.bytes(host.memTotalBytes)) · \(L10n.text(.swap, language)) \(Format.bytes(host.swapUsedBytes))")
                    .font(.callout).monospacedDigit().foregroundStyle(.secondary)
            }
        }
        .fixedSize()
    }

    static func level(_ pressure: Pressure?, sort: SortKey) -> Level {
        guard sort == .memory else { return .ok }
        switch pressure {
        case .critical: return .act
        case .warn: return .watch
        default: return .ok
        }
    }
    static func symbol(_ pressure: Pressure?, sort: SortKey) -> String? {
        guard sort == .memory else { return nil }
        switch pressure {
        case .critical: return "exclamationmark.triangle.fill"
        case .warn: return "exclamationmark.circle.fill"
        default: return nil
        }
    }
}

struct SidebarView: View {
    @Bindable var model: AppModel
    var body: some View {
        let language = model.language
        let rows = model.rows(limit: .max)
        let selection = Binding<MainCategory?>(get: { model.category }, set: { if let c = $0 { model.category = c } })
        List(selection: selection) {
            Section(L10n.text(.sectionOwners, language)) {
                ForEach([MainCategory.all, .tmux, .agents, .apps, .orphans, .system], id: \.self) { item($0, rows) }
            }
            Section(L10n.text(.sectionFilters, language)) {
                ForEach([MainCategory.needsAction, .growing], id: \.self) { item($0, rows) }
            }
        }
        .listStyle(.sidebar)
    }

    private func item(_ category: MainCategory, _ rows: [PanelRow]) -> some View {
        let total = model.report == nil ? "" : MainWindowModel.total(rows, category)
        return Label(category.title(model.language), systemImage: category.symbol)
            .badge(Text(total).monospacedDigit())
            .tag(category)
    }
}

struct OwnerListView: View {
    @Bindable var model: AppModel
    var body: some View {
        let language = model.language
        let rows = model.report.map { report in
            model.rows(limit: .max).filter { MainWindowModel.matches($0, model.category) }
        } ?? []
        let metric = L10n.text(model.sort == .memory ? .sortMemory : .sortCPU, language)
        VStack(spacing: 0) {
            HStack {
                Text(L10n.format(.ownersCount, language, "\(rows.count)") + " · " + L10n.format(.sortedBy, language, metric))
                Spacer()
                Text(L10n.text(.last30Minutes, language))
            }
            .font(.subheadline).foregroundStyle(.secondary)
            .padding(.horizontal, 16).padding(.vertical, 8)
            Divider()
            List(selection: $model.selectedOwnerID) {
                ForEach(rows) { row in
                    MainRowView(row: row, sort: model.sort, language: language).tag(row.id)
                        // Rows on screen get full histories on the next refresh.
                        .onAppear { model.setMainRow(row.id, visible: true) }
                        .onDisappear { model.setMainRow(row.id, visible: false) }
                }
            }
            .listStyle(.inset)
            .onHover { model.setListHovered($0) }
        }
    }
}

/// Same content as a panel row: title and value, subtitle and the other metric, pill and curve.
struct MainRowView: View {
    let row: PanelRow
    let sort: SortKey
    let language: Language
    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(row.title).font(.headline).lineLimit(1).truncationMode(.middle)
                Spacer(minLength: 8)
                ValueText(row: row, sort: sort, language: language)
            }
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(row.subtitle).font(.callout).foregroundStyle(.secondary).lineLimit(1).truncationMode(row.source == .swarm ? .tail : .middle)
                Spacer(minLength: 8)
                Group {
                    if sort == .memory {
                        Text("CPU " + Format.cpu(row.cpuPct)).monospacedDigit()
                    } else {
                        MemoryText(memory: row.memory(language))
                    }
                }
                .font(.callout).foregroundStyle(.secondary)
            }
            HStack(spacing: 8) {
                Pill(row: row)
                Spacer(minLength: 8)
                Sparkline(points: row.history, color: Palette.tint(row.level), language: language, metric: sort,
                          hasHistory: row.source == .owner)
                    .frame(width: 72, height: 16).clipped()
            }
        }
        .padding(.vertical, 4)
    }
}

/// Automation only. main-window mode parks the window in the main screen's
/// bottom-right corner, so the focus switch in the QA12 check (Finder) does
/// not happen to cover it; WHOATE_UI_WINDOW_FILE receives its line.
@MainActor func automationMainWindow(_ window: NSWindow) {
    let automation = WhoateApp.automation
    if automation.mainWindow, let screen = NSScreen.main?.visibleFrame {
        window.setFrameOrigin(NSPoint(x: screen.maxX - window.frame.width - 8, y: screen.minY + 8))
    }
    guard let path = automation.windowFile else { return }
    let size = window.frame.size
    appendLine("main\t\(window.windowNumber)\t\(Int(size.width))\t\(Int(size.height))\t"
        + "\(window.backingScaleFactor)\t\(ProcessInfo.processInfo.processIdentifier)\t-\n",
        to: path, failure: "window_file_failed")
}
