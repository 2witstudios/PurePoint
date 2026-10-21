import Foundation
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
}
