#if os(iOS)
import Foundation
import SwiftTerm
import Testing
import UIKit

@testable import PurePoint

// MARK: - In-memory daemon

/// One direction of an in-memory duplex stream.
private final class AttachPipe: @unchecked Sendable {
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
                let data = chunks.removeFirst()
                lock.unlock()
                cont.resume(returning: data)
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

private final class AttachStream: DaemonByteStream, @unchecked Sendable {
    private let inbound: AttachPipe
    private let outbound: AttachPipe

    init(inbound: AttachPipe, outbound: AttachPipe) {
        self.inbound = inbound
        self.outbound = outbound
    }

    func send(_ data: Data) async throws { outbound.push(data) }
    func receive() async throws -> Data { try await inbound.pull() }
    func close() {
        inbound.finish()
        outbound.finish()
    }
}

/// Collects what an attached terminal sends: its input bytes and claim/resize requests.
private final class AttachRecorder: @unchecked Sendable {
    private let lock = NSLock()
    private var input = Data()
    private var types: [String] = []

    func record(_ line: Data) {
        guard let object = try? JSONSerialization.jsonObject(with: line) as? [String: Any],
            let type = object["type"] as? String
        else { return }
        lock.withLock {
            types.append(type)
            if type == "input", let hex = object["data"] as? String { input.append(Data(hexString: hex)) }
        }
    }

    var inputBytes: [UInt8] { lock.withLock { [UInt8](input) } }
    var requestTypes: [String] { lock.withLock { types } }
}

/// A daemon that confirms the attach and then records every request on the stream.
private struct AttachDaemon: DaemonTransport {
    let recorder: AttachRecorder

    func openStream(cancellable: Bool) async throws -> any DaemonByteStream {
        let toServer = AttachPipe()
        let toClient = AttachPipe()
        let server = AttachStream(inbound: toServer, outbound: toClient)
        let recorder = recorder
        Task {
            let reader = DaemonLineReader(stream: server)
            _ = try await reader.readLine()
            try await server.send(Data(#"{"type":"attach_ready","buffered_bytes":0}"#.utf8) + [0x0A])
            while let line = try? await reader.readLine() { recorder.record(line) }
        }
        return AttachStream(inbound: toClient, outbound: toServer)
    }
}

private func waitUntil(timeout: Duration = .seconds(5), _ condition: () -> Bool) async {
    let deadline = ContinuousClock.now + timeout
    while ContinuousClock.now < deadline, !condition() {
        try? await Task.sleep(for: .milliseconds(20))
    }
}

// MARK: - Tests

@MainActor
struct ScrollableTerminalIOSTests {
    private func makeTerminal(width: CGFloat = 600, height: CGFloat = 200) -> ScrollableTerminal {
        let terminal = ScrollableTerminal(frame: CGRect(x: 0, y: 0, width: width, height: height))
        terminal.layoutIfNeeded()
        return terminal
    }

    // MARK: Keyboard input

    @Test func typedTextAndSpecialKeysReachTheDaemonAsInput() async throws {
        let recorder = AttachRecorder()
        let terminal = makeTerminal()
        let session = DaemonAttachSession(
            agentId: "ag-1", terminalView: terminal.terminalView,
            client: DaemonClient(transport: AttachDaemon(recorder: recorder)))
        terminal.attachSession = session
        let attach = Task { await session.start() }
        defer {
            attach.cancel()
            terminal.tearDown()
        }
        // Input before the stream is up is dropped, as on the Mac; a size claim shows it is up.
        let deadline = ContinuousClock.now + .seconds(5)
        while !recorder.requestTypes.contains("claim_size"), ContinuousClock.now < deadline {
            await session.claimSize()
            try await Task.sleep(for: .milliseconds(20))
        }

        let tv = terminal.terminalView
        tv.insertText("ls")
        tv.insertText("\n")
        // The hardware keyboard path for Esc, Ctrl-C and Up (SwiftTerm's `pressesBegan` → `send`).
        tv.send([0x1b])
        tv.send([0x03])
        tv.send(EscapeSequences.moveUpNormal)

        let expected: [UInt8] = Array("ls".utf8) + [0x0d, 0x1b, 0x03] + EscapeSequences.moveUpNormal
        await waitUntil { recorder.inputBytes == expected }
        #expect(recorder.inputBytes == expected)
    }

    @Test func softwareKeyBarShowsOnlyWithTheSoftwareKeyboard() {
        #expect(ScrollableTerminal.showsAccessoryBar(hardwareKeyboardAttached: false, softwareKeyboardHeight: 0))
        #expect(ScrollableTerminal.showsAccessoryBar(hardwareKeyboardAttached: false, softwareKeyboardHeight: 380))
        // Hardware keyboard: at most the shortcuts bar is on screen.
        #expect(!ScrollableTerminal.showsAccessoryBar(hardwareKeyboardAttached: true, softwareKeyboardHeight: 0))
        #expect(!ScrollableTerminal.showsAccessoryBar(hardwareKeyboardAttached: true, softwareKeyboardHeight: 69))
        // Hardware keyboard, but the software keyboard was brought up anyway.
        #expect(ScrollableTerminal.showsAccessoryBar(hardwareKeyboardAttached: true, softwareKeyboardHeight: 380))
    }

    // MARK: Size

    @Test func gridSizeFitsWholeCells() {
        let grid = TerminalGeometry.gridSize(for: CGSize(width: 805, height: 409), cell: CGSize(width: 8, height: 17))
        #expect(grid.cols == 100)
        #expect(grid.rows == 24)
        let tiny = TerminalGeometry.gridSize(for: CGSize(width: 2, height: 2), cell: CGSize(width: 8, height: 17))
        #expect(tiny.cols == 1)
        #expect(tiny.rows == 1)
    }

    @Test func localGridFollowsThePhysicalViewNotTheReportedGrid() {
        let terminal = makeTerminal(width: 600, height: 200)
        let cell = terminal.terminalView.cellSize
        let physical = TerminalGeometry.gridSize(for: CGSize(width: 600, height: 200), cell: cell)
        terminal.applyReportedSize(cols: 40, rows: 10)
        #expect(terminal.terminalView.getTerminal().cols == 40)
        #expect(terminal.terminalView.getTerminal().rows == 10)

        terminal.frame.size = CGSize(width: 600, height: 300)
        terminal.layoutIfNeeded()
        let taller = TerminalGeometry.gridSize(for: CGSize(width: 600, height: 300), cell: cell)
        #expect(terminal.localGridSize.cols == physical.cols)
        #expect(terminal.localGridSize.rows == taller.rows)
        // The daemon's grid keeps deciding what renders.
        #expect(terminal.terminalView.getTerminal().cols == 40)
        #expect(terminal.terminalView.getTerminal().rows == 10)
    }

    // MARK: Selection ownership

    @Test func remoteTerminalKeepsSelectionUnlessOptionHandsItToTheProgram() {
        func owns(alt: Bool = true, mouse: Bool = true, remote: Bool = true, _ modifiers: UIKeyModifierFlags = [])
            -> Bool
        {
            ScrollableTerminal.appOwnsSelection(
                alternateBuffer: alt, mouseReporting: mouse, remote: remote, modifiers: modifiers)
        }
        #expect(!owns())
        #expect(owns([.alternate]))
        #expect(!owns([.alternate, .command]))
        #expect(!owns(alt: false, [.alternate]))
        #expect(!owns(mouse: false, [.alternate]))
        #expect(owns(remote: false))
        #expect(!owns(remote: false, [.alternate]))
    }

    // MARK: Scrolling

    @Test func wheelSendsAtLeastOneTickPerFlush() {
        var wheel = TerminalScrollWheel()
        #expect(wheel.add(0.4) == nil)
        #expect(wheel.flush() == nil)
        #expect(wheel.add(10) == nil)
        #expect(wheel.flush() == .init(button: 4, count: 1))
        #expect(wheel.add(-95) == nil)
        #expect(wheel.flush() == .init(button: 5, count: 3))
    }

    @Test func wheelFlushesGatheredDeltaWhenDirectionReverses() {
        var wheel = TerminalScrollWheel()
        #expect(wheel.add(40) == nil)
        #expect(wheel.add(-5) == .init(button: 4, count: 1))
        #expect(wheel.accumulatedDelta == -5)
    }

    @Test func outputKeepsTheViewportWhereTheUserScrolledIt() {
        let terminal = makeTerminal()
        let tv = terminal.terminalView
        for i in 0..<200 { tv.feed(text: "line \(i)\r\n") }
        #expect(tv.tailOffsetY > 0)
        #expect(abs(tv.contentOffset.y - tv.tailOffsetY) < 1)

        // At the tail, output follows.
        tv.feed(text: "more\r\n")
        #expect(abs(tv.contentOffset.y - tv.tailOffsetY) < 1)

        // Scrolled up, output does not move the viewport.
        tv.contentOffset.y = 0
        for i in 0..<20 { tv.feed(text: "later \(i)\r\n") }
        #expect(tv.contentOffset.y == 0)
    }

    @Test func pageUpScrollsTheDrawnViewport() async {
        let terminal = makeTerminal()
        let tv = terminal.terminalView
        for i in 0..<200 { tv.feed(text: "line \(i)\r\n") }
        let tail = tv.contentOffset.y
        tv.pageUp()
        let expected = tail - CGFloat(tv.getTerminal().rows) * tv.cellSize.height
        #expect(abs(tv.contentOffset.y - expected) < 1)
    }

    // MARK: Selection and copy

    @Test func pointerSelectionSpansRowsInReadingOrder() {
        let selection = TerminalTextSelection(
            anchor: TerminalGridPoint(col: 3, row: 2), head: TerminalGridPoint(col: 5, row: 0))
        #expect(selection.start == TerminalGridPoint(col: 5, row: 0))
        #expect(selection.end == TerminalGridPoint(col: 3, row: 2))
        let spans = selection.spans(cols: 10).map { [$0.row, $0.start, $0.end] }
        #expect(spans == [[0, 5, 10], [1, 0, 10], [2, 0, 3]])
        #expect(TerminalTextSelection(anchor: .init(col: 1, row: 1), head: .init(col: 1, row: 1)).spans(cols: 10).isEmpty)
    }

    @Test func dragBoundaryRoundsToTheNearestCellEdge() {
        let cell = CGSize(width: 10, height: 20)
        #expect(TerminalGeometry.boundary(at: CGPoint(x: 14, y: 45), cell: cell, cols: 80, rows: 100) == .init(col: 1, row: 2))
        #expect(TerminalGeometry.boundary(at: CGPoint(x: 16, y: 45), cell: cell, cols: 80, rows: 100) == .init(col: 2, row: 2))
        #expect(TerminalGeometry.boundary(at: CGPoint(x: 9_000, y: -5), cell: cell, cols: 80, rows: 100) == .init(col: 80, row: 0))
    }

    @Test func copyWritesThePointerSelectionToThePasteboard() {
        let terminal = makeTerminal()
        terminal.terminalView.feed(text: "hello world\r\nsecond line\r\n")
        terminal.pointerSelection = TerminalTextSelection(
            anchor: TerminalGridPoint(col: 6, row: 0), head: TerminalGridPoint(col: 6, row: 1))
        #expect(terminal.canPerformAction(#selector(UIResponderStandardEditActions.copy(_:)), withSender: nil))
        terminal.copy(nil)
        #expect(UIPasteboard.general.string == "world\nsecond")

        terminal.pointerSelection = nil
        #expect(!terminal.canPerformAction(#selector(UIResponderStandardEditActions.copy(_:)), withSender: nil))
    }

    @Test func osc52ClipboardCopyWritesThePasteboard() {
        let terminal = makeTerminal()
        terminal.clipboardCopy(source: terminal.terminalView, content: Data("from remote".utf8))
        #expect(UIPasteboard.general.string == "from remote")
    }

    @Test func typingClearsThePointerSelection() {
        let terminal = makeTerminal()
        terminal.pointerSelection = TerminalTextSelection(anchor: .init(col: 0, row: 0), head: .init(col: 3, row: 0))
        terminal.terminalView.insertText("x")
        #expect(terminal.pointerSelection == nil)
    }

    // MARK: Links

    @Test func findsPlainWebLinksUnderAPoint() {
        let terminal = makeTerminal()
        let tv = terminal.terminalView
        tv.feed(text: "open https://example.com/docs now\r\n")
        let cell = tv.cellSize
        func point(col: Int, row: Int) -> CGPoint {
            CGPoint(x: (CGFloat(col) + 0.5) * cell.width, y: (CGFloat(row) + 0.5) * cell.height)
        }
        #expect(terminal.link(at: point(col: 10, row: 0)) == URL(string: "https://example.com/docs"))
        #expect(terminal.link(at: point(col: 1, row: 0)) == nil)
        #expect(terminal.link(at: point(col: 10, row: 1)) == nil)
    }

    @Test func findsOSC8HyperlinksUnderAPoint() {
        let terminal = makeTerminal()
        let tv = terminal.terminalView
        tv.feed(text: "\u{1b}]8;;https://purepoint.dev/x\u{1b}\\label\u{1b}]8;;\u{1b}\\ tail\r\n")
        let cell = tv.cellSize
        #expect(terminal.link(at: CGPoint(x: 2.5 * cell.width, y: 0.5 * cell.height)) == URL(string: "https://purepoint.dev/x"))
        #expect(terminal.link(at: CGPoint(x: 7.5 * cell.width, y: 0.5 * cell.height)) == nil)
    }

    // MARK: Scripted input

    @Test func scriptedInputParsesTextAndKeyTokens() {
        typealias Step = TerminalScriptedInput.Step
        let steps = TerminalScriptedInput.parse("echo hi<Enter><Esc><C-c><M-b><Up>a<b")
        #expect(
            steps == [
                Step.text("echo hi"), .text("\n"), .bytes([0x1b]), .bytes([0x03]), .bytes([0x1b, 0x62]), .arrow(.up),
                .text("a<b"),
            ])
        #expect(TerminalScriptedInput.bytes(for: .up, applicationCursor: true) == EscapeSequences.moveUpApp)
    }
}
#endif
