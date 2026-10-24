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
        let cpu = Format.cpu(owner.cpuPct, readable: owner.cpuPctReadable, unknown: owner.unknownCPUCount, language)
        // "counted the rest" only when there is a rest: nothing readable is "—" with the count.
        let rest = owner.cpuPctReadable != nil
        add(.sortCPU, cpu.text + (cpu.help.map {
            zh ? "（\($0)\(rest ? "，只计读得到的" : "")）" : " (\($0)\(rest ? "; counted the rest" : ""))"
        } ?? ""))
        add(.copyCwd, Detail.folder(owner).map { Panel.abbreviate($0, home: home) })
        if let origin = owner.origin, let pane = origin.tmuxPane {
            var place = "pane \(pane)"
            if let window = origin.windowName { place += zh ? "，" : ", "; place += L10n.format(.copyWindow, language, window) }
            if origin.paneAlive == false { place += zh ? "，" : ", "; place += L10n.text(.paneClosed, language) }
            add(.copyTmux, place)
        }
        // The relation card's tree and sentence, then the raw evidence the card leaves out (R10).
        let relation = Detail.relation(owner, procs: procs, report: report, language: language)
        add(.copyReason, relation.how + (relation.guess ? (zh ? "" : " ") + L10n.text(.relationGuess, language) : ""))
        for line in relation.lines + relation.rest {
            let parts = [line.name, line.role, line.exited ? L10n.text(.relationExited, language) : nil, line.memory?.text]
            lines.append("  " + String(repeating: "  ", count: line.depth) + parts.compactMap { $0 }.joined(separator: "  "))
        }
        lines.append(L10n.text(.copyEvidence, language) + colon)
        for e in owner.evidence { lines.append("  \(e.kind)  \(e.detail)  (\(e.confidence.rawValue))") }
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
