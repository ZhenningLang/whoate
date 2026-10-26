// Command line tab (Phase 11, research F7): is `whoate` on the user's PATH, and
// a symlink to the CLI inside whoate.app (Contents/Helpers/whoate: Contents/MacOS
// would hit Whoate on a case-insensitive disk). Never sudo, never a shell profile.
import Darwin
import Foundation

public enum CommandLineTool {
    public enum Source: String, Sendable { case app, homebrew, other }

    public enum Status: Equatable, Sendable {
        /// `path` is where PATH finds it; `version` is its `--version` output.
        case installed(path: String, source: Source, version: String?)
        /// A symlink on PATH whose target is gone (whoate.app moved or deleted).
        case broken(path: String)
        case notInstalled
        /// This whoate.app carries no CLI (a `swift run` or wrapper build).
        case noBundledCLI
    }

    public enum InstallResult: Equatable, Sendable {
        case installed(path: String)
        /// Something else named whoate is there; it is left alone.
        case conflict(path: String)
        /// No candidate is writable and on PATH: a command to run by hand.
        case manual(command: String)
    }

    /// The order the spec gives (D1 = a): Homebrew's bin, then ~/.local/bin.
    public static func candidates(home: String) -> [String] { ["/opt/homebrew/bin", home + "/.local/bin"] }

    /// The CLI inside the running app, when it has one.
    public static func bundledCLI(bundle: Bundle = .main) -> String? {
        let path = bundle.bundleURL.appendingPathComponent("Contents/Helpers/whoate").path
        return FileManager.default.isExecutableFile(atPath: path) ? path : nil
    }

    /// The first `whoate` along `pathDirs`. A dangling symlink is "broken".
    public static func detect(pathDirs: [String], bundled: String?,
                              version: (String) -> String? = { CommandLineTool.version(of: $0) }) -> Status {
        let fm = FileManager.default
        for dir in pathDirs {
            let path = (dir as NSString).appendingPathComponent("whoate")
            guard (try? fm.attributesOfItem(atPath: path)) != nil else { continue }
            if let link = try? fm.destinationOfSymbolicLink(atPath: path) {
                let target = resolve(link, relativeTo: dir)
                guard fm.fileExists(atPath: target) else { return .broken(path: path) }
                return .installed(path: path, source: source(target, bundled: bundled), version: version(path))
            }
            guard fm.isExecutableFile(atPath: path) else { continue }
            return .installed(path: path, source: source(path, bundled: bundled), version: version(path))
        }
        return bundled == nil ? .noBundledCLI : .notInstalled
    }

    static func resolve(_ link: String, relativeTo dir: String) -> String {
        link.hasPrefix("/") ? link : ((dir as NSString).appendingPathComponent(link) as NSString).standardizingPath
    }

    static func source(_ target: String, bundled: String?) -> Source {
        if target == bundled || target.hasSuffix("/whoate.app/Contents/Helpers/whoate") { return .app }
        if target.contains("/Cellar/") || target.hasPrefix("/opt/homebrew/") || target.hasPrefix("/usr/local/Homebrew/") { return .homebrew }
        return .other
    }

    /// Symlinks `whoate` in the first candidate that exists, is writable and is on PATH.
    /// Our own link, or a dangling one into a whoate.app (the app was moved), is replaced;
    /// anything else, a dangling link elsewhere included, is left alone (review C:B2).
    public static func install(bundled: String, candidates: [String], pathDirs: [String]) -> InstallResult {
        let fm = FileManager.default
        for dir in candidates {
            var isDir: ObjCBool = false
            guard fm.fileExists(atPath: dir, isDirectory: &isDir), isDir.boolValue,
                  access(dir, W_OK) == 0, pathDirs.contains(where: { same($0, dir) }) else { continue }
            let path = (dir as NSString).appendingPathComponent("whoate")
            if (try? fm.attributesOfItem(atPath: path)) != nil {
                guard let link = try? fm.destinationOfSymbolicLink(atPath: path) else { return .conflict(path: path) }
                let target = resolve(link, relativeTo: dir)
                let ours = target == bundled || (source(target, bundled: bundled) == .app && !fm.fileExists(atPath: target))
                guard ours else { return .conflict(path: path) }
                do { try fm.removeItem(atPath: path) } catch { return .conflict(path: path) }
            }
            do {
                try fm.createSymbolicLink(atPath: path, withDestinationPath: bundled)
                return .installed(path: path)
            } catch {
                continue
            }
        }
        return .manual(command: manualCommand(bundled))
    }

    public static func manualCommand(_ bundled: String) -> String {
        "mkdir -p ~/.local/bin && ln -sf '\(bundled.replacingOccurrences(of: "'", with: "'\\''"))' ~/.local/bin/whoate"
    }

    static func same(_ a: String, _ b: String) -> Bool {
        (a as NSString).standardizingPath == (b as NSString).standardizingPath
    }

    // MARK: The user's PATH

    static let marker = "__WHOATE_PATH__"

    /// A login, non-interactive shell (`-lc`): `-i` could auto-attach tmux from .zshrc.
    public static func shellArguments() -> [String] {
        ["-lc", "printf '\\n\(marker)%s\\n' \"$PATH\""]
    }

    /// The PATH line from shell output that may carry login noise before it.
    public static func parsePath(_ output: String) -> [String]? {
        guard let line = output.split(separator: "\n").last(where: { $0.hasPrefix(marker) }) else { return nil }
        let dirs = line.dropFirst(marker.count).split(separator: ":").map(String.init).filter { !$0.isEmpty }
        return dirs.isEmpty ? nil : dirs
    }

    /// The account's login shell (not the app's $SHELL).
    public static func loginShell() -> String {
        guard let entry = getpwuid(getuid()), let shell = entry.pointee.pw_shell else { return "/bin/zsh" }
        let value = String(cString: shell)
        return value.isEmpty ? "/bin/zsh" : value
    }

    /// The user's PATH from their login shell, 3 s at most; nil when it could not be read.
    public static func userPath(timeout: Double = 3) -> [String]? {
        run(loginShell(), shellArguments(), timeout: timeout).flatMap(parsePath)
    }

    // MARK: Running the CLI

    /// First line of `<path> --version`, 2 s at most.
    public static func version(of path: String) -> String? {
        run(path, ["--version"], timeout: 2)?.split(separator: "\n").first.map(String.init)
    }

    /// `whoate agent-guide` from the given CLI: the same bytes as `whoate agent-guide` (SKILL.md).
    public static func agentGuide(cli: String) -> String? { run(cli, ["agent-guide"], timeout: 5) }

    /// Runs blocking work (these processes) on a GCD thread, not Swift's small
    /// cooperative pool, which also runs the engine's ticks.
    public static func offPool<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
        await withCheckedContinuation { done in DispatchQueue.global(qos: .utility).async { done.resume(returning: work()) } }
    }

    /// stdout of a short-lived process, stdin from /dev/null; nil on failure or timeout.
    /// ceiling: after the child exits its output is read for at most 1 s more; a
    /// grandchild still holding stdout keeps one reader thread until it closes.
    static func run(_ executable: String, _ arguments: [String], timeout: Double) -> String? {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        let out = Pipe()
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        let done = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in done.signal() }
        do { try process.run() } catch { return nil }
        // Read while it runs: a full pipe would otherwise stall the child.
        final class Output: @unchecked Sendable { var data = Data() }
        let output = Output()
        let read = DispatchSemaphore(value: 0)
        DispatchQueue.global(qos: .utility).async {
            output.data = out.fileHandleForReading.readDataToEndOfFile()
            read.signal()
        }
        if done.wait(timeout: .now() + timeout) == .timedOut {
            process.terminate()
            _ = done.wait(timeout: .now() + 1)
            return nil
        }
        _ = read.wait(timeout: .now() + 1)
        guard process.terminationStatus == 0 else { return nil }
        return String(data: output.data, encoding: .utf8)
    }
}
