import Darwin
import Foundation
import Network

/// A daemon reached over a Unix domain socket: the local daemon, or a machine's forwarded
/// socket. Async streams use `NWConnection`; blocking streams use a plain socket so they never
/// wait on the cooperative thread pool.
nonisolated struct UnixSocketTransport: DaemonTransport, Hashable {
    let socketPath: String

    func openStream(cancellable: Bool) async throws -> any DaemonByteStream {
        let connection = makeConnection()
        do {
            if cancellable {
                // NWConnection ignores task cancellation, so cancelling the task cancels the
                // connection, which completes the pending connect with an error instead of
                // leaving the caller suspended.
                try await withTaskCancellationHandler {
                    try await Self.connect(connection)
                } onCancel: {
                    connection.cancel()
                }
            } else {
                try await Self.connect(connection)
            }
        } catch {
            connection.cancel()
            throw error
        }
        return UnixSocketStream(connection: connection)
    }

    func openBlockingStream(timeout: TimeInterval) throws -> any DaemonBlockingByteStream {
        try UnixBlockingSocket(path: socketPath, timeout: timeout)
    }

    // MARK: - Private

    private func makeConnection() -> NWConnection {
        let params = NWParameters(tls: nil, tcp: NWProtocolTCP.Options())
        let endpoint = NWEndpoint.unix(path: socketPath)
        return NWConnection(to: endpoint, using: params)
    }

    private static func connect(_ connection: NWConnection) async throws {
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
            DaemonClient.connectionQueue.asyncAfter(deadline: .now() + DaemonClient.connectTimeout) {
                guard !resumed else { return }
                resumed = true
                connection.cancel()
                cont.resume(throwing: DaemonClientError.connectTimeout)
            }
        }
    }
}

// MARK: - Async stream

/// A connected `NWConnection` to a daemon socket.
nonisolated final class UnixSocketStream: DaemonByteStream {
    let connection: NWConnection

    init(connection: NWConnection) {
        self.connection = connection
    }

    func send(_ data: Data) async throws {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Void, Error>) in
            connection.send(
                content: data,
                completion: .contentProcessed { error in
                    if let error {
                        cont.resume(throwing: error)
                    } else {
                        cont.resume()
                    }
                })
        }
    }

    func receive() async throws -> Data {
        try await withCheckedThrowingContinuation { (cont: CheckedContinuation<Data, Error>) in
            connection.receive(minimumIncompleteLength: 1, maximumLength: 65536) { data, _, _, error in
                if let error {
                    cont.resume(throwing: error)
                } else if let data, !data.isEmpty {
                    cont.resume(returning: data)
                } else {
                    cont.resume(throwing: DaemonClientError.eof)
                }
            }
        }
    }

    func close() {
        connection.cancel()
    }
}

// MARK: - Blocking stream

/// A plain `AF_UNIX` socket with `SO_NOSIGPIPE` and send/receive timeouts.
nonisolated final class UnixBlockingSocket: DaemonBlockingByteStream {
    private var fd: Int32
    private var chunk = [UInt8](repeating: 0, count: 65_536)

    init(path: String, timeout: TimeInterval) throws {
        let fd = Darwin.socket(AF_UNIX, SOCK_STREAM, 0)
        guard fd >= 0 else { throw DaemonClientError.notRunning }
        var connected = false
        defer { if !connected { Darwin.close(fd) } }

        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, socklen_t(MemoryLayout<Int32>.size))
        let seconds = max(1, Int(timeout.rounded(.up)))
        var interval = timeval(tv_sec: seconds, tv_usec: 0)
        setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &interval, socklen_t(MemoryLayout<timeval>.size))
        setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &interval, socklen_t(MemoryLayout<timeval>.size))

        var address = sockaddr_un()
        let pathBytes = Array(path.utf8)
        guard pathBytes.count < MemoryLayout.size(ofValue: address.sun_path) else {
            throw DaemonClientError.notRunning
        }
        address.sun_family = sa_family_t(AF_UNIX)
        address.sun_len = UInt8(MemoryLayout<sockaddr_un>.size)
        withUnsafeMutableBytes(of: &address.sun_path) { raw in
            raw.copyBytes(from: pathBytes)
        }
        let result = withUnsafePointer(to: &address) { pointer in
            pointer.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                Darwin.connect(fd, $0, socklen_t(MemoryLayout<sockaddr_un>.size))
            }
        }
        guard result == 0 else { throw DaemonClientError.notRunning }
        connected = true
        self.fd = fd
    }

    deinit { close() }

    func write(_ data: Data) throws {
        try data.withUnsafeBytes { (raw: UnsafeRawBufferPointer) in
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
    }

    func read() throws -> Data {
        while true {
            let count = chunk.withUnsafeMutableBytes { Darwin.read(fd, $0.baseAddress, $0.count) }
            if count < 0 {
                if errno == EINTR { continue }
                throw errno == EAGAIN ? DaemonClientError.requestTimeout : DaemonClientError.eof
            }
            if count == 0 { throw DaemonClientError.eof }
            return Data(chunk[0..<count])
        }
    }

    func close() {
        guard fd >= 0 else { return }
        Darwin.close(fd)
        fd = -1
    }
}
