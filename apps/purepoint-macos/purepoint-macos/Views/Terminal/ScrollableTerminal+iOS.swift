#if os(iOS)
import GameController
import SwiftTerm
import UIKit

/// iOS twin of the Mac terminal host: SwiftTerm's iOS `TerminalView`, fed by
/// `DaemonAttachSession`, with the Mac's input behaviour moved onto UIKit gestures.
///
/// SwiftTerm drops wheel scrolling on the alternate buffer (vim, less, etc.), so trackpad and
/// touch scrolling there is forwarded as mouse wheel escape sequences, as the Mac does.
class ScrollableTerminal: UIView, TerminalViewDelegate, UIGestureRecognizerDelegate, UIScrollViewDelegate {
    let terminalView: TerminalView
    var attachSession: DaemonAttachSession?
    /// Remote TUIs must not handle browser/clipboard gestures on their host.
    var usesRemoteHost = false
    var openLink: (URL) -> Void = { UIApplication.shared.open($0) }
    /// Dropping a file types its path on the Mac. A remote agent's machine cannot see this
    /// device's files, and every iPad agent is remote, so the iPad terminal takes no drops.
    var acceptsFileDrops = true
    private(set) var localGridSize = (cols: 120, rows: 40)
    private var reportedGridSize: (cols: Int, rows: Int)?
    private var lastPhysicalFont: UIFont?

    private var wheelRecognizer: UIPanGestureRecognizer?
    private var pointerDragRecognizer: UIPanGestureRecognizer?
    /// Whether the wheel gesture in progress comes from a trackpad or mouse wheel, not a finger.
    private var wheelIsScrollEvent = false
    private var scrollWheel = TerminalScrollWheel()
    private var lastScrollLocation: CGPoint = .zero
    private var scrollFlushTimer: DispatchSourceTimer?
    private var dragAutoscrollTimer: DispatchSourceTimer?
    private weak var autoscrollGesture: UIGestureRecognizer?
    private let selectionOverlay = TerminalSelectionOverlay()
    /// SwiftTerm's own key bar, shown above the software keyboard only.
    private var softwareKeyboardAccessory: UIView?
    private var keyboardObservers: [NSObjectProtocol] = []
    private var lastKnownTerminalViewSize: CGSize = .zero
    private var tornDown = false

    /// A trackpad drag selection; touch selection stays SwiftTerm's own.
    var pointerSelection: TerminalTextSelection? {
        didSet { updateSelectionOverlay() }
    }

    init(frame: CGRect, terminalView: TerminalView? = nil) {
        self.terminalView = terminalView ?? PaneTerminalView(frame: frame)
        self.terminalView.getTerminal().changeHistorySize(10_000)
        super.init(frame: frame)

        // The terminal fills its pane edge to edge; SwiftUI's safe area must not inset its rows.
        self.terminalView.contentInsetAdjustmentBehavior = .never
        self.terminalView.pinToEdges(of: self)

        // Set ourselves as the terminal delegate for input/resize events
        self.terminalView.terminalDelegate = self
        self.terminalView.delegate = self

        // Disable mouse reporting so taps and drags do native text selection.
        self.terminalView.allowMouseReporting = false

        // Apply theme
        applyTheme()
        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _) in
            self.applyTheme()
        }

        installGestures()

        softwareKeyboardAccessory = self.terminalView.inputAccessoryView
        observeHardwareKeyboard()
    }

    required init?(coder: NSCoder) { fatalError() }

    deinit { tearDown() }

    override func layoutSubviews() {
        super.layoutSubviews()
        refreshLocalGeometry(size: terminalView.frame.size)
        updateSelectionOverlay()
    }

    /// Measure physical geometry independently of the daemon's rendering grid.
    func refreshLocalGeometry(size: CGSize) {
        guard size.width > 1, size.height > 1 else { return }
        let changed = size != lastKnownTerminalViewSize || terminalView.font != lastPhysicalFont
        guard changed else { return }
        lastKnownTerminalViewSize = size
        lastPhysicalFont = terminalView.font
        // SwiftTerm re-fits its grid when it lays out at a new size, as `setFrameSize` does on the Mac.
        terminalView.layoutIfNeeded()
        localGridSize = TerminalGeometry.gridSize(for: size, cell: terminalView.cellSize)
        if let reportedGridSize { renderGrid(reportedGridSize) }
        let local = localGridSize
        let session = attachSession
        Task { await session?.sendResize(cols: local.cols, rows: local.rows) }
    }

    func tearDown() {
        guard !tornDown else { return }
        tornDown = true
        cancelPendingScrollFlush()
        stopDragAutoscroll()
        for observer in keyboardObservers {
            NotificationCenter.default.removeObserver(observer)
        }
        keyboardObservers = []
        let session = attachSession
        Task { await session?.stop() }
    }

    func applyReportedSize(cols: Int, rows: Int) {
        reportedGridSize = (cols, rows)
        renderGrid((cols, rows))
    }

    private func renderGrid(_ size: (cols: Int, rows: Int)) {
        let term = terminalView.getTerminal()
        let resized = term.cols != size.cols || term.rows != size.rows
        term.resize(cols: size.cols, rows: size.rows)
        term.updateFullScreen()
        // Refits the scroll view's content to the new grid.
        if resized { terminalView.sizeChanged(source: term) }
        terminalView.setNeedsDisplay()
    }

    // MARK: - TerminalViewDelegate

    func send(source: TerminalView, data: ArraySlice<UInt8>) {
        // Typing replaces a selection, as on the Mac.
        pointerSelection = nil
        let session = attachSession
        let inputData = Data(data)
        Task { await session?.sendInput(inputData) }
    }

    func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {
        // Ignore reported model dimensions, including asynchronous callbacks.
        // Only a changed physical frame/font can update desired local geometry.
        if source.frame.size != lastKnownTerminalViewSize || source.font != lastPhysicalFont {
            refreshLocalGeometry(size: source.frame.size)
        } else if let reportedGridSize,
            source.getTerminal().cols != reportedGridSize.cols || source.getTerminal().rows != reportedGridSize.rows
        {
            // SwiftTerm re-fit its grid to the view on layout; the daemon's grid still decides what renders.
            renderGrid(reportedGridSize)
        }
    }

    func setTerminalTitle(source: TerminalView, title: String) {}

    func scrolled(source: TerminalView, position: Double) {
        // Page Up/Down and the session's viewport restore move `yDisp`, which SwiftTerm's iOS
        // view does not draw from: show the rows it scrolled to.
        guard let paneView = source as? PaneTerminalView, !paneView.isFollowingOutput else { return }
        paneView.pendingViewportOffsetY = CGFloat(source.getTerminal().buffer.yDisp) * source.cellSize.height
    }

    func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
    func clipboardCopy(source: TerminalView, content: Data) {
        UIPasteboard.general.string = String(decoding: content, as: UTF8.self)
    }
    func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {
        guard let url = URL(string: link), url.scheme != nil else { return }
        openLink(url)
    }

    /// Resolve a link in the displayed cells, using the physical cell size even when
    /// another attached client owns the daemon's grid dimensions.
    func link(at point: CGPoint) -> URL? {
        let term = terminalView.getTerminal()
        let cell = terminalView.cellSize
        guard cell.width > 0, cell.height > 0, point.x >= 0, point.y >= 0,
            terminalView.bounds.contains(point)
        else { return nil }
        let col = Int(point.x / cell.width)
        let row = Int(point.y / cell.height)
        guard col < term.cols, row < terminalView.readableLineCount else { return nil }
        let buffer = term.buffer
        let cellData = buffer.getChar(atBufferRelative: Position(col: col, row: row))
        if let payload = cellData.getPayload() as? String,
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
            let data = buffer.getChar(atBufferRelative: Position(col: column, row: row))
            guard data.width > 0 else { continue }
            if column <= col { offset = text.utf16.count }
            text.append(term.getCharacter(for: data))
        }
        guard let detector = try? NSDataDetector(types: NSTextCheckingResult.CheckingType.link.rawValue) else { return nil }
        return detector.matches(in: text, range: NSRange(location: 0, length: text.utf16.count))
            .first { NSLocationInRange(offset, $0.range) && ["http", "https"].contains($0.url?.scheme?.lowercased() ?? "") }?.url
    }

    func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}

    // MARK: - Theme

    override func didMoveToWindow() {
        super.didMoveToWindow()
        applyTheme()
        if window != nil {
            lastKnownTerminalViewSize = .zero
            // Defer to next run loop cycle so Auto Layout has resolved the new frame.
            // updateFullScreen marks all terminal rows dirty, ensuring the terminal
            // repaints everything — not just rows that its size processing thinks changed.
            DispatchQueue.main.async { [weak self] in
                guard let self, self.window != nil else { return }
                let size = self.bounds.size
                guard size.width > 1, size.height > 1 else { return }
                self.terminalView.getTerminal().updateFullScreen()
                self.refreshLocalGeometry(size: size)
                self.terminalView.setNeedsDisplay()
            }
        }
    }

    private func applyTheme() {
        TerminalTheme.apply(to: terminalView, traits: traitCollection)
        // SwiftTerm's Mac defaults, which the Mac terminal keeps.
        terminalView.caretColor = Self.selectionColor
        terminalView.selectedTextBackgroundColor = Self.selectionColor
        selectionOverlay.color = Self.selectionColor.resolvedColor(with: traitCollection)
        refreshLocalGeometry(size: terminalView.frame.size)
        if let reportedGridSize { renderGrid(reportedGridSize) }
        backgroundColor = TerminalTheme.background
    }

    /// AppKit's `selectedTextBackgroundColor` / `selectedControlColor` for the default accent.
    static let selectionColor = UIColor { traits in
        traits.userInterfaceStyle == .dark
            ? UIColor(red: 0.247, green: 0.388, blue: 0.545, alpha: 1)
            : UIColor(red: 0.702, green: 0.843, blue: 1, alpha: 1)
    }

    // MARK: - Keyboard

    /// The shortest on-screen keyboard that counts as the software keyboard. With a hardware
    /// keyboard attached iPadOS shows at most its shortcuts bar, which is far shorter.
    static let softwareKeyboardMinHeight: CGFloat = 150

    /// SwiftTerm's Esc/Ctrl/arrow bar stands in for keys the software keyboard lacks; a
    /// hardware keyboard has them. The software keyboard can still be up with one attached
    /// (shown from the keyboard menu, or a simulator), and then the bar shows with it.
    static func showsAccessoryBar(hardwareKeyboardAttached: Bool, softwareKeyboardHeight: CGFloat) -> Bool {
        !hardwareKeyboardAttached || softwareKeyboardHeight >= softwareKeyboardMinHeight
    }

    private func observeHardwareKeyboard() {
        for name in [Notification.Name.GCKeyboardDidConnect, .GCKeyboardDidDisconnect] {
            keyboardObservers.append(
                NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
                    MainActor.assumeIsolated { self?.updateKeyboardAccessory() }
                })
        }
        for name in [UIResponder.keyboardWillChangeFrameNotification, UIResponder.keyboardWillHideNotification] {
            keyboardObservers.append(
                NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { [weak self] note in
                    let frame = name == UIResponder.keyboardWillHideNotification
                        ? CGRect.zero
                        : (note.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect) ?? .zero
                    MainActor.assumeIsolated { self?.keyboardFrameWillChange(to: frame) }
                })
        }
        updateKeyboardAccessory()
    }

    /// Height of the on-screen keyboard, not counting our accessory bar.
    private var softwareKeyboardHeight: CGFloat = 0

    private func keyboardFrameWillChange(to frame: CGRect) {
        let screen = window?.windowScene?.screen.bounds ?? frame
        var height = frame.intersection(screen).height
        if let accessory = terminalView.inputAccessoryView, terminalView.isFirstResponder {
            height -= accessory.bounds.height
        }
        softwareKeyboardHeight = max(0, height)
        // Not from inside the keyboard's own frame change.
        DispatchQueue.main.async { [weak self] in self?.updateKeyboardAccessory() }
    }

    private func updateKeyboardAccessory() {
        let accessory =
            Self.showsAccessoryBar(
                hardwareKeyboardAttached: GCKeyboard.coalesced != nil,
                softwareKeyboardHeight: softwareKeyboardHeight)
            ? softwareKeyboardAccessory : nil
        guard terminalView.inputAccessoryView !== accessory else { return }
        terminalView.inputAccessoryView = accessory
        if terminalView.isFirstResponder { terminalView.reloadInputViews() }
    }

    // MARK: - Copy

    /// SwiftTerm copies its own (touch) selection while it is first responder; a pointer
    /// selection is copied here, the next responder.
    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        if action == #selector(copy(_:)) { return selectedText() != nil }
        return super.canPerformAction(action, withSender: sender)
    }

    override func copy(_ sender: Any?) {
        guard let text = selectedText() else { return }
        UIPasteboard.general.string = text
    }

    /// The pointer selection's text, or nil without one.
    func selectedText() -> String? {
        guard let selection = pointerSelection, !selection.isEmpty else { return nil }
        return terminalView.getTerminal().getText(start: selection.start.position, end: selection.end.position)
    }

    // MARK: - Gestures

    private func installGestures() {
        let touchDown = TouchDownRecognizer { [weak self] touch, event in
            self?.handleTouchDown(touch, event: event)
        }
        terminalView.addGestureRecognizer(touchDown)

        let linkTap = LinkTapRecognizer(target: self, action: #selector(handleLinkTap(_:)))
        linkTap.linkAt = { [weak self] point, modifiers in
            guard let self, self.usesRemoteHost, !modifiers.contains(.alternate) else { return nil }
            return self.link(at: point)
        }
        terminalView.addGestureRecognizer(linkTap)
        // A tap on a link opens it instead of reaching SwiftTerm.
        for case let tap as UITapGestureRecognizer in terminalView.gestureRecognizers ?? []
        where tap.numberOfTapsRequired == 1 {
            tap.require(toFail: linkTap)
        }

        let wheel = UIPanGestureRecognizer(target: self, action: #selector(handleWheel(_:)))
        wheel.allowedScrollTypesMask = .all
        wheel.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.direct.rawValue)]
        wheel.delegate = self
        terminalView.addGestureRecognizer(wheel)
        wheelRecognizer = wheel

        let pointerDrag = UIPanGestureRecognizer(target: self, action: #selector(handlePointerDrag(_:)))
        pointerDrag.allowedTouchTypes = [NSNumber(value: UITouch.TouchType.indirectPointer.rawValue)]
        pointerDrag.delegate = self
        terminalView.addGestureRecognizer(pointerDrag)
        pointerDragRecognizer = pointerDrag
        // A pointer drag selects, as a mouse drag does on the Mac; trackpad scrolling still
        // scrolls, since it arrives as scroll events rather than touches.
        terminalView.panGestureRecognizer.allowedTouchTypes = [
            NSNumber(value: UITouch.TouchType.direct.rawValue),
            NSNumber(value: UITouch.TouchType.pencil.rawValue),
        ]

        selectionOverlay.frame = .zero
        terminalView.addSubview(selectionOverlay)
    }

    /// Also the delegate check for the terminal's wheel and pointer-drag recognizers.
    override func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
        if gestureRecognizer === wheelRecognizer { return wheelShouldBegin() }
        // Option hands a remote full-screen program the pointer (see `handleTouchDown`).
        if gestureRecognizer === pointerDragRecognizer { return !terminalView.allowMouseReporting }
        return super.gestureRecognizerShouldBegin(gestureRecognizer)
    }

    func gestureRecognizer(_ gestureRecognizer: UIGestureRecognizer, shouldReceive event: UIEvent) -> Bool {
        if gestureRecognizer === wheelRecognizer, event.type == .scroll || event.type == .touches {
            wheelIsScrollEvent = event.type == .scroll
        }
        return true
    }

    /// SwiftTerm's own pans do nothing without mouse reporting, so the wheel runs alongside them.
    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        gestureRecognizer === wheelRecognizer && otherGestureRecognizer !== terminalView.panGestureRecognizer
    }

    /// Wheel events on the alternate buffer go to the program, not to native scrolling.
    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldBeRequiredToFailBy otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        gestureRecognizer === wheelRecognizer && otherGestureRecognizer === terminalView.panGestureRecognizer
    }

    /// Decides who owns selection for each touch or click, as the Mac does per mouse-down.
    ///
    /// Local fullscreen TUIs own selection across their internal scrollback. Remote
    /// terminals use native selection so copying writes this iPad's pasteboard. Option
    /// forces native selection locally and lets a remote TUI receive pointer gestures.
    /// Command never hands gestures to the program.
    static func appOwnsSelection(
        alternateBuffer: Bool, mouseReporting: Bool, remote: Bool, modifiers: UIKeyModifierFlags
    ) -> Bool {
        alternateBuffer && mouseReporting
            && (remote ? modifiers.contains(.alternate) : !modifiers.contains(.alternate))
            && !modifiers.contains(.command)
    }

    private func handleTouchDown(_ touch: UITouch, event: UIEvent) {
        let session = attachSession
        Task { await session?.claimSize() }

        let term = terminalView.getTerminal()
        let modifiers = event.modifierFlags
        terminalView.allowMouseReporting = Self.appOwnsSelection(
            alternateBuffer: term.isCurrentBufferAlternate, mouseReporting: term.mouseMode != .off,
            remote: usesRemoteHost, modifiers: modifiers)

        // A click clears a pointer selection; Shift-click extends it.
        if touch.type == .indirectPointer, modifiers.contains(.shift), var selection = pointerSelection {
            selection.head = boundary(at: touch.location(in: terminalView))
            pointerSelection = selection
        } else {
            pointerSelection = nil
        }
    }

    @objc private func handleLinkTap(_ recognizer: LinkTapRecognizer) {
        guard recognizer.state == .ended, let link = recognizer.link else { return }
        openLink(link)
    }

    // MARK: - Pointer Selection

    private func boundary(at point: CGPoint) -> TerminalGridPoint {
        TerminalGeometry.boundary(
            at: point, cell: terminalView.cellSize, cols: terminalView.getTerminal().cols,
            rows: terminalView.readableLineCount)
    }

    @objc private func handlePointerDrag(_ recognizer: UIPanGestureRecognizer) {
        let point = recognizer.location(in: terminalView)
        switch recognizer.state {
        case .began:
            terminalView.selectNone()
            if pointerSelection == nil {
                let translation = recognizer.translation(in: terminalView)
                let start = boundary(at: CGPoint(x: point.x - translation.x, y: point.y - translation.y))
                pointerSelection = TerminalTextSelection(anchor: start, head: start)
            }
            extendPointerSelection(to: point)
        case .changed:
            extendPointerSelection(to: point)
            updateDragAutoscroll(for: recognizer)
        case .ended, .cancelled, .failed:
            stopDragAutoscroll()
            if pointerSelection?.isEmpty == true { pointerSelection = nil }
        default:
            break
        }
    }

    private func extendPointerSelection(to point: CGPoint) {
        guard var selection = pointerSelection else { return }
        selection.head = boundary(at: point)
        pointerSelection = selection
    }

    private func updateSelectionOverlay() {
        let cell = terminalView.cellSize
        selectionOverlay.cell = cell
        selectionOverlay.cols = terminalView.getTerminal().cols
        selectionOverlay.frame = CGRect(
            x: 0, y: 0,
            width: max(terminalView.bounds.width, terminalView.contentSize.width),
            height: max(terminalView.bounds.height, terminalView.contentSize.height))
        selectionOverlay.selection = pointerSelection
        terminalView.bringSubviewToFront(selectionOverlay)
    }

    private func updateDragAutoscroll(for recognizer: UIGestureRecognizer) {
        let visibleY = recognizer.location(in: terminalView).y - terminalView.contentOffset.y
        let outside = visibleY < 0 || visibleY > terminalView.bounds.height
        guard outside, !terminalView.getTerminal().isCurrentBufferAlternate else {
            stopDragAutoscroll()
            return
        }
        autoscrollGesture = recognizer
        guard dragAutoscrollTimer == nil else { return }
        let timer = DispatchSource.makeTimerSource(queue: .main)
        timer.schedule(deadline: .now(), repeating: .milliseconds(50))
        timer.setEventHandler { [weak self] in self?.autoscrollTick() }
        timer.resume()
        dragAutoscrollTimer = timer
    }

    private func autoscrollTick() {
        guard let recognizer = autoscrollGesture else { return }
        let tv = terminalView
        let visibleY = recognizer.location(in: tv).y - tv.contentOffset.y
        let overshoot = visibleY < 0 ? -visibleY : visibleY - tv.bounds.height
        let rowHeight = tv.cellSize.height
        guard rowHeight > 0 else { return }
        let lines = max(1, min(Int(overshoot / rowHeight) + 1, 10))
        // Above the view scrolls toward older output, below toward newer.
        let delta = CGFloat(visibleY < 0 ? -lines : lines) * rowHeight
        tv.contentOffset.y = max(0, min(tv.contentOffset.y + delta, tv.tailOffsetY))
        // Re-extend the selection to the pointer's row now that the content has moved.
        extendPointerSelection(to: recognizer.location(in: tv))
    }

    private func stopDragAutoscroll() {
        guard dragAutoscrollTimer != nil else { return }
        dragAutoscrollTimer?.cancel()
        dragAutoscrollTimer = nil
        autoscrollGesture = nil
        syncDisplayRowToViewport()
    }

    // MARK: - Native Scrolling

    func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
        if !decelerate { syncDisplayRowToViewport() }
    }

    func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) {
        syncDisplayRowToViewport()
    }

    func scrollViewDidEndScrollingAnimation(_ scrollView: UIScrollView) {
        syncDisplayRowToViewport()
    }

    /// Moves `yDisp` to the row at the top of the viewport after the user scrolls, so the
    /// session's "stay put while scrolled up" logic and Page Up/Down start from what is shown.
    private func syncDisplayRowToViewport() {
        let tv = terminalView
        let rowHeight = tv.cellSize.height
        let term = tv.getTerminal()
        guard rowHeight > 0, !term.isCurrentBufferAlternate else { return }
        let delta = Int((tv.contentOffset.y / rowHeight).rounded()) - term.buffer.yDisp
        if delta > 0 {
            tv.scrollDown(lines: delta)
        } else if delta < 0 {
            tv.scrollUp(lines: -delta)
        }
    }

    // MARK: - Scroll Interception

    private func wheelShouldBegin() -> Bool {
        let term = terminalView.getTerminal()
        guard term.mouseMode != .off, term.isCurrentBufferAlternate else { return false }
        // Mid-drag: don't hijack a touch drag into synthetic PTY clicks, that would repaint
        // the TUI's content under a selection that stays anchored to now-stale rows. A
        // program that owns the touch gets it as mouse drags instead.
        if !wheelIsScrollEvent {
            return !terminalView.selectionActive && !terminalView.allowMouseReporting
        }
        if terminalView.selectionActive {
            // Selection is finished. SwiftTerm drops wheel events on the alternate buffer,
            // so scrolling would be impossible until the user taps elsewhere. Scrolling
            // repaints the TUI under the selection anyway, so drop it and scroll.
            terminalView.selectNone()
        }
        return true
    }

    @objc private func handleWheel(_ recognizer: UIPanGestureRecognizer) {
        switch recognizer.state {
        case .began, .changed:
            let delta = recognizer.translation(in: terminalView).y
            recognizer.setTranslation(.zero, in: terminalView)
            lastScrollLocation = recognizer.location(in: terminalView)
            if let ticks = scrollWheel.add(delta) { sendWheel(ticks) }
            startScrollFlushTimerIfNeeded()
        case .ended, .cancelled, .failed:
            flushScrollDelta()
            cancelPendingScrollFlush()
        default:
            break
        }
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
        scrollWheel.reset()
    }

    private func flushScrollDelta() {
        guard let ticks = scrollWheel.flush() else {
            cancelPendingScrollFlush()
            return
        }
        sendWheel(ticks)
    }

    private func sendWheel(_ ticks: TerminalScrollWheel.Ticks) {
        let term = terminalView.getTerminal()
        guard term.mouseMode != .off, term.isCurrentBufferAlternate, !terminalView.selectionActive else {
            cancelPendingScrollFlush()
            return
        }

        let flags = term.encodeButton(button: ticks.button, release: false, shift: false, meta: false, control: false)

        let cell = terminalView.cellSize
        guard cell.width > 0, cell.height > 0 else { return }
        let visibleY = lastScrollLocation.y - terminalView.contentOffset.y
        let x = max(0, min(Int(lastScrollLocation.x / cell.width), term.cols - 1))
        let y = max(0, min(Int(visibleY / cell.height), term.rows - 1))

        for _ in 0..<ticks.count {
            term.sendEvent(buttonFlags: flags, x: x, y: y)
        }
    }

    // MARK: - Scripted input

    #if DEBUG
    /// Types the `-PUDebugTerminalInput` script into the first terminal that shows output.
    func runScriptedInputIfRequested() {
        guard !Self.didRunScriptedInput,
            let script = UserDefaults.standard.string(forKey: TerminalScriptedInput.launchArgument)
        else { return }
        Self.didRunScriptedInput = true
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { [weak self] in
            guard let self else { return }
            _ = self.terminalView.becomeFirstResponder()
            TerminalScriptedInput.run(TerminalScriptedInput.parse(script), in: self.terminalView)
        }
    }

    private static var didRunScriptedInput = false
    #endif
}
#endif
