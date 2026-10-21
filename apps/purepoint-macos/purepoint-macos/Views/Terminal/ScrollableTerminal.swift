#if os(macOS)
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
    /// Remote TUIs must not handle browser/clipboard gestures on their host.
    var usesRemoteHost = false
    /// The agent's daemon. Pasted images, and files dropped on a remote agent, are
    /// uploaded to it so the agent reads them from its own machine.
    var attachmentClient: DaemonClient?
    var openLink: (URL) -> Void = { NSWorkspace.shared.open($0) }
    private var clickedLink: URL?
    private var linkDragStarted = false
    private var suppressDraggedLink = false
    private(set) var localGridSize = (cols: 120, rows: 40)
    private var reportedGridSize: (cols: Int, rows: Int)?
    private var lastPhysicalFont: NSFont?

    private var scrollMonitor: Any?
    private var mouseMonitor: Any?
    private var pasteMonitor: Any?
    private var hoverMonitor: Any?
    private var dragAutoscrollTimer: DispatchSourceTimer?
    private var lastDragEvent: NSEvent?
    /// True only between a mouse-down that hit this terminal and the matching mouse-up,
    /// when SwiftTerm (not the app) owns the drag.
    private var nativeDragActive = false
    private var lastKnownTerminalViewSize: CGSize = .zero
    private var accumulatedDelta: CGFloat = 0
    private var lastScrollDirection: Bool?
    private var lastScrollLocation: NSPoint = .zero
    private var scrollFlushTimer: DispatchSourceTimer?
    private var tornDown = false
    private static let pixelsPerScrollTick: CGFloat = 30

    init(frame: NSRect, terminalView: TerminalView? = nil) {
        self.terminalView = terminalView ?? TerminalView(frame: frame)
        self.terminalView.getTerminal().changeHistorySize(10_000)
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

        // Decide per gesture who owns click/drag selection (see handleMouseEvent)
        mouseMonitor = NSEvent.addLocalMonitorForEvents(
            matching: [.leftMouseDown, .leftMouseDragged, .leftMouseUp]
        ) { [weak self] event in
            if event.type == .leftMouseDown, let self,
                self.window === event.window,
                self.bounds.contains(self.convert(event.locationInWindow, from: nil))
            {
                let session = self.attachSession
                Task { await session?.claimSize() }
            }
            if self?.handleMouseEvent(event) == true { return nil }
            return event
        }

        // SwiftTerm reports a buttonless hover in SGR mode as a left-button release
        // (`CSI <32;x;y m`), so any-event TUIs like Claude Code click wherever the
        // pointer passes. Its mouseMoved also ignores allowMouseReporting. The
        // protocol is private, so a correct report can't be sent; drop hovers instead.
        hoverMonitor = NSEvent.addLocalMonitorForEvents(matching: .mouseMoved) { [weak self] event in
            guard let self, self.isHoverReport(event) else { return event }
            return nil
        }

        // SwiftTerm's paste only handles text; an image on the clipboard is uploaded instead.
        pasteMonitor = NSEvent.addLocalMonitorForEvents(matching: .keyDown) { [weak self] event in
            guard let self, self.handlePasteImage(event) else { return event }
            return nil
        }

        // Accept file drops
        registerForDraggedTypes([.fileURL])
    }

    required init?(coder: NSCoder) { fatalError() }

    deinit { tearDown() }

    override func layout() {
        super.layout()
        refreshLocalGeometry(size: terminalView.frame.size)
    }

    /// Measure physical geometry independently of the daemon's rendering grid.
    func refreshLocalGeometry(size: CGSize) {
        guard size.width > 1, size.height > 1 else { return }
        let changed = size != lastKnownTerminalViewSize || terminalView.font != lastPhysicalFont
        guard changed else { return }
        lastKnownTerminalViewSize = size
        lastPhysicalFont = terminalView.font
        terminalView.setFrameSize(size)
        let term = terminalView.getTerminal()
        localGridSize = (term.cols, term.rows)
        if let reportedGridSize { renderGrid(reportedGridSize) }
        let local = localGridSize
        let session = attachSession
        Task { await session?.sendResize(cols: local.cols, rows: local.rows) }
    }

    func tearDown() {
        guard !tornDown else { return }
        tornDown = true
        scrollFlushTimer?.cancel()
        scrollFlushTimer = nil
        nativeDragActive = false
        stopDragAutoscroll()
        if let mouseMonitor {
            NSEvent.removeMonitor(mouseMonitor)
            self.mouseMonitor = nil
        }
        if let hoverMonitor {
            NSEvent.removeMonitor(hoverMonitor)
            self.hoverMonitor = nil
        }
        if let scrollMonitor {
            NSEvent.removeMonitor(scrollMonitor)
            self.scrollMonitor = nil
        }
        if let pasteMonitor {
            NSEvent.removeMonitor(pasteMonitor)
            self.pasteMonitor = nil
        }
        let session = attachSession
        Task { await session?.stop() }
    }

    func applyReportedSize(cols: Int, rows: Int) {
        reportedGridSize = (cols, rows)
        renderGrid((cols, rows))
    }

    private func renderGrid(_ size: (cols: Int, rows: Int)) {
        terminalView.getTerminal().resize(cols: size.cols, rows: size.rows)
        terminalView.getTerminal().updateFullScreen()
        terminalView.needsDisplay = true
    }

    // MARK: - TerminalViewDelegate

    func send(source: TerminalView, data: ArraySlice<UInt8>) {
        let session = attachSession
        let inputData = Data(data)
        Task { await session?.sendInput(inputData) }
    }

    func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        // Ignore reported model dimensions, including asynchronous callbacks.
        // Only a changed physical frame/font can update desired local geometry.
        if source.frame.size != lastKnownTerminalViewSize || source.font != lastPhysicalFont {
            refreshLocalGeometry(size: source.frame.size)
        }
    }

    func setTerminalTitle(source: TerminalView, title: String) {}
    func scrolled(source: TerminalView, position: Double) {}
    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
    func clipboardCopy(source: TerminalView, content: Data) {
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setData(content, forType: .string)
    }
    func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        guard !suppressDraggedLink, let url = URL(string: link), url.scheme != nil else { return }
        openLink(url)
    }

    /// Resolve a link in the displayed cells, using the physical cell size even when
    /// another attached client owns the daemon's grid dimensions.
    func link(at point: NSPoint) -> URL? {
        let term = terminalView.getTerminal()
        let size = terminalView.getOptimalFrameSize().size
        let cellWidth = (size.width - NSScroller.scrollerWidth(for: .regular, scrollerStyle: .legacy)) / CGFloat(term.cols)
        let cellHeight = size.height / CGFloat(term.rows)
        guard cellWidth > 0, cellHeight > 0, terminalView.bounds.contains(point) else { return nil }
        let col = Int(point.x / cellWidth)
        let row = Int((terminalView.bounds.height - point.y) / cellHeight)
        guard let cell = term.getCharData(col: col, row: row) else { return nil }
        if let payload = cell.getPayload() as? String,
            let separator = payload.firstIndex(of: ";"),
            let url = URL(string: String(payload[payload.index(after: separator)...])),
            url.scheme != nil
        {
            return url
        }
        // Plain web URLs are common in TUIs that do not emit OSC 8 hyperlinks.
        var text = ""
        var offset = 0
        for column in 0..<term.cols {
            guard let data = term.getCharData(col: column, row: row), data.width > 0 else { continue }
            if column <= col { offset = text.utf16.count }
            text.append(term.getCharacter(for: data))
        }
        guard let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) else { return nil }
        return detector.matches(in: text, range: NSRange(location: 0, length: text.utf16.count))
            .first { NSLocationInRange(offset, $0.range) && ["http", "https"].contains($0.url?.scheme?.lowercased() ?? "") }?.url
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
                self.refreshLocalGeometry(size: size)
                self.terminalView.needsDisplay = true
            }
        }
    }

    private func applyTheme(forceRefresh: Bool) {
        let appearance = window?.effectiveAppearance ?? NSApp.appearance ?? effectiveAppearance
        TerminalTheme.apply(to: terminalView, appearance: appearance)
        refreshLocalGeometry(size: terminalView.frame.size)
        if let reportedGridSize { renderGrid(reportedGridSize) }
        layer?.backgroundColor = TerminalTheme.background.cgColor
        guard forceRefresh else { return }
    }

    // MARK: - Attachments

    /// Cmd-V with an image (and no text) on the clipboard uploads it as a PNG and
    /// types its path on the agent's machine.
    private func handlePasteImage(_ event: NSEvent) -> Bool {
        guard AttachmentUpload.isCommandOnly(event.modifierFlags),
            event.charactersIgnoringModifiers == "v",
            let window, event.window === window, window.firstResponder === terminalView,
            let client = attachmentClient,
            let png = AttachmentUpload.pastedImage(from: .general)
        else { return false }
        typeUploadedPaths(of: [(png, AttachmentUpload.pastedImageName())], via: client)
        return true
    }

    /// Uploads files in order, then types their remote paths as one shell-escaped
    /// line without submitting, as a local drop does. Beeps if any upload fails
    /// or the line cannot be typed.
    private func typeUploadedPaths(of files: [(data: Data, name: String)], via client: DaemonClient) {
        let session = attachSession
        Task {
            do {
                var paths: [String] = []
                for file in files {
                    paths.append(try await AttachmentUpload.upload(file.data, name: file.name, via: client))
                }
                let line = paths.map(shellEscape).joined(separator: " ")
                // The upload uses its own connections, so it can finish while the
                // attach connection is reconnecting.
                guard await session?.deliverInput(Data(line.utf8)) == true else {
                    print("[Attachment] could not type the uploaded paths")
                    await MainActor.run { NSSound.beep() }
                    return
                }
            } catch {
                print("[Attachment] upload failed: \(error.localizedDescription)")
                await MainActor.run { NSSound.beep() }
            }
        }
    }

    // MARK: - Drag & Drop

    /// Dropping a file types its path. A remote agent's machine cannot see this Mac's
    /// files, so for a remote agent the files are uploaded first.
    private func canAcceptDrop(_ sender: NSDraggingInfo) -> Bool {
        (!usesRemoteHost || attachmentClient != nil)
            && sender.draggingPasteboard.canReadObject(
                forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true])
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
        if usesRemoteHost, let client = attachmentClient {
            // Reading a large file, or one on a slow volume, must not block the main thread.
            Task {
                let read = await Task.detached {
                    Result {
                        try urls.map { (data: try AttachmentUpload.readFile(at: $0), name: $0.lastPathComponent) }
                    }
                }.value
                switch read {
                case .success(let files):
                    typeUploadedPaths(of: files, via: client)
                case .failure(let error):
                    print("[Attachment] cannot upload drop: \(error.localizedDescription)")
                    NSSound.beep()  // a folder, unreadable or oversized file
                }
            }
            return true
        }
        let escaped = urls.map { shellEscape($0.path) }.joined(separator: " ")
        let session = attachSession
        let inputData = Data(escaped.utf8)
        Task { await session?.sendInput(inputData) }
        return true
    }

    // MARK: - Selection Ownership

    /// Decides who owns selection for each click gesture.
    ///
    /// Local fullscreen TUIs own selection across their internal scrollback. Remote
    /// terminals use native selection so copying writes this Mac's clipboard. Option
    /// forces native selection locally and lets a remote TUI receive mouse gestures.
    /// Command-click always stays local for SwiftTerm hyperlinks.
    @discardableResult
    func handleMouseEvent(_ event: NSEvent) -> Bool {
        switch event.type {
        case .leftMouseDown:
            nativeDragActive = false
            clickedLink = nil
            linkDragStarted = false
            suppressDraggedLink = false
            guard let window = event.window,
                let hit = window.contentView?.hitTest(event.locationInWindow),
                hit === terminalView || hit.isDescendant(of: terminalView)
            else { return false }
            if usesRemoteHost && !event.modifierFlags.contains(.option) {
                clickedLink = link(at: terminalView.convert(event.locationInWindow, from: nil))
            }
            let term = terminalView.getTerminal()
            let appOwnsSelection =
                term.isCurrentBufferAlternate && term.mouseMode != .off
                && (usesRemoteHost ? event.modifierFlags.contains(.option) : !event.modifierFlags.contains(.option))
                && !event.modifierFlags.contains(.command)
            terminalView.allowMouseReporting = appOwnsSelection
            nativeDragActive = !appOwnsSelection
        case .leftMouseDragged:
            linkDragStarted = true
            guard nativeDragActive else { return false }
            updateDragAutoscroll(for: event)
        case .leftMouseUp:
            // SwiftTerm must finish selection on mouse-up, but its Command-click
            // callback does not distinguish a completed drag from a click.
            suppressDraggedLink = nativeDragActive && linkDragStarted
            var handledLink = false
            if let clickedLink, event.window === terminalView.window,
                !linkDragStarted,
                clickedLink == link(at: terminalView.convert(event.locationInWindow, from: nil))
            {
                openLink(clickedLink)
                handledLink = true
            }
            clickedLink = nil
            nativeDragActive = false
            stopDragAutoscroll()
            // SwiftTerm also opens OSC links on Command-mouseUp. Consume a
            // opened link so it cannot launch twice. Drags must finish in SwiftTerm.
            return handledLink
        default:
            break
        }
        return false
    }

    /// A pointer move over this terminal that SwiftTerm would report to the app.
    private func isHoverReport(_ event: NSEvent) -> Bool {
        guard event.window === terminalView.window,
            terminalView.getTerminal().mouseMode.sendMotionEvent()
        else { return false }
        return terminalView.bounds.contains(terminalView.convert(event.locationInWindow, from: nil))
    }

    private func updateDragAutoscroll(for event: NSEvent) {
        let point = terminalView.convert(event.locationInWindow, from: nil)
        let outside = point.y < 0 || point.y > terminalView.bounds.height
        guard outside, !terminalView.getTerminal().isCurrentBufferAlternate else {
            stopDragAutoscroll()
            return
        }
        lastDragEvent = event
        guard dragAutoscrollTimer == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now(), repeating: .milliseconds(50))
        timer.setEventHandler { [weak self] in self?.autoscrollTick() }
        timer.resume()
        dragAutoscrollTimer = timer
    }

    private func autoscrollTick() {
        guard let event = lastDragEvent else { return }
        // The view isn't flipped: y < 0 is below the view (scroll toward newer output).
        let point = terminalView.convert(event.locationInWindow, from: nil)
        let overshoot = point.y < 0 ? -point.y : point.y - terminalView.bounds.height
        let rowHeight = terminalView.bounds.height / CGFloat(terminalView.getTerminal().rows)
        let lines = max(1, min(Int(overshoot / rowHeight) + 1, 10))
        if point.y < 0 {
            terminalView.scrollDown(lines: lines)
        } else {
            terminalView.scrollUp(lines: lines)
        }
        // Re-extend the selection to the pointer's row now that the content has moved.
        terminalView.mouseDragged(with: event)
    }

    private func stopDragAutoscroll() {
        dragAutoscrollTimer?.cancel()
        dragAutoscrollTimer = nil
        lastDragEvent = nil
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

        if terminalView.selectionActive {
            // Mid-drag: don't hijack scroll into synthetic PTY clicks, that would repaint
            // the TUI's content under a selection that stays anchored to now-stale rows.
            if NSEvent.pressedMouseButtons & 1 != 0 {
                cancelPendingScrollFlush()
                return event
            }
            // Selection is finished. SwiftTerm drops wheel events on the alternate buffer,
            // so passing the event through would make scrolling impossible until the user
            // clicks elsewhere. Scrolling repaints the TUI under the selection anyway, so
            // drop it and scroll.
            terminalView.selectNone()
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

    private func cancelPendingScrollFlush() {
        scrollFlushTimer?.cancel()
        scrollFlushTimer = nil
        accumulatedDelta = 0
        lastScrollDirection = nil
    }

    private func flushScrollDelta() {
        let delta = accumulatedDelta
        guard abs(delta) > 0.5 else {
            cancelPendingScrollFlush()
            return
        }

        accumulatedDelta = 0

        let term = terminalView.getTerminal()
        guard term.mouseMode != .off, term.isCurrentBufferAlternate, !terminalView.selectionActive else {
            cancelPendingScrollFlush()
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
#endif
