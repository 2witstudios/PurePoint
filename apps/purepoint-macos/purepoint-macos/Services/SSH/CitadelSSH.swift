#if os(iOS)
@preconcurrency import Citadel
import Foundation
import NIOCore
import NIOSSH

/// Connects to a machine with Citadel: the device identity, the machine's pinned host key
/// checked before authentication, and no automatic reconnect (the session paces reconnects).
nonisolated struct CitadelSSHConnector: SSHConnector {
    let machineId: UUID
    let machineName: String
    let destination: SSHDestination
    let identities: SSHIdentityStore
    let pins: HostKeyPinStore

    /// TCP connect and SSH login together.
    static let connectTimeout: Int64 = 15

    func connect(onDisconnect: @escaping @Sendable () -> Void) async throws -> any SSHExecConnection {
        let identity = try identities.identity()
        let validator = PinningHostKeyValidator(machineId: machineId, pins: pins)
        let client: SSHClient
        do {
            client = try await SSHClient.connect(
                host: destination.host,
                port: destination.port,
                authenticationMethod: .ed25519(username: destination.user, privateKey: identity.privateKey),
                hostKeyValidator: .custom(validator),
                reconnect: .never,
                connectTimeout: .seconds(Self.connectTimeout))
        } catch {
            throw describe(error, verdict: validator.verdict)
        }
        let connection = CitadelExecConnection(client: client)
        client.onDisconnect(perform: onDisconnect)
        if !client.isConnected { onDisconnect() }
        return connection
    }

    /// The refusal the user can act on: a host key to confirm or that changed, a key the
    /// machine does not accept, or the network error.
    private func describe(_ error: Error, verdict: HostKeyVerdict?) -> Error {
        if let verdict, let refusal = SSHTrustError(verdict) { return refusal }
        if let error = error as? SSHClientError, error == .allAuthenticationOptionsFailed {
            return SSHTransportError.authenticationFailed(machine: machineName, user: destination.user)
        }
        if error is AuthenticationFailed {
            return SSHTransportError.authenticationFailed(machine: machineName, user: destination.user)
        }
        return SSHTransportError.connectFailed(machine: machineName, detail: Self.detail(error))
    }

    private static func detail(_ error: Error) -> String {
        if let error = error as? ChannelError {
            switch error {
            case .connectTimeout: return "timed out"
            default: break
            }
        }
        let text = String(describing: error)
        return text.count > 200 ? String(text.prefix(200)) + "…" : text
    }
}

/// One Citadel `SSHClient`. `SSHClient` is not `Sendable`; it is only used through Citadel's
/// own thread-safe channel operations here.
nonisolated final class CitadelExecConnection: SSHExecConnection, @unchecked Sendable {
    private let client: SSHClient

    init(client: SSHClient) {
        self.client = client
    }

    func openExec(_ command: String) async throws -> any SSHExecChannel {
        let channel = CitadelExecChannel()
        let client = self.client
        try await withCheckedThrowingContinuation { (opened: CheckedContinuation<Void, Error>) in
            let once = ResumeOnce()
            let pump = Task {
                do {
                    try await client.withExec(command) { inbound, outbound in
                        channel.attach(writer: outbound)
                        once.run { opened.resume() }
                        await channel.pump(inbound)
                    }
                } catch {
                    // Before the channel opened, this is the refusal (`channelSetupRejected`
                    // for `MaxSessions`). After, `withExec`'s own close throws
                    // `alreadyClosed` once the remote command exited: the status was already
                    // read inside `pump`, so this only ends a channel that has not ended.
                    once.run { opened.resume(throwing: error) }
                    channel.finish(.disconnected)
                }
            }
            channel.setPump(pump)
        }
        return channel
    }

    func close() {
        let client = self.client
        Task { try? await client.close() }
    }
}

/// One exec channel's stdout queue and stdin writer.
nonisolated final class CitadelExecChannel: SSHExecChannel, @unchecked Sendable {
    /// Diagnostics kept for the error message, never daemon traffic.
    private static let stderrLimit = 4096

    private let lock = NSLock()
    private var events: [SSHExecEvent] = []
    private var ended: SSHExecEnd?
    private var reader: CheckedContinuation<SSHExecEvent, Never>?
    private var writer: TTYStdinWriter?
    private var pumpTask: Task<Void, Never>?
    private var stderr = Data()

    fileprivate func attach(writer: TTYStdinWriter) {
        lock.withLock { self.writer = writer }
    }

    fileprivate func setPump(_ task: Task<Void, Never>) {
        let closed = lock.withLock {
            pumpTask = task
            return ended == .closedLocally
        }
        if closed { task.cancel() }
    }

    /// Read the channel until it ends. The exit status is only visible here: `withExec`
    /// replaces it with `alreadyClosed` from its trailing close (research note, caveat 2).
    fileprivate func pump(_ inbound: TTYOutput) async {
        do {
            for try await output in inbound {
                switch output {
                case .stdout(let buffer):
                    deliver(.stdout(Data(buffer.readableBytesView)))
                case .stderr(let buffer):
                    lock.withLock {
                        stderr.append(contentsOf: buffer.readableBytesView)
                        if stderr.count > Self.stderrLimit { stderr = stderr.suffix(Self.stderrLimit) }
                    }
                }
            }
            finish(.exited(0, stderr: stderrText))
        } catch let failed as SSHClient.CommandFailed {
            finish(.exited(failed.exitCode, stderr: stderrText))
        } catch {
            finish(.disconnected)
        }
    }

    private var stderrText: String { lock.withLock { String(decoding: stderr, as: UTF8.self) } }

    func write(_ data: Data) async throws {
        guard let writer = lock.withLock({ ended == nil ? writer : nil }) else { throw DaemonClientError.eof }
        try await writer.write(ByteBuffer(bytes: data))
    }

    func read() async -> SSHExecEvent {
        await withCheckedContinuation { continuation in
            let ready: SSHExecEvent? = lock.withLock {
                if !events.isEmpty { return events.removeFirst() }
                if let ended { return .end(ended) }
                reader = continuation
                return nil
            }
            if let ready { continuation.resume(returning: ready) }
        }
    }

    /// Closing ends the exec: the pump stops, `withExec` closes the channel, and the remote
    /// relay sees end of input.
    func close() {
        finish(.closedLocally)
        lock.withLock { pumpTask }?.cancel()
    }

    fileprivate func finish(_ end: SSHExecEnd) {
        let waiting: CheckedContinuation<SSHExecEvent, Never>? = lock.withLock {
            guard ended == nil else { return nil }
            ended = end
            if end == .closedLocally { events.removeAll() }
            guard events.isEmpty, let reader else { return nil }
            self.reader = nil
            return reader
        }
        waiting?.resume(returning: .end(end))
    }

    private func deliver(_ event: SSHExecEvent) {
        let waiting: CheckedContinuation<SSHExecEvent, Never>? = lock.withLock {
            guard ended == nil else { return nil }
            if let reader {
                self.reader = nil
                return reader
            }
            events.append(event)
            return nil
        }
        waiting?.resume(returning: event)
    }
}
#endif
