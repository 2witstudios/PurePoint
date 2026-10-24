#if os(iOS)
import SwiftTerm
import UIKit

/// SwiftTerm's iOS view draws the whole buffer and scrolls it as a `UIScrollView`, and snaps the
/// viewport back to the live tail whenever output scrolls the terminal. This keeps the viewport
/// where the user scrolled it instead, which the Mac's `DaemonAttachSession` does through `yDisp`
/// (SwiftTerm on iOS does not draw from `yDisp`).
final class PaneTerminalView: TerminalView {
    /// True while SwiftTerm follows output, so the host can tell output scrolling from a
    /// scroll API call (Page Up/Down) that only moves `yDisp`.
    private(set) var isFollowingOutput = false

    /// Where SwiftTerm's next scroller update leaves the viewport instead of the live tail. It
    /// lasts for the current run loop turn, which is when SwiftTerm applies that update.
    var pendingViewportOffsetY: CGFloat? {
        didSet {
            guard pendingViewportOffsetY != nil, oldValue == nil else { return }
            DispatchQueue.main.async { [weak self] in self?.pendingViewportOffsetY = nil }
        }
    }

    override var contentOffset: CGPoint {
        get { super.contentOffset }
        set {
            guard let pendingY = pendingViewportOffsetY else {
                super.contentOffset = newValue
                return
            }
            super.contentOffset = CGPoint(x: newValue.x, y: min(pendingY, tailOffsetY))
        }
    }

    /// Takes a pasted image (an image and no text on the pasteboard) instead of SwiftTerm's
    /// text-only paste. Set by the host while it can upload files to the agent's machine.
    var pasteImage: ((Data) -> Void)?

    override func paste(_ sender: Any?) {
        let pasteboard = UIPasteboard.general
        if let pasteImage, !pasteboard.hasStrings, pasteboard.hasImages, let png = pasteboard.image?.pngData() {
            pasteImage(png)
            return
        }
        super.paste(sender)
    }

    override func scrolled(source terminal: Terminal, yDisp: Int) {
        let wasScrolledAway =
            !terminal.isCurrentBufferAlternate
            && (isTracking || isDecelerating || contentOffset.y < tailOffsetY - cellSize.height / 2)
        let savedOffset = contentOffset
        isFollowingOutput = true
        super.scrolled(source: terminal, yDisp: yDisp)
        isFollowingOutput = false
        if wasScrolledAway {
            contentOffset = savedOffset
        }
    }
}

extension TerminalView {
    /// The viewport's offset at the live tail, where SwiftTerm puts the last screen row.
    var tailOffsetY: CGFloat {
        max(0, contentSize.height - CGFloat(getTerminal().rows) * cellSize.height)
    }

    /// One character cell, derived from SwiftTerm's own font metrics.
    var cellSize: CGSize {
        let term = getTerminal()
        let frame = getOptimalFrameSize()
        return CGSize(
            width: frame.width / CGFloat(max(term.cols, 1)),
            height: frame.height / CGFloat(max(term.rows, 1)))
    }

    /// Lines in the buffer that can be read by index: scrollback plus the screen.
    var readableLineCount: Int {
        let term = getTerminal()
        let drawn = cellSize.height > 0 ? Int((contentSize.height / cellSize.height).rounded()) : 0
        let screenEnd = term.buffer.yDisp + term.rows
        return drawn > 0 ? min(drawn, screenEnd) : screenEnd
    }
}

// MARK: - Grid geometry

/// A cell in the terminal buffer: `row` counts from the top of the scrollback.
struct TerminalGridPoint: Equatable {
    var col: Int
    var row: Int

    var position: Position { Position(col: col, row: row) }

    static func < (lhs: Self, rhs: Self) -> Bool {
        (lhs.row, lhs.col) < (rhs.row, rhs.col)
    }
}

enum TerminalGeometry {
    /// The grid that fits a view, measured the way SwiftTerm's size processing measures it.
    static func gridSize(for size: CGSize, cell: CGSize) -> (cols: Int, rows: Int) {
        guard cell.width > 0, cell.height > 0 else { return (1, 1) }
        return (max(1, Int(size.width / cell.width)), max(1, Int(size.height / cell.height)))
    }

    /// The cell boundary nearest a point, so a drag selects whole characters it passes over.
    static func boundary(at point: CGPoint, cell: CGSize, cols: Int, rows: Int) -> TerminalGridPoint {
        guard cell.width > 0, cell.height > 0, rows > 0 else { return TerminalGridPoint(col: 0, row: 0) }
        let col = Int((point.x / cell.width).rounded())
        let row = Int(floor(point.y / cell.height))
        return TerminalGridPoint(col: max(0, min(col, cols)), row: max(0, min(row, rows - 1)))
    }
}

// MARK: - Pointer selection

/// A trackpad drag selection, from where the drag started to where the pointer is. SwiftTerm's
/// iOS selection is only reachable through touch gestures, so pointer drags keep their own.
struct TerminalTextSelection: Equatable {
    var anchor: TerminalGridPoint
    var head: TerminalGridPoint

    var isEmpty: Bool { anchor == head }

    var start: TerminalGridPoint { head < anchor ? head : anchor }
    var end: TerminalGridPoint { head < anchor ? anchor : head }

    /// The selected columns of each row, end exclusive.
    func spans(cols: Int) -> [(row: Int, start: Int, end: Int)] {
        guard !isEmpty else { return [] }
        return (start.row...end.row).map { row in
            (row, row == start.row ? start.col : 0, row == end.row ? end.col : cols)
        }
    }
}

/// Paints a pointer selection over the terminal's drawn buffer. It lives inside the scroll view,
/// so it scrolls with the text.
final class TerminalSelectionOverlay: UIView {
    var selection: TerminalTextSelection? { didSet { setNeedsDisplay() } }
    var cell: CGSize = .zero
    var cols = 0
    var color: UIColor = .systemBlue { didSet { setNeedsDisplay() } }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
    }

    required init?(coder: NSCoder) { fatalError() }

    override func draw(_ rect: CGRect) {
        guard let selection, cell.width > 0, cell.height > 0 else { return }
        color.withAlphaComponent(0.45).setFill()
        for span in selection.spans(cols: cols) where span.end > span.start {
            UIRectFill(
                CGRect(
                    x: CGFloat(span.start) * cell.width, y: CGFloat(span.row) * cell.height,
                    width: CGFloat(span.end - span.start) * cell.width, height: cell.height))
        }
    }
}

// MARK: - Scroll wheel

/// Turns scroll deltas into mouse-wheel ticks for full-screen programs, exactly as the Mac
/// terminal batches `scrollingDeltaY`: a direction change flushes what was gathered, and each
/// flush sends at least one tick.
struct TerminalScrollWheel {
    static let pixelsPerScrollTick: CGFloat = 30

    /// Wheel button 4 scrolls up (toward older output), 5 down.
    struct Ticks: Equatable {
        let button: Int
        let count: Int
    }

    private(set) var accumulatedDelta: CGFloat = 0
    private var lastScrollDirection: Bool?

    /// Gathers a delta (positive scrolls up). Returns ticks to send now when it reversed direction.
    mutating func add(_ delta: CGFloat) -> Ticks? {
        guard abs(delta) > 0.5 else { return nil }
        let isUp = delta > 0
        var flushed: Ticks?
        if let lastDir = lastScrollDirection, lastDir != isUp, abs(accumulatedDelta) > 0.5 {
            flushed = flush()
        }
        lastScrollDirection = isUp
        accumulatedDelta += delta
        return flushed
    }

    /// The ticks gathered since the last flush, or nil when there is nothing left to send.
    mutating func flush() -> Ticks? {
        let delta = accumulatedDelta
        guard abs(delta) > 0.5 else {
            reset()
            return nil
        }
        accumulatedDelta = 0
        return Ticks(button: delta > 0 ? 4 : 5, count: max(1, Int(abs(delta) / Self.pixelsPerScrollTick)))
    }

    mutating func reset() {
        accumulatedDelta = 0
        lastScrollDirection = nil
    }
}

// MARK: - Gesture recognizers

/// Reports each touch-down and then fails, so it never takes a gesture from the terminal. The
/// iOS counterpart of the Mac's `leftMouseDown` event monitors.
final class TouchDownRecognizer: UIGestureRecognizer {
    var onTouchDown: ((UITouch, UIEvent) -> Void)?

    override init(target: Any?, action: Selector?) {
        super.init(target: target, action: action)
        cancelsTouchesInView = false
        delaysTouchesBegan = false
        delaysTouchesEnded = false
    }

    convenience init(onTouchDown: @escaping (UITouch, UIEvent) -> Void) {
        self.init(target: nil, action: nil)
        self.onTouchDown = onTouchDown
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        if let touch = touches.first { onTouchDown?(touch, event) }
        state = .failed
    }

    override func canPrevent(_ preventedGestureRecognizer: UIGestureRecognizer) -> Bool { false }
    override func canBePrevented(by preventingGestureRecognizer: UIGestureRecognizer) -> Bool { false }
}

/// Recognizes a tap that starts and ends on the same link. It fails at once when the touch does
/// not land on a link, so the terminal's own tap handling is not delayed.
final class LinkTapRecognizer: UIGestureRecognizer {
    /// The link under a point in the recognizer's view, or nil.
    var linkAt: ((CGPoint, UIKeyModifierFlags) -> URL?)?
    private(set) var link: URL?
    private var startPoint: CGPoint = .zero
    private static let slop: CGFloat = 8

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        guard touches.count == 1, let touch = touches.first, let view,
            let url = linkAt?(touch.location(in: view), event.modifierFlags)
        else {
            state = .failed
            return
        }
        link = url
        startPoint = touch.location(in: view)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let touch = touches.first, let view else { return }
        let point = touch.location(in: view)
        if hypot(point.x - startPoint.x, point.y - startPoint.y) > Self.slop {
            state = .failed
        }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        guard let touch = touches.first, let view, let link,
            linkAt?(touch.location(in: view), event.modifierFlags) == link
        else {
            state = .failed
            return
        }
        state = .recognized
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        state = .cancelled
    }

    override func reset() {
        super.reset()
        link = nil
    }
}

// MARK: - Scripted input (DEBUG)

#if DEBUG
/// A key sequence typed into a terminal at launch (`-PUDebugTerminalInput "<script>"`), for
/// driving the simulator where `simctl` cannot type. Text goes through SwiftTerm's text input
/// (`insertText`, which hardware-keyboard characters also use) and special keys through
/// `send`, where SwiftTerm's `pressesBegan` delivers them.
///
/// Script syntax: plain text, plus `<Enter>`, `<Esc>`, `<Tab>`, `<BS>`, `<Up>`, `<Down>`,
/// `<Left>`, `<Right>`, `<C-x>` (Ctrl) and `<M-x>` (Alt) tokens.
enum TerminalScriptedInput {
    enum Arrow: Equatable { case up, down, left, right }

    enum Step: Equatable {
        case text(String)
        case bytes([UInt8])
        case arrow(Arrow)
    }

    static let launchArgument = "PUDebugTerminalInput"

    static func parse(_ script: String) -> [Step] {
        var steps: [Step] = []
        var text = ""
        var rest = Substring(script)
        func flushText() {
            if !text.isEmpty { steps.append(.text(text)) }
            text = ""
        }
        while let ch = rest.first {
            if ch == "<", let close = rest.firstIndex(of: ">"), let step = step(for: rest[rest.index(after: rest.startIndex)..<close]) {
                flushText()
                steps.append(step)
                rest = rest[rest.index(after: close)...]
            } else {
                text.append(ch)
                rest = rest.dropFirst()
            }
        }
        flushText()
        return steps
    }

    private static func step(for token: Substring) -> Step? {
        switch token {
        case "Enter", "CR": return .text("\n")
        case "Esc": return .bytes([0x1b])
        case "Tab": return .bytes([0x09])
        case "BS": return .bytes([0x7f])
        case "Up": return .arrow(.up)
        case "Down": return .arrow(.down)
        case "Left": return .arrow(.left)
        case "Right": return .arrow(.right)
        default: break
        }
        guard token.count == 3, token.dropFirst().first == "-", let key = token.last else { return nil }
        switch token.first {
        case "C": return controlByte(for: key).map { .bytes([$0]) }
        case "M": return .bytes([0x1b] + Array(String(key).utf8))
        default: return nil
        }
    }

    /// SwiftTerm's Ctrl mapping for a hardware key.
    static func controlByte(for key: Character) -> UInt8? {
        switch key {
        case "a"..."z": key.asciiValue.map { $0 - 0x60 }
        case "A"..."Z": key.asciiValue.map { $0 - 0x40 }
        case "\\": 0x1c
        case "_": 0x1f
        case "]": 0x1d
        case "[": 0x1b
        case "^", "6": 0x1e
        case " ": 0
        default: nil
        }
    }

    /// The bytes SwiftTerm sends for an arrow key in the terminal's current cursor mode.
    static func bytes(for arrow: Arrow, applicationCursor: Bool) -> [UInt8] {
        switch arrow {
        case .up: applicationCursor ? EscapeSequences.moveUpApp : EscapeSequences.moveUpNormal
        case .down: applicationCursor ? EscapeSequences.moveDownApp : EscapeSequences.moveDownNormal
        case .left: applicationCursor ? EscapeSequences.moveLeftApp : EscapeSequences.moveLeftNormal
        case .right: applicationCursor ? EscapeSequences.moveRightApp : EscapeSequences.moveRightNormal
        }
    }

    /// Types `steps` into `terminalView`, one step every 80 ms.
    static func run(_ steps: [Step], in terminalView: TerminalView) {
        Task { @MainActor in
            for step in steps {
                switch step {
                case .text(let text): terminalView.insertText(text)
                case .bytes(let bytes): terminalView.send(bytes)
                case .arrow(let arrow):
                    terminalView.send(bytes(for: arrow, applicationCursor: terminalView.getTerminal().applicationCursor))
                }
                try? await Task.sleep(nanoseconds: 80_000_000)
            }
        }
    }
}
#endif
#endif
