import Foundation
import Network

// MARK: - Values

/// One directory entry as a host reports it.
nonisolated struct HostFileEntry: Sendable, Equatable {
    let name: String
    let path: String
    let isDirectory: Bool
    let isSymlink: Bool
    let size: UInt64
    let modifiedMs: Int64
    /// Remote listings carry the daemon's `git check-ignore` verdict; local listings leave it
    /// to the caller, which runs git itself.
    let ignored: Bool?
}

nonisolated struct HostFileStat: Sendable, Equatable {
    let exists: Bool
    let isDirectory: Bool
    let size: UInt64
    let modifiedMs: Int64

    static let missing = HostFileStat(exists: false, isDirectory: false, size: 0, modifiedMs: 0)

    var modificationDate: Date? {
        exists ? Date(timeIntervalSince1970: Double(modifiedMs) / 1000) : nil
    }
}

nonisolated struct HostFileData: Sendable {
    let data: Data
    let size: UInt64
    let modifiedMs: Int64
    /// The file is larger than the requested maximum; `data` holds only its start.
    let truncated: Bool
}

/// A git or gh invocation. Locally `executablePath` is run; remotely the daemon runs `program`.
nonisolated struct HostCommand: Sendable {
    var program: String
    var executablePath: String
    var args: [String]
    var cwd: String
    var stdin: Data? = nil
    var timeoutSeconds: TimeInterval? = nil
    var outputLimit: Int? = nil
    var outputFile: URL? = nil
}

nonisolated enum HostError: LocalizedError, Equatable {
    case pathNotAllowed(String)
    case conflict(String)
    case notFound(String)
    case tooLargeToSave
    case daemon(code: String, message: String)
    case unexpectedResponse(String)

    /// Saves travel in one request line, which the daemon caps at 1 MB.
    static let maxRemoteSaveBytes = 750_000

    var errorDescription: String? {
        switch self {
        case .pathNotAllowed(let message): message
        case .conflict: "The file changed on the remote machine since it was opened. Reload it before saving."
        case .notFound(let message): message
        case .tooLargeToSave: "This file is too large to save on a remote machine (over 750 KB)."
        case .daemon(_, let message): message
        case .unexpectedResponse(let type): "Unexpected response from the remote daemon: \(type)"
        }
    }

    static func from(code: String, message: String) -> HostError {
        switch code {
        case "path_not_allowed": .pathNotAllowed(message)
        case "conflict": .conflict(message)
        case "not_found": .notFound(message)
        default: .daemon(code: code, message: message)
        }
    }
}

// MARK: - Host

/// Where a project's files live and its git runs (MACH-007). Local projects use the
/// filesystem and processes directly; remote projects go through their daemon.
///
/// Every method blocks. A remote host makes a daemon round trip, so callers must stay off the
/// main thread for remote hosts (local hosts keep their historical synchronous behaviour).
nonisolated protocol ProjectHost: Sendable {
    var isLocal: Bool { get }
    /// Distinguishes hosts in caches keyed by path.
    var identity: String { get }

    /// Immediate children of a directory, hidden files included.
    func list(directory: String) throws -> [HostFileEntry]
    /// Every file and directory below `root`, up to `limit` entries.
    func listRecursive(root: String, limit: Int) throws -> (entries: [HostFileEntry], truncated: Bool)
    func stat(path: String) throws -> HostFileStat
    func read(path: String, maxBytes: Int) throws -> HostFileData
    @discardableResult
    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat
    func delete(path: String) throws
    func run(_ command: HostCommand) -> CommandResult
    /// Change notifications for directories, delivered on the main queue (debounced).
    func makeWatcher(onChange: @escaping @Sendable () -> Void) -> any HostDirectoryWatcher
}

nonisolated protocol HostDirectoryWatcher: AnyObject, Sendable {
    func watchDirectory(path: String)
    func unwatchDirectory(path: String)
    func stopAll()
}

nonisolated extension FileTreeWatcher: HostDirectoryWatcher {}

// MARK: - Local

nonisolated struct LocalProjectHost: ProjectHost {
    static let shared = LocalProjectHost()

    var isLocal: Bool { true }
    var identity: String { "local" }

    func list(directory: String) throws -> [HostFileEntry] {
        let fm = FileManager.default
        return try fm.contentsOfDirectory(atPath: directory).compactMap { name in
            Self.entry(name: name, path: (directory as NSString).appendingPathComponent(name))
        }
    }

    func listRecursive(root: String, limit: Int) throws -> (entries: [HostFileEntry], truncated: Bool) {
        let rootURL = URL(fileURLWithPath: root, isDirectory: true)
        guard
            let enumerator = FileManager.default.enumerator(
                at: rootURL, includingPropertiesForKeys: [.isDirectoryKey], options: [.skipsPackageDescendants])
        else { return ([], false) }
        var entries: [HostFileEntry] = []
        for case let url as URL in enumerator {
            if entries.count >= limit { return (entries, true) }
            if let entry = Self.entry(name: url.lastPathComponent, path: url.standardizedFileURL.path) {
                entries.append(entry)
            }
        }
        return (entries, false)
    }

    func stat(path: String) throws -> HostFileStat {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path) else { return .missing }
        return HostFileStat(
            exists: true,
            isDirectory: attributes[.type] as? FileAttributeType == .typeDirectory,
            size: (attributes[.size] as? NSNumber)?.uint64Value ?? 0,
            modifiedMs: Self.milliseconds(attributes[.modificationDate] as? Date))
    }

    func read(path: String, maxBytes: Int) throws -> HostFileData {
        let attributes = try FileManager.default.attributesOfItem(atPath: path)
        let handle = try FileHandle(forReadingFrom: URL(fileURLWithPath: path))
        defer { try? handle.close() }
        let data = try handle.read(upToCount: maxBytes + 1) ?? Data()
        let size = (attributes[.size] as? NSNumber)?.uint64Value ?? UInt64(data.count)
        return HostFileData(
            data: data.count > maxBytes ? data.prefix(maxBytes) : data, size: size,
            modifiedMs: Self.milliseconds(attributes[.modificationDate] as? Date), truncated: data.count > maxBytes)
    }

    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        let url = URL(fileURLWithPath: path)
        if let expectedModifiedMs, try stat(path: path).modifiedMs != expectedModifiedMs {
            throw HostError.conflict(path)
        }
        if createDirectories {
            try FileManager.default.createDirectory(
                at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        }
        try data.write(to: url, options: .atomic)
        return try stat(path: path)
    }

    func delete(path: String) throws {
        try FileManager.default.removeItem(atPath: path)
    }

    func makeWatcher(onChange: @escaping @Sendable () -> Void) -> any HostDirectoryWatcher {
        FileTreeWatcher(onChange: onChange)
    }

    static func milliseconds(_ date: Date?) -> Int64 {
        Int64(((date?.timeIntervalSince1970 ?? 0) * 1000).rounded(.down))
    }

    private static func entry(name: String, path: String) -> HostFileEntry? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path) else { return nil }
        let type = attributes[.type] as? FileAttributeType
        var isDirectory = type == .typeDirectory
        if type == .typeSymbolicLink {
            var targetIsDirectory: ObjCBool = false
            guard FileManager.default.fileExists(atPath: path, isDirectory: &targetIsDirectory) else { return nil }
            isDirectory = targetIsDirectory.boolValue
        }
        return HostFileEntry(
            name: name, path: path, isDirectory: isDirectory, isSymlink: type == .typeSymbolicLink,
            size: (attributes[.size] as? NSNumber)?.uint64Value ?? 0,
            modifiedMs: milliseconds(attributes[.modificationDate] as? Date), ignored: nil)
    }

    /// Run git or gh on this Mac. Moved unchanged from GitService.
    func run(_ command: HostCommand) -> CommandResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: command.executablePath)
        process.arguments = command.args
        process.currentDirectoryURL = URL(fileURLWithPath: command.cwd)
        let outputLimit = command.outputLimit
        let outputFile = command.outputFile

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        var fileHandle: FileHandle?
        if let outputFile {
            _ = FileManager.default.createFile(atPath: outputFile.path, contents: nil, attributes: [.posixPermissions: 0o600])
            do { fileHandle = try FileHandle(forWritingTo: outputFile) }
            catch { return CommandResult(stdout: "", stderr: error.localizedDescription, exitCode: -1) }
        }
        defer { try? fileHandle?.close() }
        if let fileHandle { process.standardOutput = fileHandle }
        else { process.standardOutput = stdoutPipe }
        process.standardError = stderrPipe
        let stdinPipe = command.stdin.map { _ in Pipe() }
        if let stdinPipe { process.standardInput = stdinPipe }

        do {
            try process.run()
        } catch {
            return CommandResult(stdout: "", stderr: error.localizedDescription, exitCode: -1)
        }
        if let stdinPipe, let input = command.stdin {
            stdinPipe.fileHandleForWriting.write(input)
            try? stdinPipe.fileHandleForWriting.close()
        }

        // Drain on dedicated threads: cooperative tasks may occupy every shared
        // Dispatch worker while waiting below, starving queued pipe readers.
        // Both streams must keep draining before waiting for process exit.
        nonisolated(unsafe) var stdoutData = Data()
        nonisolated(unsafe) var stderrData = Data()
        nonisolated(unsafe) var outputExceededLimit = false
        let group = DispatchGroup()

        if outputFile == nil {
            group.enter()
            Thread.detachNewThread {
                if let outputLimit {
                    // Fixed-size reads bound capture before decoding. Keep draining
                    // after termination so a full pipe cannot prevent process exit.
                    while true {
                        let chunk = stdoutPipe.fileHandleForReading.readData(ofLength: 16_384)
                        if chunk.isEmpty { break }
                        if outputExceededLimit { continue }
                        if chunk.count > outputLimit - stdoutData.count {
                            outputExceededLimit = true
                            stdoutData.removeAll(keepingCapacity: false)
                            process.terminate()
                        } else {
                            stdoutData.append(chunk)
                        }
                    }
                } else {
                    stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
                }
                group.leave()
            }
        }
        group.enter()
        Thread.detachNewThread {
            stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
            group.leave()
        }

        // Remote lookups and explicit overview previews have deadlines; legacy local APIs retain theirs.
        let timeout = command.timeoutSeconds.map { seconds in
            let work = DispatchWorkItem {
                if process.isRunning {
                    process.terminate()
                    DispatchQueue.global().asyncAfter(deadline: .now() + 1) {
                        if process.isRunning { kill(process.processIdentifier, SIGKILL) }
                    }
                }
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + seconds, execute: work)
            return work
        }
        var canceledAt: Date?
        while process.isRunning {
            if Task.isCancelled {
                if canceledAt == nil { canceledAt = Date(); process.terminate() }
                else if Date().timeIntervalSince(canceledAt!) > 1 { kill(process.processIdentifier, SIGKILL) }
            }
            Thread.sleep(forTimeInterval: 0.02)
        }
        process.waitUntilExit()
        timeout?.cancel()
        group.wait()

        let stdout = String(data: stdoutData, encoding: .utf8) ?? ""
        let stderr = String(data: stderrData, encoding: .utf8) ?? ""
        return CommandResult(
            stdout: stdout, stderr: stderr, exitCode: process.terminationStatus,
            outputExceededLimit: outputExceededLimit)
    }
}

// MARK: - Remote (daemon over SSH)

/// A project on another machine: files and git go through that machine's daemon
/// (`fs_*`, `subscribe_fs`, `git_exec`), which confines every path to its registered projects.
nonisolated struct DaemonProjectHost: ProjectHost {
    let client: DaemonClient
    let machineId: UUID

    var isLocal: Bool { false }
    var identity: String { "machine:" + machineId.uuidString.lowercased() }

    /// Upper bound for one listing; the daemon also enforces its own.
    static let maxListEntries = 20_000

    // Request construction is separate from sending so it can be checked against the spec.
    static func listRequest(directory: String) -> DaemonRequest {
        .fsList(path: directory, recursive: false, includeHidden: true, maxEntries: maxListEntries)
    }
    static func listRecursiveRequest(root: String, limit: Int) -> DaemonRequest {
        .fsList(path: root, recursive: true, includeHidden: true, maxEntries: limit)
    }
    static func readRequest(path: String, maxBytes: Int) -> DaemonRequest {
        .fsRead(path: path, maxBytes: maxBytes)
    }
    static func writeRequest(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool)
        -> DaemonRequest
    {
        .fsWrite(path: path, data: data, expectedModifiedMs: expectedModifiedMs, createDirs: createDirectories)
    }
    static func execRequest(_ command: HostCommand) -> DaemonRequest {
        .gitExec(
            cwd: command.cwd, program: command.program, args: command.args, stdin: command.stdin,
            timeoutMs: Int((command.timeoutSeconds ?? 30) * 1000))
    }

    func list(directory: String) throws -> [HostFileEntry] {
        try listing(Self.listRequest(directory: directory), requestedPath: directory).entries
    }

    func listRecursive(root: String, limit: Int) throws -> (entries: [HostFileEntry], truncated: Bool) {
        try listing(Self.listRecursiveRequest(root: root, limit: limit), requestedPath: root)
    }

    /// The daemon answers with canonical real paths (`/private/var/…` for `/var/…`). Re-root
    /// them under the path the caller asked for, so callers can keep deriving relative paths.
    static func rebase(_ path: String, canonicalRoot: String, requestedRoot: String) -> String {
        if path == canonicalRoot { return requestedRoot }
        let prefix = canonicalRoot.hasSuffix("/") ? canonicalRoot : canonicalRoot + "/"
        guard path.hasPrefix(prefix) else { return path }
        return (requestedRoot as NSString).appendingPathComponent(String(path.dropFirst(prefix.count)))
    }

    func stat(path: String) throws -> HostFileStat {
        switch try send(.fsStat(path: path)) {
        case .fsStatResult(let stat):
            return HostFileStat(exists: stat.exists, isDirectory: stat.isDir, size: stat.size, modifiedMs: stat.modifiedMs)
        case .error(let code, _) where code == "not_found":
            return .missing
        case let other:
            throw Self.failure(other)
        }
    }

    func read(path: String, maxBytes: Int) throws -> HostFileData {
        switch try send(Self.readRequest(path: path, maxBytes: maxBytes)) {
        case .fsReadResult(_, let data, let size, let modifiedMs, let truncated):
            return HostFileData(data: data, size: size, modifiedMs: modifiedMs, truncated: truncated)
        case let other:
            throw Self.failure(other)
        }
    }

    func write(path: String, data: Data, expectedModifiedMs: Int64?, createDirectories: Bool) throws -> HostFileStat {
        guard data.count <= HostError.maxRemoteSaveBytes else { throw HostError.tooLargeToSave }
        let request = Self.writeRequest(
            path: path, data: data, expectedModifiedMs: expectedModifiedMs, createDirectories: createDirectories)
        switch try send(request) {
        case .fsWriteResult(_, let size, let modifiedMs):
            return HostFileStat(exists: true, isDirectory: false, size: size, modifiedMs: modifiedMs)
        case let other:
            throw Self.failure(other)
        }
    }

    func delete(path: String) throws {
        switch try send(.fsDelete(path: path)) {
        case .ok: return
        case let other: throw Self.failure(other)
        }
    }

    func run(_ command: HostCommand) -> CommandResult {
        let response: DaemonResponse
        do {
            // The daemon enforces the command's own deadline; allow for transfer on top.
            response = try client.sendBlocking(
                Self.execRequest(command), timeout: (command.timeoutSeconds ?? 30) + 15)
        } catch {
            return CommandResult(stdout: "", stderr: error.localizedDescription, exitCode: -1)
        }
        return Self.commandResult(response, for: command)
    }

    /// A `git_exec` reply as a local run would report it. Output the daemon truncated is never
    /// passed off as complete: callers with an output limit see it exceeded, every other
    /// caller (status, snapshots, refs) gets a failure rather than a partial parse.
    static func commandResult(_ response: DaemonResponse, for command: HostCommand) -> CommandResult {
        switch response {
        case .gitExecResult(let exitCode, let stdout, let stderr, let truncated):
            let stderrText = String(data: stderr, encoding: .utf8) ?? ""
            if let outputLimit = command.outputLimit, stdout.count > outputLimit || truncated {
                return CommandResult(stdout: "", stderr: stderrText, exitCode: exitCode, outputExceededLimit: true)
            }
            if truncated {
                return CommandResult(
                    stdout: "", stderr: "Output of \(command.program) exceeded the remote daemon's limit",
                    exitCode: -1, outputExceededLimit: true)
            }
            if let outputFile = command.outputFile {
                guard
                    FileManager.default.createFile(
                        atPath: outputFile.path, contents: stdout, attributes: [.posixPermissions: 0o600])
                else {
                    return CommandResult(stdout: "", stderr: "Could not save remote output", exitCode: -1)
                }
                return CommandResult(stdout: "", stderr: stderrText, exitCode: exitCode)
            }
            return CommandResult(stdout: String(data: stdout, encoding: .utf8) ?? "", stderr: stderrText, exitCode: exitCode)
        case .error(_, let message):
            return CommandResult(stdout: "", stderr: message, exitCode: -1)
        default:
            return CommandResult(stdout: "", stderr: "Unexpected response from the remote daemon", exitCode: -1)
        }
    }

    func makeWatcher(onChange: @escaping @Sendable () -> Void) -> any HostDirectoryWatcher {
        DaemonDirectoryWatcher(client: client, onChange: onChange)
    }

    // MARK: Private

    private func send(_ request: DaemonRequest) throws -> DaemonResponse {
        try client.sendBlocking(request, timeout: 30)
    }

    private func listing(_ request: DaemonRequest, requestedPath: String) throws
        -> (entries: [HostFileEntry], truncated: Bool)
    {
        switch try send(request) {
        case .fsListResult(let canonicalRoot, let entries, let truncated):
            return (
                entries.map {
                    HostFileEntry(
                        name: $0.name,
                        path: Self.rebase($0.path, canonicalRoot: canonicalRoot, requestedRoot: requestedPath),
                        isDirectory: $0.isDir, isSymlink: $0.isSymlink, size: $0.size,
                        modifiedMs: $0.modifiedMs, ignored: $0.ignored)
                }, truncated
            )
        case let other:
            throw Self.failure(other)
        }
    }

    private static func failure(_ response: DaemonResponse) -> HostError {
        if case .error(let code, let message) = response { return .from(code: code, message: message) }
        return .unexpectedResponse("\(response)")
    }
}

// MARK: - Remote watching

/// Directory change notifications from a remote daemon (`subscribe_fs`). One recursive
/// subscription covers every watched directory below it, so expanding folders in a tree
/// does not open a connection per folder.
nonisolated final class DaemonDirectoryWatcher: HostDirectoryWatcher, @unchecked Sendable {
    private let client: DaemonClient
    private let onChange: @Sendable () -> Void
    private let lock = NSLock()
    private var subscriptions: [String: DaemonFsSubscription] = [:]
    private var debounce: DispatchWorkItem?
    private var stopped = false

    init(client: DaemonClient, onChange: @escaping @Sendable () -> Void) {
        self.client = client
        self.onChange = onChange
    }

    func watchDirectory(path: String) {
        lock.lock()
        defer { lock.unlock() }
        guard !stopped, !subscriptions.keys.contains(where: { path == $0 || path.hasPrefix($0 + "/") }) else { return }
        let subscription = DaemonFsSubscription(client: client, root: path)
        subscriptions[path] = subscription
        Task { [weak self] in
            await subscription.start { [weak self] in self?.scheduleChange() }
            // Ended for good (stopped, or the path is not allowed): drop the entry so a later
            // watchDirectory can subscribe again.
            self?.remove(subscription)
        }
    }

    private func remove(_ subscription: DaemonFsSubscription) {
        lock.lock()
        defer { lock.unlock() }
        if let key = subscriptions.first(where: { $0.value === subscription })?.key {
            subscriptions.removeValue(forKey: key)
        }
    }

    func unwatchDirectory(path: String) {
        lock.lock()
        let subscription = subscriptions.removeValue(forKey: path)
        lock.unlock()
        if let subscription { Task { await subscription.stop() } }
    }

    func stopAll() {
        lock.lock()
        stopped = true
        let all = Array(subscriptions.values)
        subscriptions.removeAll()
        debounce?.cancel()
        debounce = nil
        lock.unlock()
        for subscription in all { Task { await subscription.stop() } }
    }

    deinit {
        for subscription in subscriptions.values { Task { await subscription.stop() } }
    }

    private func scheduleChange() {
        lock.lock()
        defer { lock.unlock() }
        guard !stopped else { return }
        debounce?.cancel()
        let callback = onChange
        let work = DispatchWorkItem { callback() }
        debounce = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
    }
}

/// Reconnect delays for a stream: 1 s doubling to a 15 s cap, back to 1 s once subscribed.
nonisolated struct SubscriptionBackoff: Sendable {
    static let initial: Duration = .seconds(1)
    static let maximum: Duration = .seconds(15)

    private(set) var current = initial

    /// The delay before the next attempt.
    mutating func next() -> Duration {
        let delay = current
        current = min(current * 2, Self.maximum)
        return delay
    }

    mutating func reset() {
        current = Self.initial
    }
}

/// One `subscribe_fs` stream with reconnection, modeled after DaemonStatusSubscription.
/// Retries until stopped (the machine may be offline for hours), backing off to 15 s; a
/// successful subscribe resets the backoff.
actor DaemonFsSubscription {
    let root: String
    private let client: DaemonClient
    private var connection: NWConnection?
    private var stopped = false
    private var backoff = SubscriptionBackoff()
    private var subscribedAt: ContinuousClock.Instant?
    private static let healthyStream: Duration = .seconds(10)

    init(client: DaemonClient, root: String) {
        self.client = client
        self.root = root
    }

    func start(onEvent: @escaping @Sendable () -> Void) async {
        while !stopped {
            do {
                try await runLoop(onEvent: onEvent)
                break
            } catch is CancellationError {
                break
            } catch HostError.pathNotAllowed {
                break
            } catch {
                guard !stopped else { break }
                // A stream that lived a while was healthy: start over from the short delay.
                if let subscribedAt, ContinuousClock.now - subscribedAt >= Self.healthyStream { backoff.reset() }
                subscribedAt = nil
                do { try await Task.sleep(for: backoff.next()) } catch { break }
            }
        }
    }

    func stop() {
        stopped = true
        connection?.cancel()
        connection = nil
    }

    private func runLoop(onEvent: @escaping @Sendable () -> Void) async throws {
        let (conn, reader) = try await client.connect()
        connection?.cancel()
        connection = conn
        guard !stopped else {
            conn.cancel()
            return
        }
        try await DaemonClient.write(.subscribeFs(root: root, recursive: true), to: conn)
        let first = DaemonClient.parse(try await reader.readLine())
        switch first {
        case .fsSubscribed:
            // Backoff resets once the stream proves healthy (an event, or ten quiet
            // seconds): the daemon can follow `fs_subscribed` with `watch_failed`.
            subscribedAt = .now
        case .error(let code, let message):
            throw HostError.from(code: code, message: message)
        default:
            throw HostError.unexpectedResponse("\(first)")
        }
        while !stopped {
            switch DaemonClient.parse(try await reader.readLine()) {
            case .fsEvent:
                backoff.reset()
                onEvent()
            case .error(let code, let message):
                // `watch_failed` (or any error) ends this subscription; retry with backoff.
                throw HostError.daemon(code: code, message: message)
            default:
                break
            }
        }
    }
}
