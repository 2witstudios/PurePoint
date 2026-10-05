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
    /// True once the daemon closed an attached stream on its own, which it does
    /// when the agent's process exits. Callers must not blindly reattach: each
    /// attach replays the agent's whole output buffer.
    private(set) var didStreamEnd = false
    private var onFirstOutput: (() -> Void)?
    /// Called on the main actor after each chunk of *live* output is fed to the terminal —
    /// never for the replay of the daemon's buffer that every attach starts with.
    private let onOutput: (@MainActor @Sendable () -> Void)?
    /// The daemon replays its whole buffer on every attach, so a terminal that
    /// already shows output must be reset first or the scrollback duplicates.
    private var resetBeforeReplay: Bool
    /// Mirrors the terminal's row count so the output filter doesn't need a
    /// main-thread hop per chunk. Updated on every resize.
    private var termRows = 24

    init(
        agentId: String,
        terminalView: TerminalView,
        resetBeforeReplay: Bool = false,
        onFirstOutput: (() -> Void)? = nil,
        onOutput: (@MainActor @Sendable () -> Void)? = nil
    ) {
        self.agentId = agentId
        self.terminalView = terminalView
        self.resetBeforeReplay = resetBeforeReplay
        self.onFirstOutput = onFirstOutput
        self.onOutput = onOutput
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
            } catch DaemonAttachError.streamEnded {
                // Daemon closed the stream after the agent exited — everything
                // has been delivered. Reattaching would only replay it.
                didStreamEnd = true
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
        if rows > 0 { termRows = rows }
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
        guard case .attachReady(let bufferedBytes) = firstResponse else {
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
        if initialRows > 0 { termRows = initialRows }
        if initialCols > 0 && initialRows > 0 {
            try await DaemonClient.write(
                .resize(agentId: agentId, cols: initialCols, rows: initialRows),
                to: conn
            )
        }

        // The daemon replays exactly `bufferedBytes` before live output begins.
        var replayRemaining = bufferedBytes

        // Stream loop
        var isFirstChunk = true
        while !stopped {
            let line: Data
            do {
                line = try await reader.readLine()
            } catch DaemonClientError.eof {
                throw DaemonAttachError.streamEnded
            }
            let response = DaemonClient.parse(line)

            switch response {
            case .output(_, let data):
                guard !data.isEmpty else { continue }
                if let cb = onFirstOutput {
                    onFirstOutput = nil
                    print("[DaemonAttach \(agentId.prefix(8))] first output: \(data.count) bytes")
                    await MainActor.run { cb() }
                }
                let filtered = Self.filterTerminalOutput([UInt8](data), maxRows: termRows)
                let reset = isFirstChunk && resetBeforeReplay
                if isFirstChunk {
                    isFirstChunk = false
                    // A retry within this session replays from the start again.
                    resetBeforeReplay = true
                }
                let isReplay = replayRemaining > 0
                replayRemaining -= data.count
                let onOutput = isReplay ? nil : self.onOutput
                await MainActor.run {
                    guard let tv else { return }
                    defer { onOutput?() }
                    let term = tv.getTerminal()
                    if reset {
                        // RIS: full reset, clears the screen and scrollback before the replay.
                        tv.feed(byteArray: [0x1b, 0x63])
                    }

                    // SwiftTerm's own "stay put while the user has scrolled up" logic
                    // (Terminal.scroll(), gated on an internal userScrolling flag) is never
                    // engaged by mouse-wheel scrolling, so every feed() re-pins yDisp to the
                    // live tail. Snapshot/restore it here via the public scroll API so
                    // continuous output doesn't yank the viewport (and any selection in it)
                    // out from under the user. Never write term.buffer.yDisp directly — its
                    // setter skips the refresh/dirty-row/scroller bookkeeping that
                    // scrollUp/scrollDown perform.
                    let priorYDisp = term.buffer.yDisp
                    // scrollPosition is 1 at the live tail (yBase is internal to SwiftTerm).
                    let wasScrolledAway =
                        !term.isCurrentBufferAlternate && tv.canScroll && tv.scrollPosition < 1

                    tv.feed(byteArray: filtered[...])

                    if wasScrolledAway {
                        let delta = term.buffer.yDisp - priorYDisp
                        if delta > 0 {
                            tv.scrollUp(lines: delta)
                        } else if delta < 0 {
                            tv.scrollDown(lines: -delta)
                        }
                    }
                    // No needsDisplay here: SwiftTerm already invalidates just the
                    // dirty rows. Marking the whole view forced a full redraw per chunk.
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
    static func filterTerminalOutput(_ bytes: [UInt8], maxRows: Int) -> [UInt8] {
        // Both rewrites start with ESC; most chunks of plain text contain none.
        guard bytes.contains(0x1b) else { return bytes }

        // "\e[?2026" followed by h (begin) or l (end)
        let syncPrefix: [UInt8] = [0x1b, 0x5b, 0x3f, 0x32, 0x30, 0x32, 0x36]
        let maxUp = max(maxRows - 1, 1)

        var result: [UInt8] = []
        result.reserveCapacity(bytes.count)
        var i = 0
        while i < bytes.count {
            guard bytes[i] == 0x1b else {
                result.append(bytes[i])
                i += 1
                continue
            }
            // Strip DEC 2026 begin/end (8 bytes each)
            if i + 8 <= bytes.count,
                bytes[i..<i + 7].elementsEqual(syncPrefix),
                bytes[i + 7] == 0x68 || bytes[i + 7] == 0x6c
            {
                i += 8
                continue
            }
            // Clamp CSI n A (cursor up): \x1b [ <digits> A
            if i + 2 < bytes.count, bytes[i + 1] == 0x5b {
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
    case streamEnded

    var errorDescription: String? {
        switch self {
        case .attachFailed(let msg): "Attach failed: \(msg)"
        case .unexpectedResponse: "Unexpected response during attach"
        case .agentGone: "Agent no longer exists"
        case .streamEnded: "Daemon ended the output stream"
        }
    }
}
