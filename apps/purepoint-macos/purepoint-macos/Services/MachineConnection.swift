import Darwin
import Foundation

/// What the app knows about a machine's daemon through its forward.
nonisolated enum DaemonAvailability: Equatable, Sendable {
    case running
    /// SSH works but nothing answers on the remote socket. Never started automatically.
    case stopped
    /// The daemon speaks another protocol version; no other requests are made.
    case incompatible(Int)
}

/// One `health` probe over the forward.
nonisolated enum HealthProbe: Equatable, Sendable {
    case running
    case incompatible(Int)
    /// The daemon answered BUSY (at its connection limit): up, but not yet known compatible.
    case busy
    /// No answer, an error, or the deadline passed.
    case failed
}

/// Turns probes into what the machine reports. One slow or failed probe must not read as a
/// stopped daemon (that drops its projects), so only several failures in a row do; BUSY is
/// transient and changes nothing.
nonisolated struct HealthProbeTracker: Sendable {
    static let failuresBeforeStopped = 3

    private(set) var consecutiveFailures = 0

    /// The availability to report after `probe`, or nil to keep the last report and retry soon.
    mutating func record(_ probe: HealthProbe) -> DaemonAvailability? {
        switch probe {
        case .running:
            consecutiveFailures = 0
            return .running
        case .incompatible(let version):
            consecutiveFailures = 0
            return .incompatible(version)
        case .busy:
            return nil
        case .failed:
            consecutiveFailures += 1
            return consecutiveFailures >= Self.failuresBeforeStopped ? .stopped : nil
        }
    }
}

nonisolated enum MachineConnectionState: Equatable, Sendable {
    case disconnected
    case connecting
    case connected(DaemonAvailability)
    case offline(String)

    var isDaemonRunning: Bool { self == .connected(.running) }
}

/// The exact ssh command lines from machines.md. The system ssh supplies identity, agent,
/// Keychain and config (MACH-001); every value is one argv element, never a shell string.
nonisolated enum MachineSSH {
    static let sshPath = "/usr/bin/ssh"
    static let remoteSocket = "/.pu/daemon.sock"

    /// ssh destinations have no `host:port` form, so `user@host:port` becomes the
    /// `ssh://user@host:port` URI. Aliases and bare hosts pass through unchanged. Mirrors the
    /// CLI's `ssh_destination` (crates/pu-cli/src/machine_target.rs).
    static func destination(_ host: String) -> String {
        if host.hasPrefix("ssh://") { return host }
        guard let colon = host.lastIndex(of: ":") else { return host }
        let head = host[..<colon]
        let port = host[host.index(after: colon)...]
        guard !head.isEmpty, !head.contains(":"), !port.isEmpty, port.allSatisfy({ ("0"..."9").contains($0) })
        else { return host }
        return "ssh://" + host
    }

    /// Every ssh call opts out of connection sharing: a user's `ControlMaster` config would
    /// otherwise park the call on a shared master that outlives it.
    static let noMultiplexing = ["-o", "ControlMaster=no", "-o", "ControlPath=none"]

    /// `ssh -o BatchMode=yes -o ControlMaster=no -o ControlPath=none <ssh_host> 'printf %s "$HOME"'`
    static func resolveHomeArguments(sshHost: String) -> [String] {
        ["-o", "BatchMode=yes"] + noMultiplexing + [destination(sshHost), "printf %s \"$HOME\""]
    }

    /// `ssh -N -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o ServerAliveInterval=15
    ///  -o ServerAliveCountMax=3 -o StreamLocalBindUnlink=yes -o ControlMaster=no
    ///  -o ControlPath=none -L <local>:<home>/.pu/daemon.sock <ssh_host>`
    static func forwardArguments(localSocket: String, remoteHome: String, sshHost: String) -> [String] {
        [
            "-N", "-T",
            "-o", "BatchMode=yes",
            "-o", "ExitOnForwardFailure=yes",
            "-o", "ServerAliveInterval=15",
            "-o", "ServerAliveCountMax=3",
            "-o", "StreamLocalBindUnlink=yes",
        ] + noMultiplexing + [
            "-L", localSocket + ":" + remoteHome + remoteSocket,
            destination(sshHost),
        ]
    }

    /// Non-interactive SSH has a minimal PATH; the prefix lets `pu` resolve from the installed
    /// locations. `pu` then starts the `pu-engine` beside itself (or in `~/.pu/bin`), never one
    /// found on PATH. Wrapped in `/bin/sh -c` so a fish or csh login shell runs it too.
    static let startDaemonCommand =
        "/bin/sh -c 'PATH=\"$HOME/.pu/bin:$HOME/.cargo/bin:$PATH\"; export PATH; exec pu health'"

    /// `ssh -o ControlMaster=no -o ControlPath=none <ssh_host> '<startDaemonCommand>'`
    static func startDaemonArguments(sshHost: String) -> [String] {
        noMultiplexing + [destination(sshHost), startDaemonCommand]
    }

    struct Output: Sendable {
        let status: Int32
        let stdout: String
        let stderr: String
    }

    /// Run ssh to completion with a deadline. stdin is closed so ssh never waits on a prompt.
    /// Output is bounded, and the result never waits on pipes a lingering child (a
    /// `ControlPersist` master, say) keeps open: it is delivered at most a moment after ssh
    /// exits, and a kill follows the deadline. `processes` lets a disconnect end it early.
    static func run(
        _ arguments: [String], timeout: TimeInterval, processes: OneShotProcesses? = nil, executable: String = sshPath
    ) async -> Output {
        await withCheckedContinuation { (continuation: CheckedContinuation<Output, Never>) in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: executable)
            process.arguments = arguments
            process.standardInput = FileHandle.nullDevice
            let stdout = Pipe()
            let stderr = Pipe()
            process.standardOutput = stdout
            process.standardError = stderr
            let out = OutputCollector()
            let err = OutputCollector()
            let readers = DispatchGroup()
            for (pipe, collector) in [(stdout, out), (stderr, err)] {
                readers.enter()
                let handle = pipe.fileHandleForReading
                Thread.detachNewThread {
                    while true {
                        let chunk = handle.availableData
                        if chunk.isEmpty { break }
                        collector.append(chunk)
                    }
                    readers.leave()
                }
            }
            let once = ResumeOnce()
            let finish: @Sendable (Int32, String?) -> Void = { status, failure in
                once.run {
                    processes?.remove(process)
                    continuation.resume(
                        returning: Output(status: status, stdout: out.text, stderr: failure ?? err.text))
                }
            }
            process.terminationHandler = { finished in
                let status = finished.terminationStatus
                // Give the readers a moment to drain what ssh wrote, but no longer.
                DispatchQueue.global().async {
                    _ = readers.wait(timeout: .now() + 1)
                    finish(status, nil)
                }
            }
            do {
                try process.run()
            } catch {
                try? stdout.fileHandleForWriting.close()
                try? stderr.fileHandleForWriting.close()
                process.terminationHandler = nil
                finish(-1, error.localizedDescription)
                return
            }
            processes?.insert(process)
            DispatchQueue.global().asyncAfter(deadline: .now() + timeout) {
                guard process.isRunning else { return }
                process.terminate()
                DispatchQueue.global().asyncAfter(deadline: .now() + 2) {
                    if process.isRunning { kill(process.processIdentifier, SIGKILL) }
                    finish(-1, "ssh timed out after \(Int(timeout)) s")
                }
            }
        }
    }

    /// The remote user's home, resolved once per connection.
    static func resolveHome(sshHost: String, processes: OneShotProcesses? = nil) async throws -> String {
        let output = await run(resolveHomeArguments(sshHost: sshHost), timeout: 20, processes: processes)
        let home = output.stdout.trimmingCharacters(in: .whitespacesAndNewlines)
        guard output.status == 0, home.hasPrefix("/") else {
            throw MachineConnectionError.ssh(sshErrorMessage(output))
        }
        return home
    }

    static func sshErrorMessage(_ output: Output) -> String {
        let message = output.stderr.trimmingCharacters(in: .whitespacesAndNewlines)
        if !message.isEmpty { return message.components(separatedBy: .newlines).last ?? message }
        return output.status == 0 ? "ssh returned no home directory" : "ssh exited with status \(output.status)"
    }
}

nonisolated enum MachineConnectionError: LocalizedError {
    case ssh(String)

    var errorDescription: String? {
        switch self {
        case .ssh(let message): message
        }
    }
}

/// Bounded, thread-safe capture of a long-running process's diagnostics.
nonisolated final class OutputCollector: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()

    func append(_ chunk: Data) {
        lock.lock()
        data.append(chunk)
        if data.count > 16_384 { data = data.suffix(16_384) }
        lock.unlock()
    }

    var text: String {
        lock.lock()
        defer { lock.unlock() }
        return String(decoding: data, as: UTF8.self)
    }
}

/// Resumes a continuation exactly once, whichever of exit, deadline or launch failure is first.
nonisolated final class ResumeOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false

    func run(_ body: () -> Void) {
        lock.lock()
        let first = !done
        done = true
        lock.unlock()
        if first { body() }
    }
}

/// The one-shot ssh calls (home lookup, Start daemon) a machine has in flight, so a
/// disconnect or quit ends them instead of leaving them running.
nonisolated final class OneShotProcesses: @unchecked Sendable {
    private let lock = NSLock()
    private var running: [ObjectIdentifier: Process] = [:]

    func insert(_ process: Process) {
        lock.lock()
        running[ObjectIdentifier(process)] = process
        lock.unlock()
    }

    func remove(_ process: Process) {
        lock.lock()
        running[ObjectIdentifier(process)] = nil
        lock.unlock()
    }

    func terminateAll() {
        lock.lock()
        let processes = Array(running.values)
        running.removeAll()
        lock.unlock()
        for process in processes where process.isRunning { process.terminate() }
    }
}

/// `~/.pu/machines/<id>/forward.lock`, held (flock) by the one app instance that owns the
/// machine's forward for as long as it does. Another instance follows that forward instead of
/// launching its own, and never ends it.
nonisolated final class ForwardLock: @unchecked Sendable {
    private let lock = NSLock()
    private var descriptor: Int32 = -1
    private let path: String

    init(machineId: UUID) {
        path = MachinePaths.directory(machineId: machineId) + "/forward.lock"
    }

    var isHeld: Bool {
        lock.lock()
        defer { lock.unlock() }
        return descriptor >= 0
    }

    /// Take the lock without waiting; true if this instance holds it (already or now).
    func tryAcquire() -> Bool {
        lock.lock()
        defer { lock.unlock() }
        if descriptor >= 0 { return true }
        let fd = open(path, O_CREAT | O_RDWR | O_CLOEXEC, 0o600)
        guard fd >= 0 else { return false }
        guard flock(fd, LOCK_EX | LOCK_NB) == 0 else {
            close(fd)
            return false
        }
        descriptor = fd
        return true
    }

    func release() {
        lock.lock()
        defer { lock.unlock() }
        guard descriptor >= 0 else { return }
        flock(descriptor, LOCK_UN)
        close(descriptor)
        descriptor = -1
    }
}

/// `~/.pu/machines/<id>/forward.pid` records the app's live forward so one left behind by a
/// crash or force-quit is ended on the next launch instead of leaking.
nonisolated enum ForwardPidFile {
    static func path(machineId: UUID) -> String {
        MachinePaths.directory(machineId: machineId) + "/forward.pid"
    }

    static func record(_ pid: pid_t, machineId: UUID) {
        _ = FileManager.default.createFile(
            atPath: path(machineId: machineId), contents: Data("\(pid)\n".utf8), attributes: [.posixPermissions: 0o600])
    }

    /// Remove the record if it still names `pid` (a newer forward's record is left alone).
    static func clear(_ pid: pid_t, machineId: UUID) {
        guard recordedPid(machineId: machineId) == pid else { return }
        try? FileManager.default.removeItem(atPath: path(machineId: machineId))
    }

    static func recordedPid(machineId: UUID) -> pid_t? {
        guard let data = FileManager.default.contents(atPath: path(machineId: machineId)),
            let pid = pid_t(String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)),
            pid > 0
        else { return nil }
        return pid
    }

    /// End the recorded forward if that pid is still our ssh forward for this machine. A pid
    /// the system has since reused for anything else — including the user's own ssh — is
    /// left running.
    static func killStale(machineId: UUID, localSocket: String) {
        guard let pid = recordedPid(machineId: machineId) else { return }
        if isForward(executablePath: executablePath(of: pid), arguments: arguments(of: pid), localSocket: localSocket) {
            kill(pid, SIGTERM)
        }
        try? FileManager.default.removeItem(atPath: path(machineId: machineId))
    }

    /// An ssh process forwarding exactly this machine's local socket.
    static func isForward(executablePath: String?, arguments: [String]?, localSocket: String) -> Bool {
        guard let executablePath, (executablePath as NSString).lastPathComponent == "ssh",
            let arguments, let flag = arguments.firstIndex(of: "-L"), flag + 1 < arguments.count
        else { return false }
        return arguments[flag + 1].hasPrefix(localSocket + ":")
    }

    static func executablePath(of pid: pid_t) -> String? {
        var buffer = [CChar](repeating: 0, count: Int(MAXPATHLEN) * 4)
        let length = proc_pidpath(pid, &buffer, UInt32(buffer.count))
        guard length > 0 else { return nil }
        return String(decoding: buffer.prefix(Int(length)).map { UInt8(bitPattern: $0) }, as: UTF8.self)
    }

    /// argv of a process owned by this user (`KERN_PROCARGS2`: argc, exec path, argv).
    static func arguments(of pid: pid_t) -> [String]? {
        var mib: [Int32] = [CTL_KERN, KERN_PROCARGS2, pid]
        var size = 0
        guard sysctl(&mib, 3, nil, &size, nil, 0) == 0, size > MemoryLayout<Int32>.size else { return nil }
        var buffer = [UInt8](repeating: 0, count: size)
        guard sysctl(&mib, 3, &buffer, &size, nil, 0) == 0, size > MemoryLayout<Int32>.size else { return nil }
        let argc = buffer.withUnsafeBytes { $0.loadUnaligned(as: Int32.self) }
        var index = MemoryLayout<Int32>.size
        while index < size, buffer[index] != 0 { index += 1 }
        while index < size, buffer[index] == 0 { index += 1 }
        var arguments: [String] = []
        while arguments.count < Int(argc), index < size {
            let start = index
            while index < size, buffer[index] != 0 { index += 1 }
            arguments.append(String(decoding: buffer[start..<index], as: UTF8.self))
            index += 1
        }
        return arguments
    }
}

/// The process id of the running forward, readable synchronously so quitting the app can
/// end it without awaiting the actor.
nonisolated final class ForwardProcessHandle: @unchecked Sendable {
    private let lock = NSLock()
    private var pid: pid_t = 0
    private let machineId: UUID

    init(machineId: UUID) {
        self.machineId = machineId
    }

    func set(_ value: pid_t) {
        lock.lock()
        pid = value
        lock.unlock()
        if value > 0 { ForwardPidFile.record(value, machineId: machineId) }
    }

    func terminate() {
        lock.lock()
        let current = pid
        pid = 0
        lock.unlock()
        guard current > 0 else { return }
        kill(current, SIGTERM)
        ForwardPidFile.clear(current, machineId: machineId)
    }
}

/// One machine's SSH forward: `~/.pu/machines/<id>/daemon.sock` → remote `~/.pu/daemon.sock`.
///
/// Keeps the forward up with backoff while connected, probes the daemon with `health` (and
/// never starts it — MACH-003), and closes the forward on disconnect, removal or quit. The
/// remote daemon and its agents keep running regardless (MACH-004).
actor MachineConnection {
    let machine: Machine
    let endpoint: DaemonEndpoint
    nonisolated let forward: ForwardProcessHandle
    nonisolated let oneShots = OneShotProcesses()
    nonisolated let forwardLock: ForwardLock
    private let onState: @Sendable (MachineConnectionState) -> Void
    private var supervisor: Task<Void, Never>?
    private var process: Process?
    private var lastReported: MachineConnectionState = .disconnected
    private var generation = 0
    private var tracker = HealthProbeTracker()

    init(machine: Machine, onState: @escaping @Sendable (MachineConnectionState) -> Void) {
        self.machine = machine
        self.endpoint = .machine(machine.id)
        self.forward = ForwardProcessHandle(machineId: machine.id)
        self.forwardLock = ForwardLock(machineId: machine.id)
        self.onState = onState
    }

    func connect() {
        guard supervisor == nil else { return }
        generation += 1
        let current = generation
        supervisor = Task { await self.supervise(generation: current) }
    }

    /// Stop probing and reporting but leave the forward up, so writes already queued can
    /// still go through it before `disconnect`.
    func stopSupervising() {
        generation += 1
        supervisor?.cancel()
        supervisor = nil
    }

    func disconnect() {
        generation += 1
        supervisor?.cancel()
        supervisor = nil
        oneShots.terminateAll()
        stopForward()
        forwardLock.release()
        report(.disconnected)
    }

    /// Quit: end the forward and any one-shot ssh calls synchronously, without the actor.
    nonisolated func terminateForQuit() {
        oneShots.terminateAll()
        forward.terminate()
        forwardLock.release()
    }

    /// Probe now rather than at the next interval — after Start daemon, for instance.
    func refreshNow() async {
        guard supervisor != nil, FileManager.default.fileExists(atPath: endpoint.socketPath) else { return }
        if let availability = tracker.record(await probe()) { report(.connected(availability)) }
    }

    /// Start the remote daemon over ssh (an explicit user action).
    func startDaemon() async -> String? {
        let output = await MachineSSH.run(
            MachineSSH.startDaemonArguments(sshHost: machine.sshHost), timeout: 60, processes: oneShots)
        await refreshNow()
        return output.status == 0 ? nil : MachineSSH.sshErrorMessage(output)
    }

    // MARK: - Private

    /// A supervisor stays in charge only while it is the latest one and not cancelled; an
    /// older one finishing after a quick disconnect/connect must not touch the newer forward.
    private func isCurrent(_ generation: Int) -> Bool {
        !Task.isCancelled && generation == self.generation
    }

    private func supervise(generation: Int) async {
        var backoff: UInt64 = 2
        while isCurrent(generation) {
            _ = try? MachinePaths.ensureDirectory(machineId: machine.id)
            if !forwardLock.tryAcquire() {
                // Another PurePoint instance owns this machine's forward: use it, never end it.
                await followSharedForward(generation: generation)
                continue
            }
            report(.connecting)
            do {
                let home = try await MachineSSH.resolveHome(sshHost: machine.sshHost, processes: oneShots)
                guard isCurrent(generation) else { return }
                let stderr = try launchForward(remoteHome: home)
                guard await waitForSocket() else {
                    guard isCurrent(generation) else { return }
                    stopForward()
                    try? await Task.sleep(for: .milliseconds(200))
                    let message = stderr.text.trimmingCharacters(in: .whitespacesAndNewlines)
                    throw MachineConnectionError.ssh(
                        message.isEmpty ? "SSH forward did not start" : message.components(separatedBy: .newlines).last ?? message)
                }
                backoff = 2
                tracker = HealthProbeTracker()
                while isCurrent(generation), process?.isRunning == true {
                    let availability = tracker.record(await probe())
                    guard isCurrent(generation) else { return }
                    if let availability { report(.connected(availability)) }
                    // An inconclusive probe (BUSY, or a failure short of the limit) retries soon.
                    let seconds = availability == nil ? 2 : 10
                    for _ in 0..<seconds where isCurrent(generation) && process?.isRunning == true {
                        try? await Task.sleep(for: .seconds(1))
                    }
                }
                guard isCurrent(generation) else { return }
                stopForward()
                report(.offline("SSH connection closed"))
            } catch {
                guard isCurrent(generation) else { return }
                stopForward()
                report(.offline(error.localizedDescription))
            }
            guard isCurrent(generation) else { return }
            try? await Task.sleep(for: .seconds(Double(backoff)))
            backoff = min(backoff * 2, 60)
        }
    }

    /// Probe the forward another instance owns until this one can take the lock (that
    /// instance quit or disconnected) or the user disconnects.
    private func followSharedForward(generation: Int) async {
        tracker = HealthProbeTracker()
        while isCurrent(generation) {
            if forwardLock.tryAcquire() { return }
            if FileManager.default.fileExists(atPath: endpoint.socketPath) {
                let availability = tracker.record(await probe())
                guard isCurrent(generation) else { return }
                if let availability { report(.connected(availability)) }
            } else {
                report(.connecting)
            }
            for _ in 0..<5 where isCurrent(generation) {
                try? await Task.sleep(for: .seconds(1))
            }
        }
    }

    private func launchForward(remoteHome: String) throws -> OutputCollector {
        stopForward()
        // A forward left by a crashed or force-quit run would hold the socket.
        ForwardPidFile.killStale(machineId: machine.id, localSocket: endpoint.socketPath)
        // A socket left by an earlier run would look ready before ssh rebinds it.
        try? FileManager.default.removeItem(atPath: endpoint.socketPath)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: MachineSSH.sshPath)
        process.arguments = MachineSSH.forwardArguments(
            localSocket: endpoint.socketPath, remoteHome: remoteHome, sshHost: machine.sshHost)
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = FileHandle.nullDevice
        let pipe = Pipe()
        process.standardError = pipe
        try process.run()
        self.process = process
        forward.set(process.processIdentifier)
        // Collected on its own thread: ssh may never close stderr while it stays up.
        let collector = OutputCollector()
        let handle = pipe.fileHandleForReading
        Thread.detachNewThread {
            while true {
                let chunk = handle.availableData
                if chunk.isEmpty { break }
                collector.append(chunk)
            }
        }
        return collector
    }

    /// The forward is usable once its local socket exists; give ssh up to ~15 s.
    private func waitForSocket() async -> Bool {
        for _ in 0..<60 {
            guard process?.isRunning == true, !Task.isCancelled else { return false }
            if FileManager.default.fileExists(atPath: endpoint.socketPath) { return true }
            try? await Task.sleep(for: .milliseconds(250))
        }
        return false
    }

    private func stopForward() {
        forward.terminate()
        if let process, process.isRunning { process.terminate() }
        process = nil
    }

    /// `health` over the forward — never `DaemonLifecycle.ensureDaemon`, which would replace
    /// the forward with a local daemon.
    private func probe() async -> HealthProbe {
        let client = endpoint.makeClient()
        return await withTaskGroup(of: HealthProbe?.self) { group in
            group.addTask {
                Self.classify(try? await client.send(.health))
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(8))
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first ?? .failed
        }
    }

    nonisolated static func classify(_ response: DaemonResponse?) -> HealthProbe {
        switch response {
        case .healthReport(_, _, let version, _)?:
            version == DaemonProtocol.version ? .running : .incompatible(version)
        case .error(let code, _)? where code == "BUSY":
            .busy
        default:
            .failed
        }
    }

    private func report(_ state: MachineConnectionState) {
        guard state != lastReported else { return }
        lastReported = state
        onState(state)
    }
}
