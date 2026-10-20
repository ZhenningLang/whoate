// Jump to a session's tmux pane (U-DEC-6): switch the most recently active
// client to the pane, then bring its terminal app forward. Commands are
// argument arrays for tmux itself, never a shell line.
import AppKit
import Foundation

public enum TmuxJump {
    public struct Client: Equatable, Sendable {
        public var tty: String
        public var pid: Int32
        public var activity: Int64
    }

    public enum Outcome: Equatable, Sendable {
        /// Switched; the terminal app's pid when one was found on the client's parent chain.
        case jumped(terminal: Int32?)
        case invalidPane
        case noClient
        case failed(String)
    }

    /// (exit status, standard output) of `tmux <args>`.
    public typealias Runner = @Sendable ([String]) -> (Int32, String)

    /// Only tmux pane ids (`%` and digits) are ever passed on.
    public static func validPane(_ id: String) -> String? {
        id.count > 1 && id.first == "%" && id.dropFirst().allSatisfy(\.isASCII) && id.dropFirst().allSatisfy(\.isNumber)
            ? id : nil
    }

    /// Space-separated: without a UTF-8 locale (apps started by launchd) tmux
    /// prints a tab separator as `_` (measured). tty paths and numbers have no spaces.
    public static let listClients = ["list-clients", "-F", "#{client_tty} #{client_pid} #{client_activity}"]

    public static func commands(pane: String, tty: String) -> [[String]] {
        [["switch-client", "-c", tty, "-t", pane], ["select-window", "-t", pane], ["select-pane", "-t", pane]]
    }

    public static func clients(_ output: String) -> [Client] {
        output.split(separator: "\n").compactMap { line in
            let f = line.split(separator: " ", omittingEmptySubsequences: false)
            guard f.count == 3, let pid = Int32(f[1]), let activity = Int64(f[2]), !f[0].isEmpty else { return nil }
            return Client(tty: String(f[0]), pid: pid, activity: activity)
        }
    }

    /// The client with the latest activity.
    public static func choose(_ clients: [Client]) -> Client? {
        clients.max { ($0.activity, $1.tty) < ($1.activity, $0.tty) }
    }

    /// First process up the parent chain that is an app (the terminal).
    public static func terminal(from pid: Int32, parent: (Int32) -> Int32?, isApp: (Int32) -> Bool) -> Int32? {
        var current: Int32? = pid
        var seen = Set<Int32>()
        while let p = current, p > 1, seen.insert(p).inserted {
            if isApp(p) { return p }
            current = parent(p)
        }
        return nil
    }

    public static func jump(pane: String, run: Runner, parent: (Int32) -> Int32? = parentPID,
                            isApp: (Int32) -> Bool = { NSRunningApplication(processIdentifier: $0) != nil }) -> Outcome {
        guard let pane = validPane(pane) else { return .invalidPane }
        let (status, output) = run(listClients)
        guard status == 0 else { return .failed("list-clients exit \(status)") }
        guard let client = choose(clients(output)) else { return .noClient }
        for args in commands(pane: pane, tty: client.tty) {
            let (code, _) = run(args)
            if code != 0 { return .failed("\(args[0]) exit \(code)") }
        }
        return .jumped(terminal: terminal(from: client.pid, parent: parent, isApp: isApp))
    }

    public static func parentPID(_ pid: Int32) -> Int32? {
        var info = kinfo_proc()
        var size = MemoryLayout<kinfo_proc>.stride
        var mib: [Int32] = [CTL_KERN, KERN_PROC, KERN_PROC_PID, pid]
        guard sysctl(&mib, 4, &info, &size, nil, 0) == 0, size > 0 else { return nil }
        return info.kp_eproc.e_ppid
    }

    /// `tmux` found on the repaired PATH (completedPath), run with an argument array.
    public static func processRunner(environment: [String: String] = ProcessInfo.processInfo.environment) -> Runner {
        let path = completedPath(environment["PATH"])
        let tmux = path.split(separator: ":").map { "\($0)/tmux" }.first { FileManager.default.isExecutableFile(atPath: $0) }
        return { args in
            guard let tmux else { return (127, "") }
            let process = Process()
            process.executableURL = URL(fileURLWithPath: tmux)
            process.arguments = args
            let out = Pipe()
            process.standardOutput = out
            process.standardError = FileHandle.nullDevice
            process.standardInput = FileHandle.nullDevice
            do { try process.run() } catch { return (126, "") }
            let data = out.fileHandleForReading.readDataToEndOfFile()
            process.waitUntilExit()
            return (process.terminationStatus, String(decoding: data, as: UTF8.self))
        }
    }

    /// Jump, then bring the terminal app forward. The message for the user, or nil on success.
    /// The tmux calls run on a background task; only the activation is on the main actor.
    @MainActor public static func perform(pane: String, language: Language) async -> String? {
        let outcome = await Task.detached(priority: .userInitiated) { jump(pane: pane, run: processRunner()) }.value
        switch outcome {
        case .jumped(let terminal?):
            NSRunningApplication(processIdentifier: terminal)?.activate()
            return nil
        case .jumped(nil): return L10n.text(.noTerminalApp, language)
        case .noClient: return L10n.text(.noTmuxClient, language)
        case .invalidPane: return L10n.text(.notInTmux, language)
        case .failed(let message): return L10n.format(.jumpFailed, language, message)
        }
    }
}
