import AppKit
import SwiftTerm

/// Wraps a LocalProcessTerminalView and intercepts scroll-wheel events to forward
/// them as mouse wheel escape sequences when the alternate screen buffer is active.
///
/// Without this, SwiftTerm's scrollWheel silently drops scroll events on the
/// alternate buffer (tmux, vim, less, etc.) because canScroll returns false when
/// isCurrentBufferAlternate.
class ScrollableTerminal: NSView {
    let terminalView: LocalProcessTerminalView

    private var scrollMonitor: Any?
    private var accumulatedDelta: CGFloat = 0
    private var lastScrollDirection: Bool?
    private var lastScrollLocation: NSPoint = .zero
    private var scrollFlushTimer: DispatchSourceTimer?
    private var tornDown = false
    private static let pixelsPerScrollTick: CGFloat = 30

    init(frame: NSRect, terminalView: LocalProcessTerminalView? = nil) {
        self.terminalView = terminalView ?? LocalProcessTerminalView(frame: frame)
        super.init(frame: frame)

        self.terminalView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(self.terminalView)
        NSLayoutConstraint.activate([
            self.terminalView.topAnchor.constraint(equalTo: topAnchor),
            self.terminalView.leadingAnchor.constraint(equalTo: leadingAnchor),
            self.terminalView.trailingAnchor.constraint(equalTo: trailingAnchor),
            self.terminalView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])

        // Disable mouse reporting so click/drag does native text selection.
        // Scroll-wheel forwarding to tmux is handled separately.
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

    func tearDown() {
        guard !tornDown else { return }
        tornDown = true
        scrollFlushTimer?.cancel()
        scrollFlushTimer = nil
        if let scrollMonitor {
            NSEvent.removeMonitor(scrollMonitor)
            self.scrollMonitor = nil
        }
        terminalView.process?.terminate()
    }

    var process: LocalProcess? { terminalView.process }

    func startProcess(executable: String, args: [String], environment: [String]?, execName: String?) {
        terminalView.startProcess(executable: executable, args: args, environment: environment, execName: execName)
    }

    func send(txt: String) {
        terminalView.send(txt: txt)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        applyTheme(forceRefresh: true)
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        applyTheme(forceRefresh: true)
    }

    private func applyTheme(forceRefresh: Bool) {
        let appearance = window?.effectiveAppearance ?? NSApp.appearance ?? effectiveAppearance
        TerminalTheme.apply(to: terminalView, appearance: appearance)
        layer?.backgroundColor = TerminalTheme.background.cgColor
        guard forceRefresh else { return }
    }

    // MARK: - Drag & Drop

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) else {
            return []
        }
        layer?.borderWidth = 2
        layer?.borderColor = NSColor.controlAccentColor.cgColor
        return .copy
    }

    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation {
        sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) ? .copy : []
    }

    override func draggingExited(_ sender: NSDraggingInfo?) {
        layer?.borderWidth = 0
    }

    override func prepareForDragOperation(_ sender: NSDraggingInfo) -> Bool {
        sender.draggingPasteboard.canReadObject(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
    }

    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        layer?.borderWidth = 0
        guard let urls = sender.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL],
              !urls.isEmpty else {
            return false
        }
        let escaped = urls.map { shellEscape($0.path) }.joined(separator: " ")
        terminalView.send(txt: escaped)
        return true
    }

    // MARK: - Scroll Interception

    private func handleScrollEvent(_ event: NSEvent) -> NSEvent? {
        guard let targetView = event.window?.contentView?.hitTest(event.locationInWindow),
              targetView === terminalView || targetView.isDescendant(of: terminalView) else {
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
