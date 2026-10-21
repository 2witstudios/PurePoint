#if os(iOS)
import Foundation
import Testing

@testable import PurePoint

// MARK: - Fakes

/// An exec channel whose remote side is a closure: `respond` sees each write and may answer
/// with stdout or end the channel.
final class FakeExecChannel: SSHExecChannel, @unchecked Sendable {
    private let lock = NSLock()
    private var events: [SSHExecEvent] = []
    private var ended: SSHExecEnd?
    private var reader: CheckedContinuation<SSHExecEvent, Never>?
    private let respond: @Sendable (FakeExecChannel, Data) -> Void
    let command: String

    init(command: String, respond: @escaping @Sendable (FakeExecChannel, Data) -> Void) {
        self.command = command
        self.respond = respond
    }

    func write(_ data: Data) async throws {
        guard lock.withLock({ ended == nil }) else { throw DaemonClientError.eof }
        respond(self, data)
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

    func close() { end(.closedLocally) }

    var isEnded: Bool { lock.withLock { ended != nil } }

    func emit(_ data: Data) {
        let waiting: CheckedContinuation<SSHExecEvent, Never>? = lock.withLock {
            guard ended == nil else { return nil }
            if let reader {
                self.reader = nil
                return reader
            }
            events.append(.stdout(data))
            return nil
        }
        waiting?.resume(returning: .stdout(data))
    }

    func end(_ end: SSHExecEnd) {
        let waiting: CheckedContinuation<SSHExecEvent, Never>? = lock.withLock {
            guard ended == nil else { return nil }
            ended = end
            guard events.isEmpty, let reader else { return nil }
            self.reader = nil
            return reader
        }
        waiting?.resume(returning: .end(end))
    }
}

/// A connection that accepts `maxSessions` concurrent channels, like OpenSSH's `MaxSessions`.
final class FakeExecConnection: SSHExecConnection, @unchecked Sendable {
    private let lock = NSLock()
    private var open: [FakeExecChannel] = []
    private(set) var closed = false
    private var refusedCount = 0
    var refusals: Int { lock.withLock { refusedCount } }
    private let maxSessions: Int
    private let respond: @Sendable (FakeExecChannel, Data) -> Void
    private let onOpen: @Sendable (FakeExecChannel) -> Void
    let onDisconnect: @Sendable () -> Void

    init(
        maxSessions: Int, respond: @escaping @Sendable (FakeExecChannel, Data) -> Void,
        onOpen: @escaping @Sendable (FakeExecChannel) -> Void = { _ in },
        onDisconnect: @escaping @Sendable () -> Void
    ) {
        self.maxSessions = maxSessions
        self.respond = respond
        self.onOpen = onOpen
        self.onDisconnect = onDisconnect
    }

    func openExec(_ command: String) async throws -> any SSHExecChannel {
        let channel: FakeExecChannel = try lock.withLock {
            open.removeAll { $0.isEnded }
            guard !closed else { throw DaemonClientError.eof }
            guard open.count < maxSessions else {
                refusedCount += 1
                throw SSHSessionLimitReached()
            }
            let channel = FakeExecChannel(command: command, respond: respond)
            open.append(channel)
            return channel
        }
        onOpen(channel)
        return channel
    }

    func close() {
        let channels = lock.withLock {
            closed = true
            return open
        }
        for channel in channels { channel.end(.disconnected) }
        onDisconnect()
    }

    /// The network dropped: every channel fails, as NIOSSH reports it.
    func drop() { close() }

    var isClosed: Bool { lock.withLock { closed } }
}

final class FakeConnector: SSHConnector, @unchecked Sendable {
    private let lock = NSLock()
    private(set) var connections: [FakeExecConnection] = []
    private var attemptCount = 0
    var failure: Error?
    let maxSessions: Int
    let respond: @Sendable (FakeExecChannel, Data) -> Void
    let onOpen: @Sendable (FakeExecChannel) -> Void

    init(
        maxSessions: Int = 10, respond: @escaping @Sendable (FakeExecChannel, Data) -> Void = { _, _ in },
        onOpen: @escaping @Sendable (FakeExecChannel) -> Void = { _ in }
    ) {
        self.maxSessions = maxSessions
        self.respond = respond
        self.onOpen = onOpen
    }

    func connect(onDisconnect: @escaping @Sendable () -> Void) async throws -> any SSHExecConnection {
        try lock.withLock {
            attemptCount += 1
            if let failure { throw failure }
            let connection = FakeExecConnection(
                maxSessions: maxSessions, respond: respond, onOpen: onOpen, onDisconnect: onDisconnect)
            connections.append(connection)
            return connection
        }
    }

    var attempts: Int { lock.withLock { attemptCount } }
    var all: [FakeExecConnection] { lock.withLock { connections } }
    func setFailure(_ error: Error?) { lock.withLock { failure = error } }
}

/// A clock tests move by hand.
final class FakeClock: @unchecked Sendable {
    private let lock = NSLock()
    private var current = Date(timeIntervalSince1970: 1_000_000)
    var now: Date { lock.withLock { current } }
    func advance(_ seconds: TimeInterval) { lock.withLock { current += seconds } }
}

/// A relay to a daemon that answers every request line with a health report.
let healthRelay: @Sendable (FakeExecChannel, Data) -> Void = { channel, data in
    guard data.contains(0x0A) else { return }
    channel.emit(Data(#"{"type":"health_report","pid":7,"uptime_seconds":1,"protocol_version":\#(DaemonProtocol.version),"agent_count":0}"#.utf8) + [0x0A])
}

func eventually(_ condition: () async -> Bool) async {
    for _ in 0..<200 {
        if await condition() { return }
        try? await Task.sleep(for: .milliseconds(10))
    }
}

// MARK: - Backoff and errors

struct SSHReconnectBackoffTests {
    @Test func scheduleDoublesToThirtySeconds() {
        #expect((0...8).map { SSHReconnectBackoff.delay(afterFailures: $0) } == [0, 1, 2, 4, 8, 16, 30, 30, 30])
    }

    @Test func failuresDelayAndSuccessResets() {
        let start = Date(timeIntervalSince1970: 0)
        var backoff = SSHReconnectBackoff()
        #expect(backoff.remainingDelay(at: start) == 0)
        backoff.recordFailure(at: start)
        backoff.recordFailure(at: start)
        #expect(backoff.remainingDelay(at: start) == 2)
        #expect(backoff.remainingDelay(at: start + 1.5) == 0.5)
        backoff.recordSuccess()
        #expect(backoff.remainingDelay(at: start) == 0)
        #expect(backoff.failures == 0)
    }
}

struct SSHRelayErrorTests {
    @Test func relayEndsMapToUnixSocketErrors() {
        func map(_ end: SSHExecEnd) -> Error { SSHRelayErrors.error(for: end, machine: "studio") }
        #expect(map(.exited(0, stderr: "")) as? DaemonClientError == .eof)
        #expect(map(.disconnected) as? DaemonClientError == .eof)
        #expect(map(.exited(69, stderr: "pu: no daemon")) as? DaemonClientError == .notRunning)
        #expect(map(.closedLocally) as? DaemonClientError == .cancelled)
    }

    @Test func missingPuNamesTheMachine() {
        let error = SSHRelayErrors.error(for: .exited(127, stderr: "sh: pu: not found"), machine: "studio")
        #expect(error as? SSHTransportError == .puNotInstalled(machine: "studio"))
        #expect(error.localizedDescription.contains("studio"))
        #expect(error.localizedDescription.contains("not installed"))
    }

    @Test func otherStatusesCarryTheLastStderrLine() {
        let error = SSHRelayErrors.error(for: .exited(1, stderr: "warming up\nproxy: socket error\n"), machine: "m")
        #expect(error as? SSHTransportError == .relayFailed(machine: "m", status: 1, detail: "proxy: socket error"))
    }

    @Test func relayCommandIsTheSpecifiedShellWrapper() {
        #expect(
            SSHExecTransport.relayCommand
                == #"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy'"#)
        #expect(
            MachineSSHCommands.startDaemonCommand
                == #"/bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu health'"#)
    }
}

// MARK: - Session

struct SSHMachineSessionTests {
    @Test func channelsShareOnePrimaryConnection() async throws {
        let connector = FakeConnector()
        let session = SSHMachineSession(connector: connector)
        let a = try await session.openChannel("relay")
        let b = try await session.openChannel("relay")
        #expect(connector.attempts == 1)
        #expect(a.onPrimary && b.onPrimary)
        #expect(await session.channelCounts == [2])
    }

    @Test func refusedChannelOpensOverflowAndIdleOverflowCloses() async throws {
        let connector = FakeConnector(maxSessions: 2)
        let session = SSHMachineSession(connector: connector)
        let first = try await session.openChannel("relay")
        _ = try await session.openChannel("relay")
        let third = try await session.openChannel("relay")
        #expect(!third.onPrimary)
        #expect(connector.attempts == 2)
        #expect(await session.channelCounts == [2, 1])
        // Space on the primary again: the next channel goes there, not to a new connection.
        first.channel.close()
        first.release()
        await eventually { await session.channelCounts == [1, 1] }
        let fourth = try await session.openChannel("relay")
        #expect(fourth.onPrimary)
        // The overflow connection closes once it carries nothing.
        third.channel.close()
        third.release()
        await eventually { await session.overflowCount == 0 }
        #expect(connector.all[1].isClosed)
        #expect(!connector.all[0].isClosed)
    }

    @Test func aBurstOfOpensAddsOnlyTheConnectionsItNeeds() async throws {
        let connector = FakeConnector(maxSessions: 10)
        let session = SSHMachineSession(connector: connector)
        let first = try await session.openChannel("relay")
        let opened = try await withThrowingTaskGroup(of: SSHOpenedChannel.self) { group in
            for _ in 0..<29 { group.addTask { try await session.openChannel("relay") } }
            return try await group.reduce(into: [first]) { $0.append($1) }
        }
        #expect(connector.attempts == 3)
        #expect(await session.channelCounts == [10, 10, 10])

        // With each connection's limit learned, a later burst asks no full connection.
        let refusalsBefore = connector.all.map(\.refusals).reduce(0, +)
        // Learning the limit costs at most one probe window of refusals, on the primary only.
        #expect(refusalsBefore <= SSHMachineSession.probeWindow)
        #expect(connector.all.dropFirst().allSatisfy { $0.refusals == 0 })
        for channel in opened.filter(\.onPrimary).prefix(5) {
            channel.channel.close()
            channel.release()
        }
        await eventually { await session.channelCounts == [5, 10, 10] }
        let again = try await withThrowingTaskGroup(of: Bool.self) { group in
            for _ in 0..<5 { group.addTask { try await session.openChannel("relay").onPrimary } }
            return try await group.reduce(into: [Bool]()) { $0.append($1) }
        }
        #expect(again == [true, true, true, true, true])
        #expect(connector.all.map(\.refusals).reduce(0, +) == refusalsBefore)
        #expect(connector.attempts == 3)
    }

    @Test func maxSessionsOneStillWorksAtOneConnectionPerStream() async throws {
        let connector = FakeConnector(maxSessions: 1)
        let session = SSHMachineSession(connector: connector)
        for _ in 0..<4 { _ = try await session.openChannel("relay") }
        #expect(connector.attempts == 4)
        #expect(await session.channelCounts == [1, 1, 1, 1])
    }

    @Test func droppedSessionFailsStreamsLikeAClosedSocketThenReconnects() async throws {
        let connector = FakeConnector(respond: healthRelay)
        let session = SSHMachineSession(connector: connector)
        let opened = try await session.openChannel("relay")
        let stream = SSHRelayByteStream(opened: opened, machineName: "studio")
        connector.all[0].drop()
        await #expect(throws: DaemonClientError.eof) { try await stream.receive() }
        await #expect(throws: DaemonClientError.eof) { try await stream.send(Data("x\n".utf8)) }
        await eventually { await !session.isConnected }
        _ = try await session.openChannel("relay")
        #expect(connector.attempts == 2)
    }

    @Test func failedConnectsBackOffAndFailFast() async throws {
        let clock = FakeClock()
        let connector = FakeConnector()
        connector.setFailure(SSHTransportError.connectFailed(machine: "m", detail: "refused"))
        let session = SSHMachineSession(connector: connector, now: { clock.now })
        await #expect(throws: SSHTransportError.connectFailed(machine: "m", detail: "refused")) {
            try await session.openChannel("relay")
        }
        // Inside the backoff window streams fail at once, without dialing again.
        await #expect(throws: SSHTransportError.connectFailed(machine: "m", detail: "refused")) {
            try await session.openChannel("relay")
        }
        #expect(connector.attempts == 1)
        clock.advance(1.1)
        connector.setFailure(nil)
        _ = try await session.openChannel("relay")
        #expect(connector.attempts == 2)
        #expect(await session.backoffState.failures == 0)
    }

    @Test func hostKeyRefusalPassesThroughUnchanged() async throws {
        let key = SSHPublicKey(type: "ssh-ed25519", blob: SSHWire.string("ssh-ed25519") + SSHWire.string(Data(count: 32)))
        let connector = FakeConnector()
        connector.setFailure(SSHTrustError.hostKeyUnconfirmed(key))
        let session = SSHMachineSession(connector: connector)
        await #expect(throws: SSHTrustError.hostKeyUnconfirmed(key)) { try await session.connect() }
    }
}

// MARK: - Transport

/// Each test gets its own session registry, so nothing outside it sees these machines.
struct SSHExecTransportTests {
    private func client(_ connector: FakeConnector, name: String = "studio") -> (DaemonClient, SSHMachineSession) {
        let sessions = SSHMachineSessions()
        let id = UUID()
        let session = SSHMachineSession(connector: connector)
        sessions.register(session, machineId: id, machineName: name)
        return (DaemonClient(transport: SSHExecTransport(machineId: id, sessions: sessions)), session)
    }

    @Test func requestsRunTheRelayCommandAndFrameNewlineJSON() async throws {
        let connector = FakeConnector(respond: healthRelay)
        let (client, _) = client(connector)
        guard case .healthReport(let pid, _, let version, _) = try await client.send(.health) else {
            Issue.record("expected a health report")
            return
        }
        #expect(pid == 7)
        #expect(version == DaemonProtocol.version)
    }

    @Test func noDaemonReadsAsNotRunning() async throws {
        let connector = FakeConnector { channel, _ in channel.end(.exited(69, stderr: "no daemon")) }
        let (client, _) = client(connector)
        await #expect(throws: DaemonClientError.notRunning) { try await client.send(.health) }
    }

    @Test func missingPuIsActionable() async throws {
        let connector = FakeConnector { channel, _ in channel.end(.exited(127, stderr: "sh: pu: not found")) }
        let (client, _) = client(connector, name: "build-box")
        await #expect(throws: SSHTransportError.puNotInstalled(machine: "build-box")) { try await client.send(.health) }
    }

    @Test func unregisteredMachineIsNotRunning() async throws {
        let client = DaemonClient(transport: SSHExecTransport(machineId: UUID(), sessions: SSHMachineSessions()))
        await #expect(throws: DaemonClientError.notRunning) { try await client.send(.health) }
    }

    @Test func requestChannelsAreReleasedAfterEachRequest() async throws {
        let connector = FakeConnector(maxSessions: 1, respond: healthRelay)
        let (client, session) = client(connector)
        for _ in 0..<3 { _ = try await client.send(.health) }
        await eventually { await session.channelCounts == [0] }
        #expect(connector.attempts == 1)
    }

    @Test func sendBlockingWorksFromAGCDThread() async throws {
        let connector = FakeConnector(respond: healthRelay)
        let (client, _) = client(connector)
        let response = try await withCheckedThrowingContinuation { (cont: CheckedContinuation<DaemonResponse, Error>) in
            DispatchQueue.global().async { cont.resume(with: Result { try client.sendBlocking(.health, timeout: 5) }) }
        }
        guard case .healthReport = response else {
            Issue.record("expected a health report")
            return
        }
    }
}

// MARK: - Local endpoint

@Suite(.serialized) struct UnreachableLocalTransportTests {
    @Test func aLocalClientOnIOSReportsNotRunning() async throws {
        UnreachableLocalTransport.trapsInDebug = false
        defer { UnreachableLocalTransport.trapsInDebug = true }
        #expect(DaemonEndpoint.local.transport is UnreachableLocalTransport)
        #expect(DaemonClient().transport is UnreachableLocalTransport)
        await #expect(throws: DaemonClientError.notRunning) { try await DaemonEndpoint.local.makeClient().send(.health) }
    }

    @Test func machineEndpointsResolveToSSHAtAppStart() {
        #expect(DaemonEndpoint.machine(UUID()).transport is SSHExecTransport)
    }
}
#endif
