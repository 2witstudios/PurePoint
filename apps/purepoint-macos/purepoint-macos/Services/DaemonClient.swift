import Foundation

// MARK: - DaemonClient

nonisolated final class DaemonClient: @unchecked Sendable {
    static let connectionQueue = DispatchQueue(label: "purepoint.daemon.connection")
    static let connectTimeout: TimeInterval = 5
    /// The daemon's socket. It also identifies the endpoint when another transport carries it.
    let socketPath: String
    let transport: any DaemonTransport

    init(socketPath: String? = nil, transport: (any DaemonTransport)? = nil) {
        let socketPath =
            socketPath
            ?? {
                let home = FileManager.default.homeDirectoryForCurrentUser.path
                return "\(home)/.pu/daemon.sock"
            }()
        self.socketPath = socketPath
        self.transport = transport ?? UnixSocketTransport(socketPath: socketPath)
    }

    /// Send a single request and return the response.
    /// Cancellation-aware: cancelling the task aborts a pending connect and closes the
    /// stream, which completes any pending receive with an error instead of leaving the
    /// caller suspended.
    func send(_ request: DaemonRequest) async throws -> DaemonResponse {
        let holder = StreamHolder()
        defer { holder.close() }

        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            let stream = try await transport.openStream(cancellable: true)
            holder.hold(stream)
            let reader = DaemonLineReader(stream: stream)
            try await Self.write(request, to: stream)
            return try await readOne(from: reader)
        } onCancel: {
            holder.close()
        }
    }

    /// Blocking request/response for synchronous callers that already run off the main
    /// thread — GitService's process pipeline and remote project hosts. On a Unix socket it
    /// uses a plain socket rather than NWConnection so it never waits on the cooperative
    /// thread pool.
    func sendBlocking(_ request: DaemonRequest, timeout: TimeInterval = 30) throws -> DaemonResponse {
        var payload = try JSONEncoder().encode(request)
        payload.append(0x0A)

        let stream = try transport.openBlockingStream(timeout: timeout)
        defer { stream.close() }

        try stream.write(payload)

        var line = Data()
        while true {
            let chunk = try stream.read()
            if let newline = chunk.firstIndex(of: 0x0A) {
                line.append(chunk[chunk.startIndex..<newline])
                break
            }
            line.append(chunk)
        }
        return Self.parse(line)
    }

    /// Open a long-lived stream to the daemon and return it with a line reader. The connect
    /// runs to completion or its timeout regardless of task cancellation.
    func connect() async throws -> (any DaemonByteStream, DaemonLineReader) {
        let stream = try await transport.openStream()
        return (stream, DaemonLineReader(stream: stream))
    }

    // MARK: - Private

    static func write(_ request: DaemonRequest, to stream: any DaemonByteStream) async throws {
        let json = try JSONEncoder().encode(request)
        var message = json
        message.append(0x0A)  // newline
        try await stream.send(message)
    }

    private func readOne(from reader: DaemonLineReader) async throws -> DaemonResponse {
        let line = try await reader.readLine()
        return Self.parse(line)
    }

    static func parse(_ data: Data) -> DaemonResponse {
        do {
            return try JSONDecoder().decode(DaemonResponse.self, from: data)
        } catch {
            let preview = String(data: data.prefix(200), encoding: .utf8) ?? "<binary>"
            print("[DaemonClient] parse error: \(error)\n  raw: \(preview)")
            return .unknown(type: "parse_error")
        }
    }
}

/// The stream a `send` is using, so task cancellation can close it from another thread. A
/// stream handed over after cancellation is closed at once.
nonisolated private final class StreamHolder: @unchecked Sendable {
    private let lock = NSLock()
    private var stream: (any DaemonByteStream)?
    private var closed = false

    func hold(_ stream: any DaemonByteStream) {
        let closeNow = lock.withLock {
            if !closed { self.stream = stream }
            return closed
        }
        if closeNow { stream.close() }
    }

    func close() {
        let stream = lock.withLock {
            closed = true
            return self.stream
        }
        stream?.close()
    }
}
