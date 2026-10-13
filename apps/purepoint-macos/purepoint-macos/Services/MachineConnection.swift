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

    /// `ssh -o BatchMode=yes <ssh_host> 'printf %s "$HOME"'`
    static func resolveHomeArguments(sshHost: String) -> [String] {
        ["-o", "BatchMode=yes", sshHost, "printf %s \"$HOME\""]
    }

    /// `ssh -N -T -o BatchMode=yes -o ExitOnForwardFailure=yes -o ServerAliveInterval=15
    ///  -o ServerAliveCountMax=3 -o StreamLocalBindUnlink=yes -L <local>:<home>/.pu/daemon.sock <ssh_host>`
    static func forwardArguments(localSocket: String, remoteHome: String, sshHost: String) -> [String] {
        [
            "-N", "-T",
            "-o", "BatchMode=yes",
            "-o", "ExitOnForwardFailure=yes",
            "-o", "ServerAliveInterval=15",
            "-o", "ServerAliveCountMax=3",
            "-o", "StreamLocalBindUnlink=yes",
            "-L", localSocket + ":" + remoteHome + remoteSocket,
            sshHost,
        ]
    }

    /// Non-interactive SSH has a minimal PATH, so try the installed locations first.
    static let startDaemonCommand = "\"$HOME/.pu/bin/pu\" health || \"$HOME/.cargo/bin/pu\" health || pu health"

    static func startDaemonArguments(sshHost: String) -> [String] {
        [sshHost, startDaemonCommand]
    }

    struct Output: Sendable {
        let status: Int32
        let stdout: String
        let stderr: String
    }

    /// Run ssh to completion with a deadline. stdin is closed so ssh never waits on a prompt.
    static func run(_ arguments: [String], timeout: TimeInterval) async -> Output {
        await withCheckedContinuation { (continuation: CheckedContinuation<Output, Never>) in
            let process = Process()
            process.executableURL = URL(fileURLWithPath: sshPath)
            process.arguments = arguments
            process.standardInput = FileHandle.nullDevice
            let stdout = Pipe()
            let stderr = Pipe()
            process.standardOutput = stdout
            process.standardError = stderr
            nonisolated(unsafe) var outData = Data()
            nonisolated(unsafe) var errData = Data()
            let group = DispatchGroup()
            group.enter()
            Thread.detachNewThread {
                outData = stdout.fileHandleForReading.readDataToEndOfFile()
                group.leave()
            }
            group.enter()
            Thread.detachNewThread {
                errData = stderr.fileHandleForReading.readDataToEndOfFile()
                group.leave()
            }
            process.terminationHandler = { finished in
                group.notify(queue: .global()) {
                    continuation.resume(
                        returning: Output(
                            status: finished.terminationStatus,
                            stdout: String(data: outData, encoding: .utf8) ?? "",
                            stderr: String(data: errData, encoding: .utf8) ?? ""))
                }
            }
            do {
                try process.run()
            } catch {
                try? stdout.fileHandleForWriting.close()
                try? stderr.fileHandleForWriting.close()
                process.terminationHandler = nil
                group.notify(queue: .global()) {
                    continuation.resume(returning: Output(status: -1, stdout: "", stderr: error.localizedDescription))
                }
                return
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + timeout) {
                if process.isRunning { process.terminate() }
            }
        }
    }

    /// The remote user's home, resolved once per connection.
    static func resolveHome(sshHost: String) async throws -> String {
        let output = await run(resolveHomeArguments(sshHost: sshHost), timeout: 20)
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

/// The process id of the running forward, readable synchronously so quitting the app can
/// end it without awaiting the actor.
nonisolated final class ForwardProcessHandle: @unchecked Sendable {
    private let lock = NSLock()
    private var pid: pid_t = 0

    func set(_ value: pid_t) {
        lock.lock()
        pid = value
        lock.unlock()
    }

    func terminate() {
        lock.lock()
        let current = pid
        pid = 0
        lock.unlock()
        if current > 0 { kill(current, SIGTERM) }
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
    nonisolated let forward = ForwardProcessHandle()
    private let onState: @Sendable (MachineConnectionState) -> Void
    private var supervisor: Task<Void, Never>?
    private var process: Process?
    private var lastReported: MachineConnectionState = .disconnected
    private var generation = 0

    init(machine: Machine, onState: @escaping @Sendable (MachineConnectionState) -> Void) {
        self.machine = machine
        self.endpoint = .machine(machine.id)
        self.onState = onState
    }

    func connect() {
        guard supervisor == nil else { return }
        generation += 1
        let current = generation
        supervisor = Task { await self.supervise(generation: current) }
    }

    func disconnect() {
        generation += 1
        supervisor?.cancel()
        supervisor = nil
        stopForward()
        report(.disconnected)
    }

    /// Probe now rather than at the next interval — after Start daemon, for instance.
    func refreshNow() async {
        guard process?.isRunning == true else { return }
        report(.connected(await probe()))
    }

    /// Start the remote daemon over ssh (an explicit user action).
    func startDaemon() async -> String? {
        let output = await MachineSSH.run(MachineSSH.startDaemonArguments(sshHost: machine.sshHost), timeout: 60)
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
            report(.connecting)
            do {
                let home = try await MachineSSH.resolveHome(sshHost: machine.sshHost)
                guard isCurrent(generation) else { return }
                _ = try MachinePaths.ensureDirectory(machineId: machine.id)
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
                while isCurrent(generation), process?.isRunning == true {
                    let availability = await probe()
                    guard isCurrent(generation) else { return }
                    report(.connected(availability))
                    for _ in 0..<10 where isCurrent(generation) && process?.isRunning == true {
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

    private func launchForward(remoteHome: String) throws -> OutputCollector {
        stopForward()
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
    private func probe() async -> DaemonAvailability {
        let client = endpoint.makeClient()
        return await withTaskGroup(of: DaemonAvailability?.self) { group in
            group.addTask {
                switch try? await client.send(.health) {
                case .healthReport(_, _, let version, _)?:
                    return version == DaemonProtocol.version ? .running : .incompatible(version)
                case .error(let code, _)? where code == "BUSY":
                    return .running
                default:
                    return .stopped
                }
            }
            group.addTask {
                try? await Task.sleep(for: .seconds(8))
                return nil
            }
            let first = await group.next() ?? nil
            group.cancelAll()
            return first ?? .stopped
        }
    }

    private func report(_ state: MachineConnectionState) {
        guard state != lastReported else { return }
        lastReported = state
        onState(state)
    }
}
