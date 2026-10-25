// R21 (owner 2026-10-25): every memory number says what it is. Whole machine:
// physical memory used / total with a percentage, the compressor's share of it,
// swap used / the swap files' current total. Per owner: "占用" (phys_footprint,
// Activity Monitor's Memory column) split into an estimate of the part in physical
// memory, min(RSS, footprint), and the rest, compressed or swapped out. RSS counts
// shared pages, so both parts are estimates; the split is given only when the
// footprint and the RSS were read in full. The sidebar adds those estimates (spike
// 2026-10-25 on the owner's Mac: footprints 36.9G, RSS 9.0G, capped 8.4G, physical
// memory used 19.0G of 24G). app/DESIGN.md §12.
import Foundation

public enum MemoryWording {
    static func percent(_ part: UInt64, of total: UInt64) -> String {
        total == 0 ? "—" : "\(Int((Double(part) / Double(total) * 100).rounded()))%"
    }

    /// "物理内存 18.9G / 24.0G · 79%"
    public static func physical(_ h: Host, _ language: Language) -> String {
        L10n.format(.memPhysicalLine, language, Format.bytes(h.memUsedBytes), Format.bytes(h.memTotalBytes),
                    percent(h.memUsedBytes, of: h.memTotalBytes))
    }

    /// "18.9G / 24.0G · 79%", for a place whose label already says physical memory.
    public static func physicalValue(_ h: Host) -> String {
        "\(Format.bytes(h.memUsedBytes)) / \(Format.bytes(h.memTotalBytes)) · \(percent(h.memUsedBytes, of: h.memTotalBytes))"
    }

    /// "9.5G / 10.0G · 95%", or the used amount alone without a total, under a swap label.
    public static func swapValue(_ h: Host) -> String {
        guard let total = h.swapTotalBytes, total > 0 else { return Format.bytes(h.swapUsedBytes) }
        return "\(Format.bytes(h.swapUsedBytes)) / \(Format.bytes(total)) · \(percent(h.swapUsedBytes, of: total))"
    }

    /// "其中压缩 9.1G": the compressor's pages are part of physical memory used.
    public static func compressed(_ h: Host, _ language: Language) -> String {
        L10n.format(.memCompressedOf, language, Format.bytes(h.compressedBytes))
    }

    /// "swap 9.5G / 10.0G · 95%"; without the total (an older core) only what is used.
    public static func swap(_ h: Host, _ language: Language) -> String {
        guard let total = h.swapTotalBytes, total > 0 else {
            return L10n.format(.memSwapUsed, language, Format.bytes(h.swapUsedBytes))
        }
        return L10n.format(.memSwapLine, language, Format.bytes(h.swapUsedBytes), Format.bytes(total),
                           percent(h.swapUsedBytes, of: total))
    }

    /// Main window toolbar: the three in one line.
    public static func hostLine(_ h: Host, _ language: Language) -> String {
        [physical(h, language), compressed(h, language), swap(h, language)].joined(separator: " · ")
    }

    /// Hover text for the host numbers: how the three relate.
    public static func hostHelp(_ language: Language) -> String { L10n.text(.memHostHelp, language) }

    /// One owner's memory: an estimate of the part in physical memory, min(RSS, footprint)
    /// (RSS counts shared pages), the rest compressed or swapped out, and the footprint.
    /// No split when some processes' footprint or RSS could not be read: the footprint
    /// is then only a lower bound, and a difference taken from it would claim too much.
    public struct Split: Equatable, Sendable {
        public var physical: UInt64?
        public var outside: UInt64?
        public var total: UInt64
    }

    public static func split(_ o: Owner) -> Split {
        guard o.unknownMemCount == 0, let resident = o.reclaimablePhysical?.bytes else {
            return Split(physical: nil, outside: nil, total: o.memBytes)
        }
        let physical = min(resident, o.memBytes)
        return Split(physical: physical, outside: o.memBytes - physical, total: o.memBytes)
    }

    /// The detail line under the header, with the header's precision ("≥" and its hover):
    /// "物理内存约 7.5G · 压缩或 swap 约 2.2G · 合计（占用）9.7G";
    /// "合计（占用）≥ 4.2G · 有进程读不到，拆不出物理内存和压缩/swap";
    /// "合计（占用）读不到"; "合计（占用）9.7G · 物理内存部分读不到".
    public static func splitLine(_ o: Owner, _ language: Language) -> Format.Memory {
        let s = split(o)
        let total = Format.memory(o.memBytes, unknown: o.unknownMemCount, language)
        if o.unknownMemCount > 0 {
            if o.memBytes == 0 { return Format.Memory(text: L10n.text(.memSplitNoneRead, language), help: total.help) }
            return Format.Memory(text: L10n.format(.memSplitPartial, language, total.text), help: total.help)
        }
        guard let physical = s.physical, let outside = s.outside else {
            return Format.Memory(text: L10n.format(.memSplitUnknown, language, total.text), help: nil)
        }
        return Format.Memory(text: L10n.format(.memSplit, language, Format.bytes(physical), Format.bytes(outside), total.text),
                             help: nil)
    }

    /// Sidebar total for a kind of owners: the physical memory of each (nil when its RSS
    /// could not be read). A lower bound "≥" when some are nil, "—" when all are.
    public static func physicalTotal(_ values: [UInt64?]) -> String {
        let read = values.compactMap { $0 }
        if values.isEmpty { return Format.bytes(0) }
        if read.isEmpty { return "—" }
        return (read.count < values.count ? "≥ " : "") + Format.bytes(read.reduce(0, +))
    }

    /// Core verdict sentences (verdict/rules.rs, frozen; the CLI and JSON keep them as they
    /// are) carry an owner's memory as "占 9.70 GiB", "+500 MiB": in the GUI the amount gets
    /// the app's units and the footprint label (review round 8 B:B1). "占"/"using" before
    /// it becomes the label; otherwise the label follows the amount. Already labelled
    /// amounts only change units.
    public static func verdict(_ text: String, _ language: Language) -> String {
        // No bare "K": a name like "4K Video Downloader" is no amount (review round 8 B [risk]).
        let pattern = #"(占\s?|using\s)?([+\-−]?)(\d+(?:\.\d+)?)\s?(GiB|MiB|KiB|G|M)(?![A-Za-z])"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return text }
        let source = text as NSString
        var output = "", last = 0
        let label = L10n.text(.memFootprintInline, language)
        for match in regex.matches(in: text, range: NSRange(location: 0, length: source.length)) {
            output += source.substring(with: NSRange(location: last, length: match.range.location - last))
            last = match.range.location + match.range.length
            let group = { (i: Int) in match.range(at: i).location == NSNotFound ? "" : source.substring(with: match.range(at: i)) }
            let scale: Double = switch group(4) {
            case "GiB", "G": 1_073_741_824
            case "MiB", "M": 1_048_576
            default: 1024
            }
            let amount = group(2) + Format.bytes(UInt64(((Double(group(3)) ?? 0) * scale).rounded()))
            // Room for the label itself plus a few characters ("footprint " is 10).
            let before = output.suffix(label.count + 4).lowercased()
            if before.contains(label.lowercased()) {
                output += amount
            } else if !group(1).isEmpty {
                output += label + " " + amount
            } else {
                output += amount + L10n.text(.memFootprintAfter, language)
            }
        }
        return output + source.substring(from: last)
    }
}

