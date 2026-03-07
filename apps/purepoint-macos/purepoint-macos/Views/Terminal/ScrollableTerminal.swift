import AppKit
import SwiftTerm

/// Wraps a TerminalView and intercepts scroll-wheel events to forward
/// them as mouse wheel escape sequences when the alternate screen buffer is active.
///
/// Without this, SwiftTerm's scrollWheel silently drops scroll events on the
/// alternate buffer (vim, less, etc.) because canScroll returns false when
/// isCurrentBufferAlternate.
class ScrollableTerminal: NSView, TerminalViewDelegate {
    let terminalView: TerminalView
    var attachSession: DaemonAttachSession?

    private var scrollMonitor: Any?
    private var lastKnownTerminalViewSize: CGSize = .zero
    private var accumulatedDelta: CGFloat = 0
    private var lastScrollDirection: Bool?
    private var lastScrollLocation: NSPoint = .zero
    private var scrollFlushTimer: DispatchSourceTimer?
    private var tornDown = false
    private static let pixelsPerScrollTick: CGFloat = 30

    init(frame: NSRect, terminalView: TerminalView? = nil) {
        self.terminalView = terminalView ?? TerminalView(frame: frame)
        self.terminalView.getTerminal().changeScrollback(10_000)
        super.init(frame: frame)

        self.terminalView.pinToEdges(of: self)

        // Set ourselves as the terminal delegate for input/resize events
        self.terminalView.terminalDelegate = self

        // Disable mouse reporting so click/drag does native text selection.
        self.terminalView.allowMouseReporting = false

        // Apply theme
        applyTheme(forceRefresh: false)

        // Intercept scroll events before SwiftTerm's non-overridable scrollWheel
        scrollMonitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
            guard let self else { return event }
            return self.handleScrollEvent(event)
        }

        // Accept file drops
        registerForDraggedTypes([.fileURL])
    }

    required init?(coder: NSCoder) { fatalError() }

    deinit { tearDown() }

    override func layout() {
        super.layout()
        let currentSize = terminalView.frame.size
        guard currentSize != lastKnownTerminalViewSize,
            currentSize.width > 1,
            currentSize.height > 1
        else { return }
        lastKnownTerminalViewSize = currentSize
        terminalView.setFrameSize(currentSize)
    }

    func tearDown() {
        guard !tornDown else { return }
        tornDown = true
        scrollFlushTimer?.cancel()
        scrollFlushTimer = nil
        if let scrollMonitor {
            NSEvent.removeMonitor(scrollMonitor)
            self.scrollMonitor = nil
        }
        let session = attachSession
        Task { await session?.stop() }
    }

    // MARK: - TerminalViewDelegate

    func send(source: TerminalView, data: ArraySlice<UInt8>) {
        let session = attachSession
        let inputData = Data(data)
        Task { await session?.sendInput(inputData) }
    }

    func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        let session = attachSession
        Task { await session?.sendResize(cols: newCols, rows: newRows) }
    }

    func setTerminalTitle(source: TerminalView, title: String) {}
    func scrolled(source: TerminalView, position: Double) {}
    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
    func clipboardCopy(source: TerminalView, content: Data) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setData(content, forType: .string)
    }
    func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}

    // MARK: - Theme

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyTheme(forceRefresh: true)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        applyTheme(forceRefresh: true)
        if window != nil {
            lastKnownTerminalViewSize = .zero
            // Defer to next run loop cycle so Auto Layout has resolved the new frame.
            // updateFullScreen marks all terminal rows dirty, ensuring drawTerminalContents
            // repaints everything — not just rows that processSizeChange thinks changed.
            DispatchQueue.main.async { [weak self] in
                guard let self, self.window != nil else { return }
                let size = self.bounds.size
                guard size.width > 1, size.height > 1 else { return }
                self.terminalView.getTerminal().updateFullScreen()
                self.lastKnownTerminalViewSize = .zero
                self.terminalView.setFrameSize(size)
                self.lastKnownTerminalViewSize = size
                self.terminalView.needsDisplay = true
            }
        }
    }

    private func applyTheme(forceRefresh: Bool) {
        let appearance = window?.effectiveAppearance ?? NSApp.appearance ?? effectiveAppearance
        TerminalTheme.apply(to: terminalView, appearance: appearance)
        layer?.backgroundColor = TerminalTheme.background.cgColor
        guard forceRefresh else { return }
    }

    // MARK: - Drag & Drop

    private func canAcceptDrop(_ sender: NSDraggingInfo) -> Bool {
        sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard canAcceptDrop(sender) else { return [] }
        layer?.borderWidth = 2
        layer?.borderColor = NSColor.controlAccentColor.cgColor
        return .copy
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        canAcceptDrop(sender) ? .copy : []
    }

    override func draggingExited(_ sender: NSDraggingInfo?) {
        layer?.borderWidth = 0
    }

    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool {
        canAcceptDrop(sender)
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        layer?.borderWidth = 0
        guard
            let urls = sender.draggingPasteboard.readObjects(
                forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL],
            !urls.isEmpty
        else {
            return false
        }
        let escaped = urls.map { shellEscape($0.path) }.joined(separator: " ")
        let session = attachSession
        let inputData = Data(escaped.utf8)
        Task { await session?.sendInput(inputData) }
        return true
    }

    // MARK: - Scroll Interception

    private func handleScrollEvent(_ event: NSEvent) -> NSEvent? {
        guard let targetView = event.window?.contentView?.hitTest(event.locationInWindow),
            targetView === terminalView || targetView.isDescendant(of: terminalView)
        else {
            return event
        }

        let term = terminalView.getTerminal()
        guard term.mouseMode != .off, term.isCurrentBufferAlternate else {
            return event
        }

        let delta = event.scrollingDeltaY
        guard abs(delta) > 0.5 else { return nil }

        let isUp = delta > 0
        if let lastDir = lastScrollDirection, lastDir != isUp, abs(accumulatedDelta) > 0.5 {
            flushScrollDelta()
        }
        lastScrollDirection = isUp

        accumulatedDelta += delta
        lastScrollLocation = event.locationInWindow
        startScrollFlushTimerIfNeeded()

        return nil
    }

    private func startScrollFlushTimerIfNeeded() {
        guard scrollFlushTimer == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now() + .milliseconds(16), repeating: .milliseconds(16))
        timer.setEventHandler { [weak self] in
            self?.flushScrollDelta()
        }
        timer.resume()
        scrollFlushTimer = timer
    }

    private func flushScrollDelta() {
        let delta = accumulatedDelta
        guard abs(delta) > 0.5 else {
            scrollFlushTimer?.cancel()
            scrollFlushTimer = nil
            lastScrollDirection = nil
            return
        }

        accumulatedDelta = 0

        let term = terminalView.getTerminal()
        guard term.mouseMode != .off, term.isCurrentBufferAlternate else {
            scrollFlushTimer?.cancel()
            scrollFlushTimer = nil
            lastScrollDirection = nil
            return
        }

        let isUp = delta > 0
        let button = isUp ? 4 : 5
        let flags = term.encodeButton(button: button, release: false, shift: false, meta: false, control: false)

        let localPoint = terminalView.convert(lastScrollLocation, from: nil)
        let cellWidth = terminalView.bounds.width / CGFloat(term.cols)
        let cellHeight = terminalView.bounds.height / CGFloat(term.rows)
        let x = max(0, min(Int(localPoint.x / cellWidth), term.cols - 1))
        let y = max(0, min(Int((terminalView.bounds.height - localPoint.y) / cellHeight), term.rows - 1))

        let ticks = max(1, Int(abs(delta) / Self.pixelsPerScrollTick))
        for _ in 0..<ticks {
            term.sendEvent(buttonFlags: flags, x: x, y: y)
        }
    }
}
