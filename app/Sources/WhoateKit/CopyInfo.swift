// "Copy details" (R4): plain text about one owner for the user's own AI to
// judge keep-or-close. Never conversation text: no first prompt or reply.
// Command lines come only from the core (whoate_why procs, already redacted).
import Foundation

public enum CopyInfo {
    // ceiling: 30 processes listed, the rest counted; enough for an AI to judge.
    // Raise it if owners report cut-off lists for large apps.
    public static let processLimit = 30

    public static func text(owner: Owner, procs: [ProcBrief], report: Report?, language: Language,
                            home: String = NSHomeDirectory()) -> String {
        let zh = language == .zh
        let colon = zh ? "：" : ": "
        var lines: [String] = []
        func add(_ key: L10nKey, _ value: String?) {
            if let value, !value.isEmpty { lines.append(L10n.text(key, language) + colon + value) }
        }
        let taken = report.flatMap { Panel.parseDate($0.takenAt) }.map(Format.localTime) ?? ""
        lines.append(L10n.format(.copyHeader, language, taken))

        let title = RowNames.title(owner, language)
        if let agent = owner.agent {
            add(.copyOwner, L10n.format(.copySession, language, RowNames.tool(agent.tool), title))
            add(.copyKind, L10n.format(.copyKindAgent, language, agent.tool))
        } else {
            add(.copyOwner, title)
            add(.copyKind, RowNames.kindLabel(owner.kind, language))
        }
        let memory = Format.memory(owner.memBytes, unknown: owner.unknownMemCount, language).text
        add(.copyMemory, memory + (owner.unknownMemCount > 0
            ? L10n.format(.copyUnknown, language, "\(owner.unknownMemCount)") : ""))
        add(.sortCPU, Format.cpu(owner.cpuPct))
        add(.copyCwd, Detail.folder(owner).map { Panel.abbreviate($0, home: home) })
        if let origin = owner.origin, let pane = origin.tmuxPane {
            var place = "pane \(pane)"
            if let window = origin.windowName { place += zh ? "，" : ", "; place += L10n.format(.copyWindow, language, window) }
            if origin.paneAlive == false { place += zh ? "，" : ", "; place += L10n.text(.paneClosed, language) }
            add(.copyTmux, place)
        }
        let why = Detail.why(owner, procs: procs, language: language)
        add(.copyReason, why.sentence + (why.chain.map { (zh ? "。" : ". ") + L10n.format(.whyChain, language, $0) } ?? ""))
        add(.copySuggestion, owner.verdict.level == .ok ? L10n.text(.normal, language) : owner.verdict.text)

        let members = procs.filter(\.inOwner).sorted { ($0.memBytes ?? 0, $1.pid) > ($1.memBytes ?? 0, $0.pid) }
        lines.append(L10n.format(.copyProcesses, language, "\(owner.pids.count)") + colon)
        if members.isEmpty {
            lines.append("  " + L10n.text(.copyNoArgv, language))
            lines.append("  " + owner.pids.map(String.init).joined(separator: " "))
        } else {
            let width = members.prefix(processLimit).map(\.comm.count).max() ?? 0
            for p in members.prefix(processLimit) {
                let argv = (p.argv ?? []).joined(separator: " ")
                lines.append("  \(p.pid)  \(p.comm.padding(toLength: width, withPad: " ", startingAt: 0))  \(argv)")
            }
            if members.count > processLimit {
                lines.append("  " + L10n.format(.copyMore, language, "\(members.count - processLimit)"))
            }
        }
        return lines.joined(separator: "\n")
    }
}
