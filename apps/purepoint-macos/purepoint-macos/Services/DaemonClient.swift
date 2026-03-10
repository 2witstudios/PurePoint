import Foundation
import Network

// MARK: - DaemonClient

nonisolated final class DaemonClient: @unchecked Sendable {
    static let connectionQueue = DispatchQueue(label: "purepoint.daemon.connection")
    private let socketPath: String

    init(socketPath: String? = nil) {
        self.socketPath =
            socketPath
            ?? {
                let home = FileManager.default.homeDirectoryForCurrentUser.path
                return "\(home)/.pu/daemon.sock"
            }()
    }

    /// Send a single request and return the response.
    func send(_ request: DaemonRequest) async throws -> DaemonResponse {
        let (connection, reader) = try await connect()
        defer { connection.cancel() }

        try await Self.write(request, to: connection)
        return try await readOne(from: reader)
    }

    /// Connect to the daemon and return the connection + a line reader.
    func connect() async throws -> (NWConnection, DaemonLineReader) {
        let params = NWParameters(tls: nil, tcp: NWProtocolTCP.Options())
        let endpoint = NWEndpoint.unix(path: socketPath)
        let connection = NWConnection(to: endpoint, using: params)

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
        }

        let reader = DaemonLineReader(connection: connection)
        return (connection, reader)
    }

    // MARK: - Private

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

// MARK: - Errors

nonisolated enum DaemonClientError: Error, LocalizedError {
    case eof
    case cancelled
    case notRunning

    var errorDescription: String? {
        switch self {
        case .eof: "Connection to daemon closed"
        case .cancelled: "Connection cancelled"
        case .notRunning: "Daemon is not running"
        }
    }
}
