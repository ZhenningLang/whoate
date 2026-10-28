// Three-column main window (app/DESIGN.md): sidebar of kinds and filters,
// the panel's own rows in the middle, details on the right. Selection, sort
// and the window/activation plumbing (MainWindows) are shared with the panel.
import AppKit
import SwiftUI
import WhoateKit

struct MainWindowView: View {
    @Bindable var model: AppModel
    let mainWindows: MainWindows
    var openSettings: () -> Void = {}

    private var language: Language { model.language }
    private var closing: Bool { model.close != .idle && model.closeOrigin == .main }
    /// While a signal is out the sheet stays up: the answer comes back to it.
    private var busy: Bool {
        switch model.close {
        case .running, .forcing: true
        default: false
        }
    }
    private var closeSheet: Binding<Bool> {
        Binding(get: { closing }, set: { if !$0 && closing { model.cancelClose() } })
    }

    var body: some View {
        NavigationSplitView {
            SidebarView(model: model, openSettings: openSettings)
                .navigationSplitViewColumnWidth(min: 180, ideal: 200, max: 260)
        } content: {
            OwnerListView(model: model)
                .navigationSplitViewColumnWidth(min: 320, ideal: 360, max: 480)
        } detail: {
            DetailView(model: model)
        }
        // R16: the close confirmation over the whole window, the three columns dimmed behind it,
        // and the toolbar too (AppKit's, outside SwiftUI's overlay).
        .overlay { if closing { CloseDim().rowFrame("close", "dim") } }
        .onChange(of: closing) { TitlebarDim.set(mainWindows.all.first, on: closing) }
        .sheet(isPresented: closeSheet) {
            CloseCard(model: model).frame(width: 440).interactiveDismissDisabled(busy)
                .coordinateSpace(.named(RowFrames.space))
                .onPreferenceChange(RowFrames.self) { if WhoateApp.automation.stateFile != nil { RowFrames.sheet = $0 } }
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
        .safeAreaInset(edge: .top, spacing: 0) { if model.firstLaunchBanner { FirstLaunchBanner(model: model) } }
        .frame(minWidth: 960, minHeight: 600)
        .environment(\.curveGapLimit, model.profile.gapLimit)
        .coordinateSpace(.named(RowFrames.space))
        .onPreferenceChange(RowFrames.self) { frames in
            // Its own file: the panel window writes WHOATE_UI_LAYOUT_FILE itself.
            if let path = WhoateApp.automation.layoutFile { RowFrames.write(frames, to: path + ".main") }
            if WhoateApp.automation.stateFile != nil { RowFrames.main = frames }
        }
        .background(RefreshVisibility { model.setVisible(main: $0) }.frame(width: 0, height: 0))
        .background(WindowReporter { window in
            mainWindows.register(window)
            automationMainWindow(window)
        }.frame(width: 0, height: 0))
    }
}

/// R16 (review round 7): the toolbar lives in AppKit's titlebar container, which a
/// SwiftUI overlay never reaches; the same dim goes over it while the close sheet is up.
enum TitlebarDim {
    static let id = NSUserInterfaceItemIdentifier("whoate.titlebarDim")

    static func container(_ window: NSWindow?) -> NSView? {
        var view = window?.standardWindowButton(.closeButton)?.superview
        while let current = view, !String(describing: type(of: current)).contains("TitlebarContainer") {
            view = current.superview
        }
        return view
    }

    @MainActor static func set(_ window: NSWindow?, on: Bool) {
        guard let titlebar = container(window) else { return }
        titlebar.subviews.first { $0.identifier == id }?.removeFromSuperview()
        guard on else { return }
        let dim = NSView(frame: titlebar.bounds)
        dim.identifier = id
        dim.wantsLayer = true
        dim.layer?.backgroundColor = NSColor.black.withAlphaComponent(0.4).cgColor
        dim.autoresizingMask = [.width, .height]
        titlebar.addSubview(dim, positioned: .above, relativeTo: nil)
    }

    /// Automation only: the dim's height over the titlebar, 0 when it is not there.
    @MainActor static func height(_ window: NSWindow) -> Int {
        Int(container(window)?.subviews.first { $0.identifier == id }?.frame.height ?? 0)
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
                // R21: physical memory used / total · %, the compressor's share of it, swap used / total · %,
                // in two short lines so the Memory/CPU control always keeps its place (review round 8 A:B1).
                VStack(alignment: .trailing, spacing: 0) {
                    Text(MemoryWording.physical(host, language) + " · " + MemoryWording.compressed(host, language))
                    Text(MemoryWording.swap(host, language))
                }
                .font(.caption).monospacedDigit().foregroundStyle(.secondary).lineLimit(1)
                .help(MemoryWording.hostHelp(language))
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
    var openSettings: () -> Void = {}
    var body: some View {
        let language = model.language
        let rows = model.rows(limit: .max)
        let selection = Binding<MainCategory?>(get: { model.category }, set: { if let c = $0 { model.category = c } })
        List(selection: selection) {
            // R23: the kinds' totals are their rows' footprints, as in the list.
            Section(L10n.text(.memSectionOwners, language)) {
                ForEach([MainCategory.all, .tmux, .agents, .apps, .orphans, .system], id: \.self) { item($0, rows) }
            }
            Section(L10n.text(.sectionFilters, language)) {
                ForEach([MainCategory.needsAction, .growing], id: \.self) { item($0, rows) }
            }
        }
        .listStyle(.sidebar)
        // Owner 2026-10-28: the version at the bottom left, settings at the bottom right.
        .safeAreaInset(edge: .bottom) {
            HStack {
                Text(AboutInfo.make(info: Bundle.main.infoDictionary, language: language).short)
                    .font(.caption).monospacedDigit().foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
                    .textSelection(.enabled)
                Spacer(minLength: 8)
                Button { openSettings() } label: { Image(systemName: "gearshape") }
                    .buttonStyle(.borderless).foregroundStyle(.secondary)
                    .help(L10n.text(.settingsTitle, language)).accessibilityLabel(L10n.text(.settingsTitle, language))
            }
            .padding(.horizontal, 16).padding(.vertical, 10)
        }
    }

    private func item(_ category: MainCategory, _ rows: [PanelRow]) -> some View {
        let total = model.report == nil ? nil : MainWindowModel.total(rows, category, model.language)
        return Label(category.title(model.language), systemImage: category.symbol)
            .badge(Text(total?.text ?? "").monospacedDigit())
            .help(total?.help ?? "")
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
                // R21: in memory order the number is the footprint, and the header says so.
                Text(model.sort == .memory ? L10n.format(.memListHeader, language, "\(rows.count)")
                     : L10n.format(.ownersCount, language, "\(rows.count)") + " · " + L10n.format(.sortedBy, language, metric))
                Spacer()
                Text(L10n.text(.last30Minutes, language))
            }
            .font(.subheadline).foregroundStyle(.secondary)
            .padding(.horizontal, 16).padding(.top, 8).padding(.bottom, model.sort == .memory ? 2 : 8)
            // R21: what the number is, defined on a line of its own (it does not fit beside the count).
            if model.sort == .memory {
                Text(L10n.text(.memPanelCaption, language)).font(.caption).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(.horizontal, 16).padding(.bottom, 8)
            }
            Divider()
            ScrollViewReader { proxy in
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
                .onChange(of: model.scrollRequest) { scroll(proxy) }
                .onAppear { scroll(proxy) }
            }
        }
    }

    /// R9: a selection whoate made (not a click here) is scrolled to the centre once.
    /// The category's rows are laid out first; the second try covers a window still opening.
    private func scroll(_ proxy: ScrollViewProxy) {
        guard let request = model.scrollRequest, request.serial != model.scrolledSerial else { return }
        model.scrolledSerial = request.serial
        for delay in [0.05, 0.4] {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { proxy.scrollTo(request.id, anchor: .center) }
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
                // Owner 2026-10-28: "total (physical + compressed/swap)", as the list header defines.
                if sort == .memory, let parts = MemoryWording.parts(row) {
                    Text(parts).font(.caption).monospacedDigit().foregroundStyle(.secondary).fixedSize()
                }
            }
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(row.subtitle).font(.callout).foregroundStyle(.secondary).lineLimit(1).truncationMode(row.source == .swarm ? .tail : .middle)
                Spacer(minLength: 8)
                Group {
                    if sort == .memory {
                        HStack(spacing: 4) { Text("CPU"); MemoryText(memory: row.cpu(language)) }
                    } else {
                        // R21: labelled like the CPU number in memory order.
                        HStack(spacing: 4) { Text(L10n.text(.memFootprint, language)); MemoryText(memory: row.memory(language)) }
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
    if automation.mainWindow, let width = automation.mainWidth {
        window.setContentSize(NSSize(width: width, height: window.contentLayoutRect.height))
    }
    if automation.mainWindow, let screen = NSScreen.main?.visibleFrame {
        window.setFrameOrigin(NSPoint(x: screen.maxX - window.frame.width - 8, y: screen.minY + 8))
    }
    guard let path = automation.windowFile else { return }
    let size = window.frame.size
    appendLine("main\t\(window.windowNumber)\t\(Int(size.width))\t\(Int(size.height))\t"
        + "\(window.backingScaleFactor)\t\(ProcessInfo.processInfo.processIdentifier)\t-\n",
        to: path, failure: "window_file_failed")
}

/// Phase 11 first launch, when no menu bar item could be pointed at: where whoate lives.
struct FirstLaunchBanner: View {
    let model: AppModel
    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: "menubar.arrow.up.rectangle").foregroundStyle(.secondary)
            Text(L10n.text(.firstLaunchBanner, model.language)).font(.callout)
            Spacer(minLength: 8)
            Button(L10n.text(.gotIt, model.language)) { model.firstLaunchBanner = false }
        }
        .padding(.horizontal, 16).padding(.vertical, 8)
        .background(.bar)
        .overlay(alignment: .bottom) { Divider() }
    }
}
