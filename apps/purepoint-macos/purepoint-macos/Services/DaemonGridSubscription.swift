import Foundation
import Network

/// Subscribes to grid command events from the daemon and dispatches them to GridState.
/// Modeled after DaemonAttachSession — actor isolation serializes writes.
actor DaemonGridSubscription {
    let projectRoot: String
    private weak var gridState: GridState?
    private var connection: NWConnection?
    private var stopped = false

    init(projectRoot: String, gridState: GridState) {
        self.projectRoot = projectRoot
        self.gridState = gridState
    }

    /// Start the subscription loop with reconnection.
    func start() async {
        guard !stopped else { return }

        var backoff: UInt64 = 500_000_000  // 0.5s
        let maxBackoff: UInt64 = 5_000_000_000  // 5s
        var retries = 0
        let maxRetries = 20

        while !stopped {
            do {
                try await runSubscriptionLoop()
                break  // Normal exit
            } catch is CancellationError {
                break
            } catch {
                retries += 1
                guard !stopped, retries <= maxRetries else { break }
                do {
                    try await Task.sleep(nanoseconds: backoff)
                } catch {
                    break
                }
                backoff = min(backoff * 2, maxBackoff)
            }
        }
    }

    /// Send a grid command through the subscription connection.
    func sendCommand(_ command: GridCommandPayload) async {
        guard let conn = connection else { return }
        try? await DaemonClient.write(
            .gridCommand(projectRoot: projectRoot, command: command),
            to: conn
        )
    }

    /// Stop the subscription.
    func stop() {
        stopped = true
        connection?.cancel()
        connection = nil
    }

    // MARK: - Private

    private func runSubscriptionLoop() async throws {
        let gs = self.gridState

        let client = DaemonClient()
        let (conn, reader) = try await client.connect()
        self.connection?.cancel()
        self.connection = conn

        // Send subscribe request
        try await DaemonClient.write(.subscribeGrid(projectRoot: projectRoot), to: conn)

        // Read GridSubscribed confirmation
        let firstLine = try await reader.readLine()
        let firstResponse = DaemonClient.parse(firstLine)
        guard case .gridSubscribed = firstResponse else {
            if case .error(_, let msg) = firstResponse {
                throw DaemonGridError.subscribeFailed(msg)
            }
            throw DaemonGridError.unexpectedResponse
        }

        // Stream loop — receive GridEvent messages
        while !stopped {
            let line = try await reader.readLine()
            let response = DaemonClient.parse(line)

            switch response {
            case .gridEvent(_, let command):
                await MainActor.run {
                    gs?.handleRemoteCommand(command)
                }
            case .error(_, let message):
                throw DaemonGridError.subscribeFailed(message)
            default:
                break
            }
        }
    }
}

enum DaemonGridError: Error, LocalizedError {
    case subscribeFailed(String)
    case unexpectedResponse

    var errorDescription: String? {
        switch self {
        case .subscribeFailed(let msg): "Grid subscribe failed: \(msg)"
        case .unexpectedResponse: "Unexpected response during grid subscribe"
        }
    }
}
