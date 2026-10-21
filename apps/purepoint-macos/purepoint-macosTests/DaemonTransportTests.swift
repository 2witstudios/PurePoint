import CoreGraphics
import Foundation
import SwiftTerm
import Testing

@testable import PurePoint

// MARK: - In-memory fixture

/// One direction of an in-memory duplex stream.
private final class InMemoryPipe: @unchecked Sendable {
    private let lock = NSLock()
    private var chunks: [Data] = []
    private var finished = false
    private var waiter: CheckedContinuation<Data, Error>?

    func push(_ data: Data) {
        lock.lock()
        if let waiter {
            self.waiter = nil
            lock.unlock()
            waiter.resume(returning: data)
            return
        }
        chunks.append(data)
        lock.unlock()
    }

    func finish() {
        lock.lock()
        finished = true
        let waiter = self.waiter
        self.waiter = nil
        lock.unlock()
        waiter?.resume(throwing: DaemonClientError.eof)
    }

    func pull() async throws -> Data {
        try await withCheckedThrowingContinuation { cont in
            lock.lock()
            if !chunks.isEmpty {
                let chunk = chunks.removeFirst()
                lock.unlock()
                cont.resume(returning: chunk)
            } else if finished {
                lock.unlock()
                cont.resume(throwing: DaemonClientError.eof)
            } else {
                waiter = cont
                lock.unlock()
            }
        }
    }
}

/// One end of an in-memory duplex stream: what it sends, the other end receives.
private final class InMemoryByteStream: DaemonByteStream {
    private let inbound: InMemoryPipe
    private let outbound: InMemoryPipe

    private init(inbound: InMemoryPipe, outbound: InMemoryPipe) {
        self.inbound = inbound
        self.outbound = outbound
    }

    static func pair() -> (InMemoryByteStream, InMemoryByteStream) {
        let a = InMemoryPipe()
        let b = InMemoryPipe()
        return (InMemoryByteStream(inbound: a, outbound: b), InMemoryByteStream(inbound: b, outbound: a))
    }

    func send(_ data: Data) async throws { outbound.push(data) }
    func receive() async throws -> Data { try await inbound.pull() }
    func close() {
        inbound.finish()
        outbound.finish()
    }
}

/// A transport whose every stream is served in-process by `serve`.
private struct InMemoryTransport: DaemonTransport {
    let serve: @Sendable (InMemoryByteStream) async throws -> Void

    func openStream(cancellable: Bool) async throws -> any DaemonByteStream {
        let (client, server) = InMemoryByteStream.pair()
        Task { try? await serve(server) }
        return client
    }
}

/// Counts the streams a transport opened.
private final class OpenCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var count = 0
    func increment() -> Int { lock.withLock { count += 1; return count } }
    var value: Int { lock.withLock { count } }
}

/// A daemon that answers one `health` request with a health report, split across chunks the
/// way a socket may deliver it, followed by bytes past the newline that must be ignored.
private let healthDaemon: @Sendable (InMemoryByteStream) async throws -> Void = { server in
    let reader = DaemonLineReader(stream: server)
    let request = try await reader.readLine()
    let object = try JSONSerialization.jsonObject(with: request) as? [String: Any]
    guard object?["type"] as? String == "health" else {
        try await server.send(Data(#"{"type":"error","code":"BAD","message":"unexpected"}"#.utf8) + [0x0A])
        return
    }
    try await server.send(Data(#"{"type":"health_report","pid":42,"#.utf8))
    try await server.send(Data(#""uptime_seconds":7,"protocol_version":1,"agent_count":2}"#.utf8) + [0x0A])
    try await server.send(Data("trailing".utf8))
}

/// Run a blocking call on a plain thread, as GitService and remote hosts do.
private func offMain<T: Sendable>(_ body: @escaping @Sendable () throws -> T) async throws -> T {
    try await withCheckedThrowingContinuation { cont in
        DispatchQueue.global().async { cont.resume(with: Result { try body() }) }
    }
}

private func expectHealth(_ response: DaemonResponse) {
    guard case .healthReport(let pid, let uptime, let version, let count) = response else {
        Issue.record("expected healthReport, got \(response)")
        return
    }
    #expect(pid == 42)
    #expect(uptime == 7)
    #expect(version == 1)
    #expect(count == 2)
}

// MARK: - Framing

struct DaemonTransportTests {

    @Test func sendRoundTripsRequestAndResponseOverTransport() async throws {
        let client = DaemonClient(transport: InMemoryTransport(serve: healthDaemon))
        expectHealth(try await client.send(.health))
    }

    @Test func sendBlockingRoundTripsOverBridgedTransport() async throws {
        let client = DaemonClient(transport: InMemoryTransport(serve: healthDaemon))
        expectHealth(try await offMain { try client.sendBlocking(.health, timeout: 5) })
    }

    @Test func sendBlockingTimesOutWhenDaemonNeverAnswers() async throws {
        let silent = InMemoryTransport { server in _ = try await server.receive() }
        let client = DaemonClient(transport: silent)
        await #expect(throws: DaemonClientError.requestTimeout) {
            try await offMain { try client.sendBlocking(.health, timeout: 1) }
        }
    }

    @Test func sendSurfacesEofWhenDaemonClosesStream() async throws {
        let closing = InMemoryTransport { server in
            _ = try await server.receive()
            server.close()
        }
        let client = DaemonClient(transport: closing)
        await #expect(throws: DaemonClientError.eof) { try await client.send(.health) }
    }

    @Test func lineReaderSplitsAndJoinsChunks() async throws {
        let (client, server) = InMemoryByteStream.pair()
        let reader = DaemonLineReader(stream: client)
        try await server.send(Data("one\ntw".utf8))
        try await server.send(Data("o\nthree\n".utf8))
        server.close()
        #expect(String(decoding: try await reader.readLine(), as: UTF8.self) == "one")
        #expect(String(decoding: try await reader.readLine(), as: UTF8.self) == "two")
        #expect(String(decoding: try await reader.readLine(), as: UTF8.self) == "three")
        await #expect(throws: DaemonClientError.eof) { try await reader.readLine() }
    }

    // MARK: - Unix socket

    #if os(macOS)
    @Test func defaultTransportIsUnixSocketAtClientPath() {
        let client = DaemonClient(socketPath: "/tmp/pu-test.sock")
        #expect(client.transport as? UnixSocketTransport == UnixSocketTransport(socketPath: "/tmp/pu-test.sock"))
    }

    @Test func unixSendBlockingWithoutListenerIsNotRunning() async throws {
        let path = NSTemporaryDirectory() + "pu-missing-\(UUID().uuidString.prefix(8)).sock"
        let client = DaemonClient(socketPath: path)
        await #expect(throws: DaemonClientError.notRunning) {
            try await offMain { try client.sendBlocking(.health, timeout: 1) }
        }
    }
    #endif
}

// MARK: - Long-lived streams

/// A daemon that confirms one subscription with `reply`, then closes the stream.
private func confirmThenClose(_ reply: String, opens: OpenCounter) -> InMemoryTransport {
    InMemoryTransport { server in
        _ = opens.increment()
        _ = try await DaemonLineReader(stream: server).readLine()
        try await server.send(Data(reply.utf8) + [0x0A])
        server.close()
    }
}

private func waitUntil(timeout: Duration = .seconds(10), _ condition: () async -> Bool) async {
    let deadline = ContinuousClock.now + timeout
    while ContinuousClock.now < deadline, !(await condition()) {
        try? await Task.sleep(for: .milliseconds(20))
    }
}

struct DaemonStreamTransportTests {

    @Test func gridSubscriptionReconnectsOverTransportAfterStreamCloses() async throws {
        let opens = OpenCounter()
        let client = DaemonClient(transport: confirmThenClose(#"{"type":"grid_subscribed"}"#, opens: opens))
        let registry = await WorkspaceRegistry(loadWorkspaces: { _ in [] })
        let subscription = DaemonGridSubscription(projectRoot: "/p", registry: registry, client: client)
        let task = Task { await subscription.start() }
        await waitUntil { opens.value >= 2 }
        await subscription.stop()
        task.cancel()
        #expect(opens.value >= 2)
    }

    @Test func statusSubscriptionReportsDaemonLostWhenEstablishedStreamCloses() async throws {
        let opens = OpenCounter()
        let client = DaemonClient(transport: confirmThenClose(#"{"type":"status_subscribed"}"#, opens: opens))
        let subscription = DaemonStatusSubscription(projectRoot: "/p", client: client)
        let lost = OpenCounter()
        await subscription.start(onEvent: { _, _ in }, onDaemonLost: { _ = lost.increment() })
        #expect(lost.value == 1)
        #expect(opens.value == 1)
    }

    @Test func statusSubscriptionSurfacesSubscribeErrorWithoutRetrying() async throws {
        let opens = OpenCounter()
        let client = DaemonClient(
            transport: confirmThenClose(#"{"type":"error","code":"NOPE","message":"no"}"#, opens: opens))
        let subscription = DaemonStatusSubscription(projectRoot: "/p", client: client)
        let lost = OpenCounter()
        await subscription.start(onEvent: { _, _ in }, onDaemonLost: { _ = lost.increment() })
        #expect(lost.value == 0)
        #expect(opens.value == 1)
    }

    @MainActor @Test func attachSessionEndsWithoutReattachWhenDaemonClosesStream() async throws {
        let opens = OpenCounter()
        let client = DaemonClient(
            transport: confirmThenClose(#"{"type":"attach_ready","buffered_bytes":0}"#, opens: opens))
        let terminal = PlatformTerminalView(frame: .init(x: 0, y: 0, width: 200, height: 100))
        let session = DaemonAttachSession(agentId: "ag-1", terminalView: terminal, client: client)
        await session.start()
        #expect(await session.didStreamEnd)
        #expect(await session.isAgentGone == false)
        #expect(opens.value == 1)
    }

    @MainActor @Test func attachSessionStopsRetryingWhenAgentIsGone() async throws {
        let opens = OpenCounter()
        let client = DaemonClient(
            transport: confirmThenClose(#"{"type":"error","code":"AGENT_NOT_FOUND","message":"gone"}"#, opens: opens))
        let terminal = PlatformTerminalView(frame: .init(x: 0, y: 0, width: 200, height: 100))
        let session = DaemonAttachSession(agentId: "ag-1", terminalView: terminal, client: client)
        await session.start()
        #expect(await session.isAgentGone)
        #expect(opens.value == 1)
    }
}

// MARK: - Endpoint resolution

/// Providers are injected through the task-local override, so neither other suites nor the
/// hosted app can observe them, and nothing here mutates the process-wide hook.
struct DaemonEndpointTransportTests {
    private let machineId = UUID(uuidString: "6F1C2B3A-1111-4222-8333-944455556666")!

    #if os(macOS)
    @Test func macOSEndpointsResolveToUnixSocketsAtTodaysPaths() {
        // The Mac app never installs a provider.
        #expect(DaemonEndpoint.transportProvider == nil)

        let home = FileManager.default.homeDirectoryForCurrentUser.path
        let local = DaemonEndpoint.local
        #expect(local.transport as? UnixSocketTransport == UnixSocketTransport(socketPath: home + "/.pu/daemon.sock"))
        #expect(local.makeClient().transport as? UnixSocketTransport == UnixSocketTransport(socketPath: home + "/.pu/daemon.sock"))
        #expect(local.makeClient().socketPath == DaemonEndpoint.localSocketPath)

        let machinePath = home + "/.pu/machines/6f1c2b3a-1111-4222-8333-944455556666/daemon.sock"
        let machine = DaemonEndpoint.machine(machineId)
        #expect(machine.transport as? UnixSocketTransport == UnixSocketTransport(socketPath: machinePath))
        #expect(machine.makeClient().transport as? UnixSocketTransport == UnixSocketTransport(socketPath: machinePath))
    }
    #endif

    @Test func injectedProviderResolvesMachineEndpointsWithoutChangingCallSites() async throws {
        let seen = EndpointLog()
        let provider: DaemonEndpoint.TransportProvider = { endpoint in
            seen.append(endpoint)
            return InMemoryTransport(serve: healthDaemon)
        }
        let client = DaemonEndpoint.$transportOverride.withValue(provider) {
            DaemonEndpoint.machine(machineId).makeClient()
        }
        #expect(client.transport is InMemoryTransport)
        #expect(seen.endpoints == [DaemonEndpoint.machine(machineId)])
        expectHealth(try await client.send(.health))
        // Outside the scope the override is gone.
        #expect(!(DaemonEndpoint.machine(machineId).transport is InMemoryTransport))
    }

    #if os(iOS)
    @Test func iOSMachineEndpointsGoOverSSH() {
        #expect((DaemonEndpoint.machine(machineId).transport as? SSHExecTransport)?.machineId == machineId)
        #expect(DaemonEndpoint.local.transport is UnreachableLocalTransport)
    }

    @Test func iOSClientNamingAMachineSocketReachesThatMachineOverSSH() {
        let client = DaemonClient(socketPath: MachinePaths.socketPath(machineId: machineId))
        #expect((client.transport as? SSHExecTransport)?.machineId == machineId)
    }

    @Test func iOSClientNamingAnyOtherSocketNeverOpensAUnixSocket() async {
        UnreachableLocalTransport.trapsInDebug = false
        defer { UnreachableLocalTransport.trapsInDebug = true }
        let client = DaemonClient(socketPath: NSTemporaryDirectory() + "pu-missing.sock")
        #expect(client.transport is UnreachableLocalTransport)
        await #expect(throws: DaemonClientError.notRunning) { _ = try await client.send(.health) }
    }
    #endif
}

/// Endpoints a provider was asked for, asserted after the fact rather than inside it.
private final class EndpointLog: @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [DaemonEndpoint] = []
    func append(_ endpoint: DaemonEndpoint) { lock.withLock { recorded.append(endpoint) } }
    var endpoints: [DaemonEndpoint] { lock.withLock { recorded } }
}
