import AppKit
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
    private(set) var isAgentGone = false
    private var onFirstOutput: (() -> Void)?
    private var lastFullRefreshAtNanos: UInt64 = 0
    private static let fullRefreshIntervalNanos: UInt64 = 200_000_000  // 200ms

    init(agentId: String, terminalView: TerminalView, onFirstOutput: (() -> Void)? = nil) {
        self.agentId = agentId
        self.terminalView = terminalView
        self.onFirstOutput = onFirstOutput
    }

    /// Start streaming output from the daemon to the terminal view.
    func start() async {
        guard !stopped else { return }

        let fastRetries = 5
        let fastDelay: UInt64 = 100_000_000  // 100ms
        let slowDelay: UInt64 = 2_000_000_000  // 2s
        var retries = 0
        let maxRetries = 20

        while !stopped {
            do {
                try await runAttachLoop()
                // Normal exit (agent completed) — don't reconnect
                break
            } catch is CancellationError {
                break
            } catch DaemonAttachError.agentGone {
                print("[DaemonAttach \(agentId.prefix(8))] agent gone — stopping retries")
                isAgentGone = true
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
                    break  // CancellationError — exit immediately
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
            if case .error(let code, let msg) = firstResponse {
                print("[DaemonAttach \(agentId.prefix(8))] attach error: \(msg)")
                if code == "AGENT_NOT_FOUND" {
                    throw DaemonAttachError.agentGone
                }
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
                guard !data.isEmpty else { continue }
                let now = DispatchTime.now().uptimeNanoseconds
                if let cb = onFirstOutput {
                    onFirstOutput = nil
                    print("[DaemonAttach \(agentId.prefix(8))] first output: \(data.count) bytes")
                    await MainActor.run { cb() }
                }
                let shouldForceFullRefresh = now &- lastFullRefreshAtNanos >= Self.fullRefreshIntervalNanos
                if shouldForceFullRefresh {
                    lastFullRefreshAtNanos = now
                }
                let bytes = [UInt8](data)
                let termRows = await MainActor.run { tv?.getTerminal().rows ?? 24 }
                let filtered = Self.filterTerminalOutput(bytes, maxRows: termRows)
                await MainActor.run {
                    guard let tv else { return }
                    let term = tv.getTerminal()

                    // SwiftTerm's own "stay put while the user has scrolled up" logic
                    // (Terminal.scroll(), gated on an internal userScrolling flag) is never
                    // engaged by mouse-wheel scrolling, so every feed() re-pins yDisp to the
                    // live tail. Snapshot/restore it here via the public scroll API so
                    // continuous output doesn't yank the viewport (and any selection in it)
                    // out from under the user. Never write term.buffer.yDisp directly — its
                    // setter skips the refresh/dirty-row/scroller bookkeeping that
                    // scrollUp/scrollDown perform.
                    let priorYDisp = term.buffer.yDisp
                    let wasScrolledAway = !term.isCurrentBufferAlternate && priorYDisp > 0

                    tv.feed(byteArray: ArraySlice(filtered))

                    if wasScrolledAway {
                        let delta = term.buffer.yDisp - priorYDisp
                        if delta > 0 {
                            tv.scrollUp(lines: delta)
                        } else if delta < 0 {
                            tv.scrollDown(lines: -delta)
                        }
                    }

                    // DEBUG: Log terminal buffer state to find desync
                    let buf = term.buffer
                    if buf.y == 0 && buf.yDisp > 0 {
                        // swiftlint:disable:next line_length
                        print(
                            "[TermDBG] CURSOR AT TOP: y=\(buf.y) x=\(buf.x) yDisp=\(buf.yDisp) scrollTop=\(buf.scrollTop) scrollBottom=\(buf.scrollBottom) rows=\(term.rows) cols=\(term.cols)"
                        )
                    }
                    tv.needsDisplay = true
                }
            case .error(let code, let message):
                if code == "AGENT_NOT_FOUND" {
                    throw DaemonAttachError.agentGone
                }
                throw DaemonAttachError.attachFailed(message)
            default:
                break
            }
        }
    }

    // MARK: - Terminal Output Filter

    /// Filter terminal output to work around SwiftTerm rendering issues:
    /// 1. Strip DEC 2026 synchronized output sequences (SwiftTerm#203 — sync buffer
    ///    snapshot mistracking causes cursor/scroll desync).
    /// 2. Clamp CSI n A (cursor-up) sequences so n never exceeds viewport rows.
    ///    Ink's eraseLines() emits cursor-up counts that can exceed viewport height,
    ///    causing the cursor to overshoot row 0 and desync the buffer.
    private static func filterTerminalOutput(_ bytes: [UInt8], maxRows: Int) -> [UInt8] {
        let syncBegin: [UInt8] = [0x1b, 0x5b, 0x3f, 0x32, 0x30, 0x32, 0x36, 0x68]
        let syncEnd: [UInt8] = [0x1b, 0x5b, 0x3f, 0x32, 0x30, 0x32, 0x36, 0x6c]
        let maxUp = max(maxRows - 1, 1)

        var result: [UInt8] = []
        result.reserveCapacity(bytes.count)
        var i = 0
        while i < bytes.count {
            // Strip DEC 2026 begin/end (8 bytes each)
            if i + 8 <= bytes.count {
                let slice = Array(bytes[i..<i + 8])
                if slice == syncBegin || slice == syncEnd {
                    i += 8
                    continue
                }
            }
            // Clamp CSI n A (cursor up): \x1b [ <digits> A
            if bytes[i] == 0x1b, i + 2 < bytes.count, bytes[i + 1] == 0x5b {
                var j = i + 2
                var digits = 0
                var hasDigits = false
                while j < bytes.count, bytes[j] >= 0x30, bytes[j] <= 0x39 {
                    digits = digits * 10 + Int(bytes[j] - 0x30)
                    hasDigits = true
                    j += 1
                }
                if j < bytes.count, bytes[j] == 0x41, hasDigits {
                    // It's CSI <n> A — clamp n
                    let clamped = min(digits, maxUp)
                    let replacement = Array("\u{1b}[\(clamped)A".utf8)
                    result.append(contentsOf: replacement)
                    i = j + 1
                    continue
                }
            }
            result.append(bytes[i])
            i += 1
        }
        return result
    }
}

enum DaemonAttachError: Error, LocalizedError {
    case attachFailed(String)
    case unexpectedResponse
    case agentGone

    var errorDescription: String? {
        switch self {
        case .attachFailed(let msg): "Attach failed: \(msg)"
        case .unexpectedResponse: "Unexpected response during attach"
        case .agentGone: "Agent no longer exists"
        }
    }
}
