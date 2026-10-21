#if os(iOS)
import Foundation

// MARK: - Errors

/// Failures only the SSH transport has, each naming the machine so the message is actionable.
nonisolated enum SSHTransportError: LocalizedError, Equatable {
    /// The relay's shell could not find `pu` (exit 127).
    case puNotInstalled(machine: String)
    /// The relay exited with another status.
    case relayFailed(machine: String, status: Int, detail: String)
    /// The SSH connection could not be made.
    case connectFailed(machine: String, detail: String)
    /// The machine refused the iPad's key.
    case authenticationFailed(machine: String, user: String)
    /// The registry entry is an ssh config alias, which the iPad cannot resolve.
    case unsupportedDestination(machine: String)

    var errorDescription: String? {
        switch self {
        case .puNotInstalled(let machine):
            "PurePoint is not installed on \(machine): `pu` was not found in ~/.pu/bin, ~/.cargo/bin or the login PATH. Install PurePoint there, then connect again."
        case .relayFailed(let machine, let status, let detail):
            "The daemon relay on \(machine) exited with status \(status)" + (detail.isEmpty ? "." : ": \(detail)")
        case .connectFailed(let machine, let detail):
            "Could not connect to \(machine): \(detail)"
        case .authenticationFailed(let machine, let user):
            "\(machine) refused this iPad's SSH key for \(user). Add the iPad's public key to ~/.ssh/authorized_keys for \(user) on \(machine)."
        case .unsupportedDestination(let machine):
            "\(machine) is not a user@host[:port] destination, which the iPad needs (it has no ssh config aliases)."
        }
    }
}

/// How a relay channel's end reads to `DaemonClient`: exactly what a Unix socket reports where
/// one exists (REQ-IPAD-015), plus actionable errors for a missing or failing `pu`.
nonisolated enum SSHRelayErrors {
    /// `pu daemon proxy`'s status when no daemon answers (`EX_UNAVAILABLE`).
    static let noDaemonStatus = 69
    /// The shell's status when `pu` is not on the PATH.
    static let commandNotFoundStatus = 127

    static func error(for end: SSHExecEnd, machine: String) -> Error {
        switch end {
        case .exited(0, _), .disconnected:
            DaemonClientError.eof
        case .exited(noDaemonStatus, _):
            DaemonClientError.notRunning
        case .exited(commandNotFoundStatus, _):
            SSHTransportError.puNotInstalled(machine: machine)
        case .exited(let status, let stderr):
            SSHTransportError.relayFailed(machine: machine, status: status, detail: lastLine(stderr))
        case .closedLocally:
            DaemonClientError.cancelled
        }
    }

    static func lastLine(_ text: String) -> String {
        text.split(whereSeparator: \.isNewline).last.map { $0.trimmingCharacters(in: .whitespaces) } ?? ""
    }
}

// MARK: - Transport

/// Daemon connections to a machine over SSH (IPAD-004): each stream is one exec channel running
/// `pu daemon proxy` on the machine's session, carrying the daemon's newline JSON unchanged.
nonisolated struct SSHExecTransport: DaemonTransport {
    /// Run through `/bin/sh` like the Mac's start command, so it works whatever the login
    /// shell, with `pu` found in the install locations a non-interactive PATH lacks.
    static let relayCommand =
        "/bin/sh -c 'PATH=\"$HOME/.pu/bin:$HOME/.cargo/bin:$PATH\"; export PATH; exec pu daemon proxy'"
    /// The Mac's Start daemon command (`machines.md`), given 60 s.
    static let startDaemonCommand = MachineSSHCommands.startDaemonCommand

    /// Bringing up SSH (TCP, key exchange, authentication) takes longer than a socket connect.
    static let connectTimeout: TimeInterval = 20

    let machineId: UUID
    let sessions: SSHMachineSessions

    /// Resolved per stream, so a client made before the machine's session was (re)created
    /// still reaches the current one.
    func openStream(cancellable: Bool) async throws -> any DaemonByteStream {
        guard let (session, machineName) = sessions.session(for: machineId) else {
            throw DaemonClientError.notRunning
        }
        let opened = try await Deadline.run(
            seconds: Self.connectTimeout, cancellable: cancellable, onTimeout: DaemonClientError.connectTimeout
        ) {
            try await session.openChannel(Self.relayCommand)
        } discard: { late in
            late.channel.close()
            late.release()
        }
        return SSHRelayByteStream(opened: opened, machineName: machineName)
    }

    /// `sendBlocking`'s connect gets the SSH connect allowance, not the request's timeout.
    func openBlockingStream(timeout: TimeInterval) throws -> any DaemonBlockingByteStream {
        try bridgedBlockingStream(connectTimeout: Self.connectTimeout + 1, timeout: timeout)
    }
}

/// Command strings shared by the relay and Start daemon.
nonisolated enum MachineSSHCommands {
    static let startDaemonCommand =
        "/bin/sh -c 'PATH=\"$HOME/.pu/bin:$HOME/.cargo/bin:$PATH\"; export PATH; exec pu health'"
}

/// A daemon stream over one relay channel.
nonisolated final class SSHRelayByteStream: DaemonByteStream {
    private let opened: SSHOpenedChannel
    private let machineName: String

    init(opened: SSHOpenedChannel, machineName: String) {
        self.opened = opened
        self.machineName = machineName
    }

    /// A stream dropped without `close` still ends its channel and hands its slot back; both
    /// are idempotent, so a closed stream is unaffected.
    deinit {
        close()
    }

    /// How long a failed write waits for the relay's exit status before reporting a plain EOF.
    static let endGrace: Duration = .seconds(1)

    func send(_ data: Data) async throws {
        do {
            try await opened.channel.write(data)
        } catch {
            // The relay may have exited before reading (no daemon, no `pu`): its status says why.
            if case .exited(let status, let stderr) = await Self.end(of: opened.channel, within: Self.endGrace) {
                opened.release()
                throw SSHRelayErrors.error(for: .exited(status, stderr: stderr), machine: machineName)
            }
            // A write into a dropped session fails like one into a closed socket.
            throw DaemonClientError.eof
        }
    }

    /// How `channel` ended, waiting at most `limit`. A channel that has not ended by then is
    /// closed, which also ends the wait: a stream whose write failed carries nothing more.
    static func end(of channel: any SSHExecChannel, within limit: Duration) async -> SSHExecEnd? {
        await withTaskGroup(of: SSHExecEnd?.self) { group in
            group.addTask {
                while true {
                    if case .end(let end) = await channel.read() { return end }
                }
            }
            group.addTask {
                try? await Task.sleep(for: limit)
                return nil
            }
            let first = await group.next() ?? nil
            if first == nil { channel.close() }
            group.cancelAll()
            return first
        }
    }

    func receive() async throws -> Data {
        while true {
            switch await opened.channel.read() {
            case .stdout(let data):
                if !data.isEmpty { return data }
            case .end(let end):
                opened.release()
                throw SSHRelayErrors.error(for: end, machine: machineName)
            }
        }
    }

    func close() {
        opened.channel.close()
        opened.release()
    }
}

// MARK: - Deadlines

nonisolated enum Deadline {
    /// Run `operation` with a deadline. With `cancellable`, cancelling the caller throws
    /// `DaemonClientError.cancelled`; without it the operation runs to completion or the
    /// deadline. A value that arrives after the caller gave up is handed to `discard`.
    static func run<T: Sendable>(
        seconds: TimeInterval,
        cancellable: Bool,
        onTimeout: Error,
        operation: @escaping @Sendable () async throws -> T,
        discard: @escaping @Sendable (T) -> Void
    ) async throws -> T {
        let gate = Gate<T>(discard: discard)
        let work = Task {
            do { gate.finish(.success(try await operation())) } catch { gate.finish(.failure(error)) }
        }
        let timer = Task {
            try? await Task.sleep(for: .seconds(seconds))
            gate.finish(.failure(onTimeout))
        }
        defer { timer.cancel() }
        if cancellable {
            return try await withTaskCancellationHandler {
                try await withCheckedThrowingContinuation { gate.wait($0) }
            } onCancel: {
                gate.finish(.failure(DaemonClientError.cancelled))
                work.cancel()
            }
        }
        return try await withCheckedThrowingContinuation { gate.wait($0) }
    }

    /// Delivers the first result to the waiter; later successes are discarded.
    nonisolated private final class Gate<T: Sendable>: @unchecked Sendable {
        private let lock = NSLock()
        private let discard: @Sendable (T) -> Void
        private var result: Result<T, Error>?
        private var waiter: CheckedContinuation<T, Error>?

        init(discard: @escaping @Sendable (T) -> Void) {
            self.discard = discard
        }

        func wait(_ continuation: CheckedContinuation<T, Error>) {
            let ready: Result<T, Error>? = lock.withLock {
                if let result { return result }
                waiter = continuation
                return nil
            }
            if let ready { continuation.resume(with: ready) }
        }

        func finish(_ value: Result<T, Error>) {
            let (first, waiter): (Bool, CheckedContinuation<T, Error>?) = lock.withLock {
                guard result == nil else { return (false, nil) }
                result = value
                defer { self.waiter = nil }
                return (true, self.waiter)
            }
            guard first else {
                if case .success(let late) = value { discard(late) }
                return
            }
            waiter?.resume(with: value)
        }
    }
}

// MARK: - Machine sessions

/// The iPad's SSH session per machine, shared by everything that talks to that machine's
/// daemon: `DaemonEndpoint.machine(id)` resolves here. A machine row's connection registers
/// its session while the machine is in the registry.
nonisolated final class SSHMachineSessions: @unchecked Sendable {
    static let shared = SSHMachineSessions()

    private struct Entry {
        let session: SSHMachineSession
        let machineName: String
    }

    private let lock = NSLock()
    private var entries: [UUID: Entry] = [:]

    func register(_ session: SSHMachineSession, machineId: UUID, machineName: String) {
        lock.withLock { entries[machineId] = Entry(session: session, machineName: machineName) }
    }

    func unregister(machineId: UUID, session: SSHMachineSession) {
        lock.withLock {
            if entries[machineId]?.session === session { entries[machineId] = nil }
        }
    }

    func session(for machineId: UUID) -> (SSHMachineSession, String)? {
        lock.withLock { entries[machineId].map { ($0.session, $0.machineName) } }
    }

    /// Install the iOS transport provider: machine endpoints go over SSH, and the local
    /// endpoint, which iOS does not have, fails loudly. Called once at app start.
    static func installTransportProvider() {
        DaemonEndpoint.transportProvider = { endpoint in
            guard let machineId = endpoint.machineId else { return UnreachableLocalTransport() }
            return SSHExecTransport(machineId: machineId, sessions: shared)
        }
    }
}
#endif
