import Foundation
import Network
import SwiftTerm

/// Manages a single attach connection per terminal view.
/// Connects to the daemon, streams Output messages, and feeds bytes to the terminal.
/// Actor isolation serializes all writes to the NWConnection, preventing garbled JSON.
actor DaemonAttachSession {
    let agentId: String
    private weak var terminalView: TerminalView?
    private var connection: NWConnection?
    private var stopped = false
    private var onFirstOutput: (() -> Void)?
    private var hasReceivedOutput = false

    init(agentId: String, terminalView: TerminalView, onFirstOutput: (() -> Void)? = nil) {
        self.agentId = agentId
        self.terminalView = terminalView
        self.onFirstOutput = onFirstOutput
    }

    /// Start streaming output from the daemon to the terminal view.
    func start() async {
        guard !stopped else { return }

        let fastRetries = 5
        let fastDelay: UInt64 = 100_000_000   // 100ms
        let slowDelay: UInt64 = 2_000_000_000 // 2s
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
                print("[DaemonAttach \(agentId.prefix(8))] retry \(retries): \(error.localizedDescription)")
                guard !stopped, retries <= maxRetries else { break }
                let delay = retries <= fastRetries ? fastDelay : slowDelay
                do {
                    try await Task.sleep(nanoseconds: delay)
                } catch {
                    break // CancellationError — exit immediately
                }
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
        self.connection?.cancel()
        self.connection = conn

        // Send attach request
        try await DaemonClient.write(.attach(agentId: agentId), to: conn)

        // Read AttachReady
        let firstLine = try await reader.readLine()
        let firstResponse = DaemonClient.parse(firstLine)
        guard case .attachReady = firstResponse else {
            if case .error(_, let msg) = firstResponse {
                print("[DaemonAttach \(agentId.prefix(8))] attach error: \(msg)")
                throw DaemonAttachError.attachFailed(msg)
            }
            print("[DaemonAttach \(agentId.prefix(8))] unexpected response: \(firstLine.prefix(100))")
            throw DaemonAttachError.unexpectedResponse
        }
        print("[DaemonAttach \(agentId.prefix(8))] attached successfully")

        // Send initial resize so PTY matches the terminal view's actual dimensions.
        // The sizeChanged delegate fires during terminal creation (before connection
        // exists), so this is the first opportunity to sync the PTY size.
        let (initialCols, initialRows) = await MainActor.run {
            guard let tv else { return (0, 0) }
            let term = tv.getTerminal()
            return (term.cols, term.rows)
        }
        if initialCols > 0 && initialRows > 0 {
            try await DaemonClient.write(
                .resize(agentId: agentId, cols: initialCols, rows: initialRows),
                to: conn
            )
        }

        // Stream loop
        while !stopped {
            let line = try await reader.readLine()
            let response = DaemonClient.parse(line)

            switch response {
            case .output(_, let data):
                if !hasReceivedOutput {
                    hasReceivedOutput = true
                    print("[DaemonAttach \(agentId.prefix(8))] first output: \(data.count) bytes")
                    if let cb = onFirstOutput {
                        onFirstOutput = nil
                        await MainActor.run { cb() }
                    }
                }
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
