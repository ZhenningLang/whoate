// Main window right column: one owner's details, a process group, or the
// system overview when nothing is selected. Cards are GroupBoxes on the
// window background (content layer, no glass: app/DESIGN.md §4). Which cards
// exist and what they say comes from WhoateKit's Detail.
import AppKit
import Charts
import SwiftUI
import WhoateKit

struct DetailView: View {
    @Bindable var model: AppModel
    var body: some View {
        let language = model.language
        let row = model.selectedOwnerID.flatMap { id in model.rows(limit: .max).first { $0.id == id } }
        let owner = row.flatMap { model.owner($0.id) }
        VStack(spacing: 0) {
            ScrollViewReader { proxy in
                ScrollView {
                    VStack(alignment: .leading, spacing: 16) {
                        if let row, let owner {
                            OwnerDetail(row: row, owner: owner, language: language, procs: model.procs(owner.id),
                                        report: model.report, select: { model.select($0) })
                        } else if let row, let swarm = model.report?.swarms.first(where: { $0.id == row.id }) {
                            SwarmDetail(row: row, swarm: swarm, owners: model.report?.owners ?? [], language: language,
                                        select: { model.select($0) })
                        } else {
                            OverviewDetail(host: model.report?.host, language: language)
                        }
                    }
                    .padding(20)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                // The visible part of the scrolled content (automation layout check).
                .rowFrame("detail", "viewport")
                // Screenshots only (WHOATE_UI_DETAIL_SCROLL): bring a card below the fold into view,
                // whether the selection came before or after this view.
                .onChange(of: model.selectedOwnerID) { automationScroll(proxy) }
                .onAppear { automationScroll(proxy) }
            }
            // Actions stay in view at the bottom of the column, whatever the scroll position.
            if let owner {
                Divider()
                OwnerActions(model: model, owner: owner).padding(.horizontal, 20).padding(.vertical, 12)
                    .rowFrame("detail", "buttons")
            }
        }
    }
}

extension DetailView {
    func automationScroll(_ proxy: ScrollViewProxy) {
        guard let card = WhoateApp.automation.detailScroll, model.selectedOwnerID != nil else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { proxy.scrollTo(card, anchor: .top) }
    }
}

/// Kind line, title, the panel's pill and numbers: at least what the panel row shows.
struct DetailHeaderView: View {
    let header: DetailHeader
    let row: PanelRow
    let language: Language
    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(header.kindLine).font(.callout).foregroundStyle(.secondary).lineLimit(1).truncationMode(.middle)
            Text(header.title).font(.title2.weight(.semibold)).lineLimit(2)
            Text(header.subtitle).font(.callout).foregroundStyle(.secondary).lineLimit(2)
            // Review 10-29: the split stays on one line; where the column is too narrow it moves under the numbers.
            ViewThatFits(in: .horizontal) {
                HStack(spacing: 12) {
                    Pill(row: row)
                    HStack(alignment: .firstTextBaseline, spacing: 4) { footprint; inline }
                    cpu
                }
                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 12) { Pill(row: row); footprint; cpu }
                    inline
                }
            }
            if let split = header.memorySplit {
                VStack(alignment: .leading, spacing: 2) {
                    Text(split.text).font(.callout).monospacedDigit().help(split.help ?? "")
                    Text(L10n.text(.memSplitNote, language)).font(.caption).foregroundStyle(.secondary)
                }
            }
            if let verdict = header.verdict {
                Text(verdict).font(.callout).fixedSize(horizontal: false, vertical: true)
            }
        }
    }
}

extension DetailHeaderView {
    // R21: the number says what it is.
    private var footprint: some View {
        HStack(alignment: .firstTextBaseline, spacing: 4) {
            Text(L10n.text(.memFootprint, language)).font(.callout).foregroundStyle(.secondary)
            ValueText(row: row, sort: .memory, language: language)
        }
    }
    // Owner 2026-10-28: "占用 5.0G（物理内存约 656M + 压缩/swap 约 4.4G）".
    @ViewBuilder private var inline: some View {
        if let inline = MemoryWording.inline(row, language) {
            Text(inline).font(.callout).monospacedDigit().foregroundStyle(.secondary).fixedSize()
        }
    }
    private var cpu: some View {
        HStack(spacing: 4) { Text("CPU"); MemoryText(memory: row.cpu(language)) }
            .font(.callout).foregroundStyle(.secondary)
    }
}

struct OwnerDetail: View {
    let row: PanelRow
    let owner: Owner
    let language: Language
    /// The owner's processes from whoate_why; empty until it arrives (or when replaying).
    let procs: [ProcBrief]
    let report: Report?
    /// Selects another owner (a pane's session) through AppModel.select.
    var select: (String) -> Void

    var body: some View {
        DetailHeaderView(header: Detail.header(owner, row: row, language: language), row: row, language: language)
        ForEach(Array(Detail.cards(owner, history: row.history, catalog: Advice.catalog, language: language,
                                   procs: procs, report: report).enumerated()),
                id: \.offset) { _, card in
            switch card {
            case .session(let session): sessionCard(session)
            case .paneSessions(let sessions): paneSessionsCard(sessions)
            case .system(let system): systemCard(system)
            case .relation(let relation): relationCard(relation).id("relation")
            case .curve:
                box(L10n.text(.curve30, language)) {
                    HistoryChart(points: row.history, color: Palette.tint(row.level), language: language)
                        .frame(height: 140)
                        .rowFrame("detail", "trend")
                }
                // R11: CPU under memory, same 30 minutes; unreadable points are holes, never 0.
                box(L10n.text(.curveCPU30, language)) {
                    HistoryChart(points: row.history, color: Palette.tint(row.level), language: language, metric: .cpu)
                        .frame(height: 100)
                        .rowFrame("detail", "trendCPU")
                }
                .id("cpu")
            case .parts(let parts): partsCard(parts)
            case .folders(let groups): foldersCard(groups)
            case .facts(let facts, let linuxNote): factsCard(facts, linuxNote: linuxNote)
            case .advice(let items): adviceCard(items)
            }
        }
    }

    private func box<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        GroupBox {
            content().padding(8).frame(maxWidth: .infinity, alignment: .leading)
        } label: { Text(title).font(.headline) }
    }

    private func sessionCard(_ session: SessionCard) -> some View {
        box(L10n.text(.session, language)) {
            Grid(alignment: .leadingFirstTextBaseline, horizontalSpacing: 16, verticalSpacing: 8) {
                ForEach(Array(session.fields.enumerated()), id: \.offset) { _, field in
                    GridRow {
                        Text(field.label).foregroundStyle(.secondary)
                        Text(field.value).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                    }
                }
                if session.prompt != nil || session.reply != nil {
                    GridRow {
                        Text(L10n.text(.conversation, language)).foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 4) {
                            if let text = session.prompt { quote("User", text) }
                            if let text = session.reply { quote("Assistant", text) }
                        }
                    }
                }
                GridRow {
                    Text(L10n.text(.confidence, language)).foregroundStyle(.secondary)
                    Pill(text: session.confidence, level: .ok)
                }
            }
            .font(.callout)
        }
    }

    private func quote(_ who: String, _ text: String) -> some View {
        Text("\(Text(who + ": ").fontWeight(.semibold))\(text)")
            .lineLimit(3).fixedSize(horizontal: false, vertical: true)
    }

    /// Sessions running in this pane (R3); a click opens the session's own details.
    private func paneSessionsCard(_ sessions: [PaneSession]) -> some View {
        box(L10n.text(.paneSessions, language) + L10n.text(.memCardSuffix, language)) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(sessions.enumerated()), id: \.offset) { index, session in
                    if index > 0 { Divider() }
                    Button { select(session.ownerID) } label: {
                        HStack(spacing: 8) {
                            Text(session.title).lineLimit(1).truncationMode(.middle)
                            Text(session.tool).foregroundStyle(.secondary)
                            Spacer(minLength: 8)
                            MemoryText(memory: session.memory).foregroundStyle(.secondary)
                            Image(systemName: "chevron.right").foregroundStyle(.tertiary)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain).font(.callout).padding(.vertical, 6)
                }
            }
        }
    }

    /// The restart command is selectable text only (R6): whoate never runs it.
    private func systemCard(_ system: SystemCard) -> some View {
        box(L10n.text(.systemCard, language)) {
            VStack(alignment: .leading, spacing: 8) {
                if let command = system.command {
                    Text(command).font(.callout.monospaced()).textSelection(.enabled)
                } else {
                    Text(L10n.text(.restartNotRun, language)).foregroundStyle(.secondary)
                }
                Text(system.permissions)
                Text(system.launchd)
                Text(system.sideEffects)
                Text(system.safety).foregroundStyle(.secondary)
            }
            .font(.callout).fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The process tree, top-down (R10): where the owner came from, its parents,
    /// its processes with what they are for and their memory; past the limit one
    /// folded line. Then how whoate decided, plain words only.
    private func relationCard(_ relation: Relation) -> some View {
        box(L10n.text(.relation, language) + L10n.text(.memCardSuffix, language)) {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(relation.lines.enumerated()), id: \.offset) { _, line in relationLine(line) }
                if let summary = relation.restSummary {
                    DisclosureGroup(summary) {
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(Array(relation.rest.enumerated()), id: \.offset) { _, line in relationLine(line) }
                        }
                    }
                    .foregroundStyle(.secondary)
                }
                Text(L10n.format(.relationHow, language, relation.how)
                     + (relation.guess ? (language == .zh ? "" : " ") + L10n.text(.relationGuess, language) : ""))
                    .foregroundStyle(.secondary).fixedSize(horizontal: false, vertical: true).padding(.top, 4)
            }
            .font(.callout)
            .rowFrame("detail", "relation")
        }
    }

    private func relationLine(_ line: RelationLine) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            // Tree guides in the system font; one step is 16 pt (app/DESIGN.md §2 spacing).
            if line.depth > 0 {
                Text("└─").foregroundStyle(.tertiary).padding(.leading, CGFloat(line.depth - 1) * 16)
            }
            Text(line.name).lineLimit(1).truncationMode(.middle)
            if let role = line.role { Text(role).foregroundStyle(.secondary).lineLimit(1).truncationMode(.tail) }
            Spacer(minLength: 8)
            if line.exited { Text(L10n.text(.relationExited, language)).foregroundStyle(.secondary) }
            if let memory = line.memory { MemoryText(memory: memory).foregroundStyle(.secondary) }
            if line.marked { Text(L10n.text(.relationMarked, language)).foregroundStyle(Color.accentColor) }
        }
    }

    private func partsCard(_ parts: [Part]) -> some View {
        box(L10n.text(owner.agent != nil ? .processesInSession : .parts, language) + L10n.text(.memCardSuffix, language)) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(parts.enumerated()), id: \.offset) { index, part in
                    if index > 0 { Divider() }
                    HStack(spacing: 8) {
                        Text(part.displayName)
                            .lineLimit(1).truncationMode(.middle)
                        Spacer(minLength: 8)
                        MemoryText(memory: Format.memory(part.memBytes, unknown: part.unknownMemCount, language))
                            .foregroundStyle(.secondary)
                    }
                    .font(.callout).padding(.vertical, 6)
                }
            }
        }
    }

    /// The Codex app's processes without a session, by working folder (R7).
    private func foldersCard(_ groups: [FolderGroup]) -> some View {
        box(L10n.text(.byFolder, language) + L10n.text(.memCardSuffix, language)) {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
                    if index > 0 { Divider() }
                    HStack(spacing: 8) {
                        Text(group.folder).lineLimit(1).truncationMode(.middle)
                        Text(L10n.format(.processCount, language, "\(group.processes)")).foregroundStyle(.secondary)
                        Spacer(minLength: 8)
                        MemoryText(memory: Format.memory(group.memBytes, unknown: group.unknown, language))
                            .foregroundStyle(.secondary)
                    }
                    .font(.callout).padding(.vertical, 6)
                }
            }
        }
    }

    private func factsCard(_ facts: [Fact], linuxNote: Bool) -> some View {
        box(L10n.text(.facts, language)) {
            VStack(alignment: .leading, spacing: 4) {
                ForEach(Array(facts.enumerated()), id: \.offset) { _, fact in
                    HStack {
                        Text(fact.label).foregroundStyle(.secondary)
                        Spacer(minLength: 8)
                        Text(Self.text(fact.value)).monospacedDigit().textSelection(.enabled)
                    }
                }
                if linuxNote { Text(L10n.text(.linuxNote, language)).font(.caption).foregroundStyle(.secondary) }
            }
            .font(.callout)
        }
    }

    static func text(_ value: FactValue) -> String {
        switch value {
        case .bool(let v): String(v)
        case .count(let v): String(v)
        case .number(let v): String(format: "%g", v)
        case .text(let v): v
        case .strings(let v): v.joined(separator: ", ")
        }
    }

    private func adviceCard(_ items: [Advice.Item]) -> some View {
        box(L10n.text(.advice, language)) {
            VStack(alignment: .leading, spacing: 12) {
                ForEach(items, id: \.id) { item in
                    VStack(alignment: .leading, spacing: 4) {
                        if let cause = item.cause { Text(L10n.format(.adviceCause, language, cause)) }
                        if let advice = item.advice { Text(L10n.format(.adviceAdvice, language, advice)) }
                        if item.cause == nil { Text(item.id) }
                        if let source = item.source, let url = URL(string: source) {
                            Link(url.host() ?? source, destination: url).font(.subheadline)
                        }
                    }
                    .font(.callout).fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }
}

/// Only three actions (R6): jump to the pane, copy details for the user's AI
/// (R4), and the red close alone at the trailing edge (R2, HIG Buttons).
struct OwnerActions: View {
    @Bindable var model: AppModel
    let owner: Owner
    private var language: Language { model.language }

    var body: some View {
        let actions = Detail.actions(owner)
        HStack(spacing: 8) {
            if actions.jumpShown {
                // tmux runs off the main thread: a stuck server never freezes the window.
                // Outside tmux the button stays, greyed, with the reason beside it (U-DEC-6).
                Button(L10n.text(.jumpToPane, language)) {
                    guard let pane = actions.pane else { return }
                    let id = owner.id
                    Task { model.finishJump(await TmuxJump.perform(pane: pane, language: language), for: id) }
                }
                .disabled(actions.pane == nil)
                if actions.pane == nil {
                    Text(L10n.text(.notInTmux, language)).font(.callout).foregroundStyle(.secondary)
                }
            }
            Button(L10n.text(.copyInfo, language)) {
                copy(CopyInfo.text(owner: owner, procs: model.procs(owner.id), report: model.report, language: language))
                model.markCopied(owner.id)
            }
            if let notice = model.notice {
                Text(notice).font(.callout).foregroundStyle(.secondary).lineLimit(2)
            }
            Spacer(minLength: 16)
            // Destructive and red, never the default button; it only opens the confirmation (U-BEH-1).
            Button(L10n.format(.closeProcesses, language, "\(actions.close)"), role: .destructive) {
                model.requestClose(owner.id)
            }
            .buttonStyle(.borderedProminent).tint(.red)
            .disabled(actions.closeDisabled)
            .help(actions.closeDisabled ? L10n.text(.systemDoNotClose, language) : "")
        }
        // Button titles are never truncated (app/DESIGN.md §11).
        .fixedSize(horizontal: true, vertical: false)
    }
}

/// Puts text on the pasteboard; nothing is ever run.
func copy(_ text: String) {
    NSPasteboard.general.clearContents()
    NSPasteboard.general.setString(text, forType: .string)
}

struct SwarmDetail: View {
    let row: PanelRow
    let swarm: Swarm
    let owners: [Owner]
    let language: Language
    /// Opens a user's own details through AppModel.select (the R9 path).
    var select: (String) -> Void

    var body: some View {
        DetailHeaderView(header: Detail.header(swarm, row: row, language: language), row: row, language: language)
        if Spread.isSpread(swarm) { spread } else { distribution }
    }

    /// R19: what it is in one sentence, who uses it, and what would help.
    @ViewBuilder private var spread: some View {
        let byID = Dictionary(owners.map { ($0.id, $0) }, uniquingKeysWith: { a, _ in a })
        let users = Spread.users(swarm, owners: byID, language: language)
        Text(Spread.summary(swarm, users: users, language: language)).font(.body).fixedSize(horizontal: false, vertical: true)
        GroupBox {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(users.enumerated()), id: \.offset) { index, user in
                    if index > 0 { Divider() }
                    if let id = user.id {
                        Button { select(id) } label: { userLine(user, link: true) }.buttonStyle(.plain)
                    } else {
                        userLine(user, link: false)
                    }
                }
            }
            .padding(.horizontal, 8)
        } label: { Text(L10n.text(.spreadUsers, language) + L10n.text(.memCardSuffix, language)).font(.headline) }
        Text(Spread.advice(users, owners: byID, language: language)).font(.callout).foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func userLine(_ user: Spread.User, link: Bool) -> some View {
        HStack(spacing: 12) {
            Text(user.name).lineLimit(1).truncationMode(.middle)
                .foregroundStyle(link ? Color.accentColor : Color.secondary)
            Spacer()
            Text(L10n.format(.spreadCount, language, "\(user.count)")).foregroundStyle(.secondary)
            Text(user.memBytes.map(Format.bytes) ?? "—").frame(minWidth: 52, alignment: .trailing)
            Text("CPU " + Format.cpu(user.cpuPct)).foregroundStyle(.secondary).frame(minWidth: 72, alignment: .trailing)
            // Kept on every line so the columns line up; only the lines that open show it.
            Image(systemName: "chevron.right").font(.caption).foregroundStyle(.tertiary).opacity(link ? 1 : 0)
        }
        .font(.callout).monospacedDigit().padding(.vertical, 6).contentShape(Rectangle())
    }

    /// A burst group: how many copies each owner started.
    private var distribution: some View {
        GroupBox {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(swarm.owners.sorted { $0.count > $1.count }.enumerated()), id: \.offset) { index, member in
                    if index > 0 { Divider() }
                    HStack {
                        Text(owners.first { $0.id == member.id }.map(Panel.title) ?? member.id)
                            .lineLimit(1).truncationMode(.middle)
                        Spacer()
                        Text(L10n.format(.copies, language, "\(member.count)")).monospacedDigit().foregroundStyle(.secondary)
                    }
                    .font(.callout).padding(.vertical, 6)
                }
            }
            .padding(.horizontal, 8)
        } label: { Text(L10n.text(.swarmOwners, language)).font(.headline) }
    }
}

struct OverviewDetail: View {
    let host: WhoateKit.Host?
    let language: Language
    var body: some View {
        Text(L10n.text(.overview, language)).font(.title2.weight(.semibold))
        Text(L10n.text(.noSelection, language)).font(.callout).foregroundStyle(.secondary)
        if let host {
            let overview = Detail.overview(host, language: language)
            GroupBox {
                VStack(alignment: .leading, spacing: 8) {
                    PressureBar(host: host)
                    grid(overview.memory)
                }
                .padding(8)
            } label: { Text(Panel.headline(host, language)).font(.headline) }
            if let title = overview.zonesTitle {
                GroupBox { grid(overview.zones).padding(8).frame(maxWidth: .infinity, alignment: .leading) }
                    label: { Text(title).font(.headline) }
            }
            if let gpu = overview.gpu {
                GroupBox {
                    Text(gpu).font(.callout).padding(8).frame(maxWidth: .infinity, alignment: .leading)
                } label: { Text(L10n.text(.gpuMemory, language)).font(.headline) }
            }
            if !overview.diagnostics.isEmpty {
                GroupBox {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(Array(overview.diagnostics.enumerated()), id: \.offset) { _, text in
                            Text(text).font(.callout).fixedSize(horizontal: false, vertical: true)
                        }
                    }
                    .padding(8).frame(maxWidth: .infinity, alignment: .leading)
                } label: { Text(L10n.text(.diagnostics, language)).font(.headline) }
            }
        }
    }

    private func grid(_ lines: [(String, String)]) -> some View {
        Grid(alignment: .leading, horizontalSpacing: 16, verticalSpacing: 4) {
            ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                GridRow {
                    Text(line.0).foregroundStyle(.secondary)
                    Text(line.1).monospacedDigit()
                }
            }
        }
        .font(.callout)
    }
}

/// Swift Charts: one line per continuous run, so a gap stays a gap. Shown when
/// any run in the last 30 minutes can be drawn (Format.drawable), even if the
/// newest stretch is missing. `metric` .cpu draws CPU percent (R11), the readable
/// part where some processes were unreadable (R12).
struct HistoryChart: View {
    let points: [HistoryPoint]
    let color: Color
    let language: Language
    var metric: SortKey = .memory
    @Environment(\.curveGapLimit) private var gapLimit
    var body: some View {
        if !Format.drawable(points, metric: metric, gapLimit: gapLimit) {
            Text(L10n.text(.notEnoughSamples, language)).font(.callout).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                Chart {
                    ForEach(Array(Format.runs(points, metric: metric, gapLimit: gapLimit).enumerated()), id: \.offset) { index, run in
                        ForEach(run, id: \.timestampS) { p in
                            LineMark(x: .value("t", Date(timeIntervalSince1970: TimeInterval(p.timestampS))),
                                     y: .value("value", Self.value(p, metric)), series: .value("run", index))
                                .foregroundStyle(color)
                                .lineStyle(StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
                        }
                    }
                }
                .chartYAxis {
                    AxisMarks(position: .leading) { value in
                        AxisGridLine()
                        AxisValueLabel {
                            let v = max(0, value.as(Double.self) ?? 0)
                            Text(metric == .cpu ? Format.cpu(v) : Format.bytes(UInt64(v)))
                        }
                    }
                }
            }
        }
    }

    static func value(_ p: HistoryPoint, _ metric: SortKey) -> Double {
        metric == .cpu ? p.cpuPct ?? p.cpuPctReadable ?? 0 : Double(p.memBytes)
    }
}
