#if os(iOS)
import Foundation
import NIOSSH

// MARK: - Connection seam

/// How an exec channel ended.
nonisolated enum SSHExecEnd: Equatable, Sendable {
    /// The remote command exited (0 also covers an end with no status). `stderr` is its last
    /// few kilobytes of diagnostics.
    case exited(Int, stderr: String)
    /// The channel or the SSH connection under it failed.
    case disconnected
    /// This side closed the channel.
    case closedLocally
}

nonisolated enum SSHExecEvent: Equatable, Sendable {
    case stdout(Data)
    case end(SSHExecEnd)
}

/// One exec channel: stdin in, stdout out, the exit status at the end.
nonisolated protocol SSHExecChannel: AnyObject, Sendable {
    /// Write to the command's stdin.
    func write(_ data: Data) async throws
    /// The next stdout chunk, or how the channel ended. Once ended, every call returns `.end`.
    func read() async -> SSHExecEvent
    /// Close the channel, which is end-of-input for the remote command. Idempotent.
    func close()
}

/// One authenticated SSH connection that opens exec channels.
nonisolated protocol SSHExecConnection: AnyObject, Sendable {
    /// Open a session channel running `command`. Throws the server's refusal (see
    /// `SSHMachineSession.isSessionLimit`) when it will not open another channel.
    func openExec(_ command: String) async throws -> any SSHExecChannel
    func close()
}

/// Makes SSH connections to one machine, with its identity and pinned host key.
nonisolated protocol SSHConnector: Sendable {
    /// `onDisconnect` runs once when the connection drops or is closed.
    func connect(onDisconnect: @escaping @Sendable () -> Void) async throws -> any SSHExecConnection
}

// MARK: - Backoff

/// Reconnect pacing after failed connects: 1, 2, 4, 8, 16, then every 30 s. A success resets it.
nonisolated struct SSHReconnectBackoff: Sendable {
    static let schedule: [TimeInterval] = [1, 2, 4, 8, 16, 30]

    private(set) var failures = 0
    private(set) var notBefore: Date?

    static func delay(afterFailures failures: Int) -> TimeInterval {
        guard failures > 0 else { return 0 }
        return schedule[min(failures, schedule.count) - 1]
    }

    mutating func recordFailure(at now: Date) {
        failures += 1
        notBefore = now.addingTimeInterval(Self.delay(afterFailures: failures))
    }

    mutating func recordSuccess() {
        failures = 0
        notBefore = nil
    }

    /// Seconds until the next connect may start.
    func remainingDelay(at now: Date) -> TimeInterval {
        guard let notBefore else { return 0 }
        return max(0, notBefore.timeIntervalSince(now))
    }
}

// MARK: - Session

/// A channel opened through a machine session, with the hook that hands its slot back.
nonisolated struct SSHOpenedChannel: Sendable {
    let channel: any SSHExecChannel
    /// Call once when the channel ended or was closed; an idle overflow connection closes.
    let release: @Sendable () -> Void
    /// Whether it rides the primary connection (false: an overflow connection).
    let onPrimary: Bool
}

/// One machine's SSH: a single primary connection that carries every daemon channel, plus
/// overflow connections opened only when the server refuses a channel (`MaxSessions`), each
/// closed again once it carries none (IPAD-005). Connect failures back off; while backing off,
/// opening a channel fails at once with the last error, as a socket with no listener would.
actor SSHMachineSession {
    private struct Slot {
        let connection: any SSHExecConnection
        var channels = 0
    }

    private let connector: any SSHConnector
    private let now: @Sendable () -> Date
    private var primaryId: Int?
    private var slots: [Int: Slot] = [:]
    /// Overflow connection ids in the order they were opened.
    private var overflowOrder: [Int] = []
    private var nextId = 0
    private var pendingPrimary: Task<Int, Error>?
    private var backoff = SSHReconnectBackoff()
    private var lastError: Error?
    private var onPrimaryDropped: (@Sendable () -> Void)?

    init(connector: any SSHConnector, now: @escaping @Sendable () -> Date = { Date() }) {
        self.connector = connector
        self.now = now
    }

    /// Runs whenever the primary connection drops (not when `disconnect` closes it).
    func setPrimaryDroppedHandler(_ handler: (@Sendable () -> Void)?) {
        onPrimaryDropped = handler
    }

    var isConnected: Bool { primaryId != nil }
    var overflowCount: Int { overflowOrder.count }
    var backoffState: SSHReconnectBackoff { backoff }

    /// The channels each connection carries, primary first (for tests and diagnostics).
    var channelCounts: [Int] {
        ([primaryId].compactMap { $0 } + overflowOrder).compactMap { slots[$0]?.channels }
    }

    /// Bring the primary connection up, first waiting out any backoff. Throws the connect
    /// error (a host-key refusal is `SSHTrustError`).
    func connect() async throws {
        let wait = backoff.remainingDelay(at: now())
        if primaryId == nil, wait > 0 {
            try await Task.sleep(for: .seconds(wait))
        }
        _ = try await primary(failFast: false)
    }

    /// Close every connection. The machine's channels end as disconnected.
    func disconnect() {
        pendingPrimary?.cancel()
        pendingPrimary = nil
        let connections = slots.values.map(\.connection)
        slots.removeAll()
        overflowOrder.removeAll()
        primaryId = nil
        for connection in connections { connection.close() }
    }

    /// Forget past failures so the next connect starts at once (a user's Connect, a key
    /// import, a trusted host key).
    func resetBackoff() {
        backoff.recordSuccess()
        lastError = nil
    }

    /// Open an exec channel on the primary connection, or on an overflow connection when the
    /// primary refuses it.
    func openChannel(_ command: String) async throws -> SSHOpenedChannel {
        let primary = try await primary(failFast: true)
        do {
            return try await open(command, on: primary)
        } catch where Self.isSessionLimit(error) {
            // Fall through to the overflow connections.
        }
        for id in overflowOrder {
            do {
                return try await open(command, on: id)
            } catch where Self.isSessionLimit(error) {
                continue
            }
        }
        let id = try await addConnection(primary: false)
        do {
            return try await open(command, on: id)
        } catch {
            releaseIfIdle(id)
            throw error
        }
    }

    /// The server refused another session channel on this connection: OpenSSH `MaxSessions`.
    nonisolated static func isSessionLimit(_ error: Error) -> Bool {
        if let error = error as? NIOSSHError, error.type == .channelSetupRejected { return true }
        return error is SSHSessionLimitReached
    }

    // MARK: - Private

    private func primary(failFast: Bool) async throws -> Int {
        if let primaryId { return primaryId }
        if let pendingPrimary { return try await pendingPrimary.value }
        if failFast, backoff.remainingDelay(at: now()) > 0, let lastError { throw lastError }
        let task = Task { try await self.addConnection(primary: true) }
        pendingPrimary = task
        defer { pendingPrimary = nil }
        do {
            let id = try await task.value
            backoff.recordSuccess()
            lastError = nil
            return id
        } catch {
            if !(error is CancellationError) {
                backoff.recordFailure(at: now())
                lastError = error
            }
            throw error
        }
    }

    private func addConnection(primary: Bool) async throws -> Int {
        nextId += 1
        let id = nextId
        let connection = try await connector.connect { [weak self] in
            Task { await self?.dropped(id) }
        }
        if Task.isCancelled {
            // `disconnect` ran while this connect was in flight.
            connection.close()
            throw CancellationError()
        }
        slots[id] = Slot(connection: connection)
        if primary { primaryId = id } else { overflowOrder.append(id) }
        return id
    }

    private func open(_ command: String, on id: Int) async throws -> SSHOpenedChannel {
        guard let connection = slots[id]?.connection else { throw DaemonClientError.eof }
        // Count the channel before the await so an overflow connection being opened is not
        // closed as idle meanwhile.
        slots[id]?.channels += 1
        let channel: any SSHExecChannel
        do {
            channel = try await connection.openExec(command)
        } catch {
            slots[id]?.channels -= 1
            releaseIfIdle(id)
            throw error
        }
        let released = ReleaseOnce()
        return SSHOpenedChannel(
            channel: channel,
            release: { [weak self] in
                guard released.claim() else { return }
                Task { await self?.release(id) }
            },
            onPrimary: id == primaryId)
    }

    private func release(_ id: Int) {
        guard slots[id] != nil else { return }
        slots[id]?.channels -= 1
        releaseIfIdle(id)
    }

    /// An overflow connection closes once it carries no channels; the primary stays.
    private func releaseIfIdle(_ id: Int) {
        guard id != primaryId, let slot = slots[id], slot.channels <= 0 else { return }
        slots[id] = nil
        overflowOrder.removeAll { $0 == id }
        slot.connection.close()
    }

    private func dropped(_ id: Int) {
        guard slots.removeValue(forKey: id) != nil else { return }
        overflowOrder.removeAll { $0 == id }
        if id == primaryId {
            primaryId = nil
            onPrimaryDropped?()
        }
    }
}

/// A fake server's channel refusal in tests; NIOSSH reports `channelSetupRejected`.
nonisolated struct SSHSessionLimitReached: Error {}

nonisolated private final class ReleaseOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var claimed = false

    func claim() -> Bool {
        lock.withLock {
            defer { claimed = true }
            return !claimed
        }
    }
}
#endif
