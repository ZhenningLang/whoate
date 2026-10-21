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
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    if let row, let owner {
                        OwnerDetail(row: row, owner: owner, language: language, procs: model.procs(owner.id),
                                    report: model.report, select: { model.select($0) })
                    } else if let row, let swarm = model.report?.swarms.first(where: { $0.id == row.id }) {
                        SwarmDetail(row: row, swarm: swarm, owners: model.report?.owners ?? [], language: language)
                    } else {
                        OverviewDetail(host: model.report?.host, language: language)
                    }
                }
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            // The visible part of the scrolled content (automation layout check).
            .rowFrame("detail", "viewport")
            // Actions stay in view at the bottom of the column, whatever the scroll position.
            if let owner {
                Divider()
                OwnerActions(model: model, owner: owner).padding(.horizontal, 20).padding(.vertical, 12)
                    .rowFrame("detail", "buttons")
            }
        }
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
            HStack(spacing: 12) {
                Pill(row: row)
                ValueText(row: row, sort: .memory, language: language)
                Text("CPU " + Format.cpu(row.cpuPct)).font(.callout).monospacedDigit().foregroundStyle(.secondary)
            }
            if let verdict = header.verdict {
                Text(verdict).font(.callout).fixedSize(horizontal: false, vertical: true)
            }
        }
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
            case .why(let why): whyCard(why)
            case .curve:
                box(L10n.text(.curve30, language)) {
                    HistoryChart(points: row.history, color: Palette.tint(row.level), language: language)
                        .frame(height: 140)
                        .rowFrame("detail", "trend")
                }
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
        box(L10n.text(.paneSessions, language)) {
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

    /// One sentence, the parent chain when the grouping is not obvious, and the
    /// evidence folded away (R8).
    private func whyCard(_ why: WhyText) -> some View {
        box(L10n.text(.whyHere, language)) {
            VStack(alignment: .leading, spacing: 8) {
                Text(why.sentence).fixedSize(horizontal: false, vertical: true)
                if let chain = why.chain {
                    Text(L10n.format(.whyChain, language, chain)).foregroundStyle(.secondary)
                        .textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                }
                DisclosureGroup(L10n.format(.evidenceDetails, language, "\(why.details.count)")) {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(Array(why.details.enumerated()), id: \.offset) { index, line in
                            if index > 0 { Divider() }
                            HStack(alignment: .firstTextBaseline, spacing: 8) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(line.title).font(.body.weight(.medium))
                                    Text(line.detail).foregroundStyle(.secondary)
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                                Spacer(minLength: 8)
                                Pill(text: line.confidence, level: .ok)
                            }
                            .padding(.vertical, 8)
                        }
                    }
                }
            }
            .font(.callout)
        }
    }

    private func partsCard(_ parts: [Part]) -> some View {
        box(L10n.text(owner.agent != nil ? .processesInSession : .parts, language)) {
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
        box(L10n.text(.byFolder, language)) {
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
    var body: some View {
        DetailHeaderView(header: Detail.header(swarm, row: row), row: row, language: language)
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

/// Swift Charts: one line per continuous run, so a gap stays a gap. Shown only
/// when the panel's curve function (Format.trend) would draw a line too.
struct HistoryChart: View {
    let points: [HistoryPoint]
    let color: Color
    let language: Language
    var body: some View {
        if Format.trend(points, width: 1, height: 1) == .insufficient {
            Text(L10n.text(.notEnoughSamples, language)).font(.callout).foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            VStack(alignment: .leading, spacing: 4) {
                Chart {
                    ForEach(Array(Format.runs(points).enumerated()), id: \.offset) { index, run in
                        ForEach(run, id: \.timestampS) { p in
                            LineMark(x: .value("t", Date(timeIntervalSince1970: TimeInterval(p.timestampS))),
                                     y: .value("bytes", Double(p.memBytes)), series: .value("run", index))
                                .foregroundStyle(color)
                                .lineStyle(StrokeStyle(lineWidth: 1.5, lineCap: .round, lineJoin: .round))
                        }
                    }
                }
                .chartYAxis {
                    AxisMarks(position: .leading) { value in
                        AxisGridLine()
                        AxisValueLabel { Text(Format.bytes(UInt64(max(0, value.as(Double.self) ?? 0)))) }
                    }
                }
            }
        }
    }
}
