import Foundation
import Network
import SwiftTerm

/// Manages a single attach connection per terminal view.
/// Connects to the daemon, streams Output messages, and feeds bytes to the terminal.
/// Actor isolation serializes all writes to the NWConnection, preventing garbled JSON.
actor DaemonAttachSession {
    let agentId: String
    private weak var terminalView: TerminalView?
    private var streamTask: Task<Void, Never>?
    private var connection: NWConnection?
    private var stopped = false

    init(agentId: String, terminalView: TerminalView) {
        self.agentId = agentId
        self.terminalView = terminalView
    }

    /// Start streaming output from the daemon to the terminal view.
    func start() async {
        guard !stopped else { return }

        var backoff: UInt64 = 500_000_000 // 0.5s
        let maxBackoff: UInt64 = 5_000_000_000 // 5s
        var retries = 0
        let maxRetries = 20

        while !stopped {
            do {
                try await runAttachLoop()
                // Normal exit (agent completed) — don't reconnect
                break
            } catch is CancellationError {
                break
            } catch {
                // Connection lost — attempt reconnect with backoff
                retries += 1
                guard !stopped, retries <= maxRetries else { break }
                try? await Task.sleep(nanoseconds: backoff)
                backoff = min(backoff * 2, maxBackoff)
            }
        }
    }

    /// Send terminal input to the daemon. Awaiting ensures actor serializes writes.
    func sendInput(_ data: Data) async {
        guard let conn = connection else { return }
        try? await DaemonClient.write(.input(agentId: agentId, data: data), to: conn)
    }

    /// Send resize notification to the daemon. Awaiting ensures actor serializes writes.
    func sendResize(cols: Int, rows: Int) async {
        guard let conn = connection else { return }
        try? await DaemonClient.write(.resize(agentId: agentId, cols: cols, rows: rows), to: conn)
    }

    /// Stop the attach session.
    func stop() {
        stopped = true
        streamTask?.cancel()
        streamTask = nil
        connection?.cancel()
        connection = nil
    }

    // MARK: - Private

    private func runAttachLoop() async throws {
        // Capture terminal view reference before async work — avoids
        // crossing actor isolation boundary later.
        let tv = self.terminalView

        let client = DaemonClient()
        let (conn, reader) = try await client.connect()
        self.connection = conn

        // Send attach request
        try await DaemonClient.write(.attach(agentId: agentId), to: conn)

        // Read AttachReady
        let firstLine = try await reader.readLine()
        let firstResponse = DaemonClient.parse(firstLine)
        guard case .attachReady = firstResponse else {
            if case .error(_, let msg) = firstResponse {
                throw DaemonAttachError.attachFailed(msg)
            }
            throw DaemonAttachError.unexpectedResponse
        }

        // Stream loop
        while !stopped {
            let line = try await reader.readLine()
            let response = DaemonClient.parse(line)

            switch response {
            case .output(_, let data):
                let bytes = [UInt8](data)
                await MainActor.run {
                    tv?.feed(byteArray: ArraySlice(bytes))
                }
            case .error(_, let message):
                throw DaemonAttachError.attachFailed(message)
            default:
                break
            }
        }
    }
}

enum DaemonAttachError: Error, LocalizedError {
    case attachFailed(String)
    case unexpectedResponse

    var errorDescription: String? {
        switch self {
        case .attachFailed(let msg): "Attach failed: \(msg)"
        case .unexpectedResponse: "Unexpected response during attach"
        }
    }
}
