import Darwin
import Foundation
import Network

// MARK: - DaemonClient

nonisolated final class DaemonClient: @unchecked Sendable {
    static let connectionQueue = DispatchQueue(label: "purepoint.daemon.connection")
    static let connectTimeout: TimeInterval = 5
    let socketPath: String

    init(socketPath: String? = nil) {
        self.socketPath =
            socketPath
            ?? {
                let home = FileManager.default.homeDirectoryForCurrentUser.path
                return "\(home)/.pu/daemon.sock"
            }()
    }

    /// Send a single request and return the response.
    /// Cancellation-aware: NWConnection ignores task cancellation, so cancelling
    /// the task cancels the connection, which completes any pending connect or
    /// receive with an error instead of leaving the caller suspended.
    func send(_ request: DaemonRequest) async throws -> DaemonResponse {
        let connection = makeConnection()
        defer { connection.cancel() }

        return try await withTaskCancellationHandler {
            try Task.checkCancellation()
            let reader = try await Self.connect(connection)
            try await Self.write(request, to: connection)
            return try await readOne(from: reader)
        } onCancel: {
            connection.cancel()
        }
    }

    /// Blocking request/response for synchronous callers that already run off the main
    /// thread — GitService's process pipeline and remote project hosts. It uses a plain
    /// socket rather than NWConnection so it never waits on the cooperative thread pool.
    func sendBlocking(_ request: DaemonRequest, timeout: TimeInterval = 30) throws -> DaemonResponse {
        var payload = try JSONEncoder().encode(request)
        payload.append(0x0A)

        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonClientError.notRunning }
        defer { Darwin.close(fd) }

        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        let seconds = max(1, Int(timeout.rounded(.up)))
        var interval = timeval(tv_sec: seconds, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &interval, socklen_t(MemoryLayout<timeval>.size))
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &interval, socklen_t(MemoryLayout<timeval>.size))

        var address = sockaddr_un()
        let pathBytes = Array(socketPath.utf8)
        guard pathBytes.count < MemoryLayout.size(ofValue: address.sun_path) else {
            throw DaemonClientError.notRunning
        }
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: pathBytes)
        }
        let connected = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard connected == 0 else { throw DaemonClientError.notRunning }

        try payload.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
            guard let base = raw.baseAddress else { return }
            var offset = 0
            while offset < raw.count {
                let written = Darwin.write(fd, base + offset, raw.count - offset)
                if written < 0 {
                    if errno == EINTR { continue }
                    throw errno == EAGAIN ? DaemonClientError.requestTimeout : DaemonClientError.eof
                }
                offset += written
            }
        }

        var line = Data()
        var chunk = [UInt8](repeating: 0, count: 65_536)
        while true {
            let count = chunk.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
            if count < 0 {
                if errno == EINTR { continue }
                throw errno == EAGAIN ? DaemonClientError.requestTimeout : DaemonClientError.eof
            }
            if count == 0 { throw DaemonClientError.eof }
            if let newline = chunk[0..<count].firstIndex(of: 0x0A) {
                line.append(contentsOf: chunk[0..<newline])
                break
            }
            line.append(contentsOf: chunk[0..<count])
        }
        return Self.parse(line)
    }

    /// Connect to the daemon and return the connection + a line reader.
    func connect() async throws -> (NWConnection, DaemonLineReader) {
        let connection = makeConnection()
        let reader = try await Self.connect(connection)
        return (connection, reader)
    }

    // MARK: - Private

    private func makeConnection() -> NWConnection {
        let params = NWParameters(tls: nil, tcp: NWProtocolTCP.Options())
        let endpoint = NWEndpoint.unix(path: socketPath)
        return NWConnection(to: endpoint, using: params)
    }

    private static func connect(_ connection: NWConnection) async throws -> DaemonLineReader {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            // nonisolated(unsafe) is safe because the handler runs on the serial connectionQueue
            nonisolated(unsafe) var resumed = false
            connection.stateUpdateHandler = { state in
                guard !resumed else { return }
                switch state {
                case .ready:
                    resumed = true
                    cont.resume()
                case .failed(let err):
                    resumed = true
                    cont.resume(throwing: err)
                case .waiting(let err):
                    // Stale socket: file exists but no listener — fail fast
                    connection.cancel()
                    resumed = true
                    cont.resume(throwing: err)
                case .cancelled:
                    resumed = true
                    cont.resume(throwing: DaemonClientError.cancelled)
                default:
                    break
                }
            }
            connection.start(queue: DaemonClient.connectionQueue)
            // A daemon that has stopped accepting leaves the connection pending
            // forever; give up so callers see an error instead of hanging.
            DaemonClient.connectionQueue.asyncAfter(deadline: .now() + Self.connectTimeout) {
                guard !resumed else { return }
                resumed = true
                connection.cancel()
                cont.resume(throwing: DaemonClientError.connectTimeout)
            }
        }

        return DaemonLineReader(connection: connection)
    }

    static func write(_ request: DaemonRequest, to connection: NWConnection) async throws {
        let json = try JSONEncoder().encode(request)
        var message = json
        message.append(0x0A)  // newline

        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            connection.send(
                content: message,
                completion: .contentProcessed { error in
                    if let error {
                        cont.resume(throwing: error)
                    } else {
                        cont.resume()
                    }
                })
        }
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
