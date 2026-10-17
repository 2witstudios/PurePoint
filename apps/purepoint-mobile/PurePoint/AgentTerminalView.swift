import SwiftUI
import SwiftTerm
import UIKit
import CoreText

// Automatic responses, paste and hardware keys are dropped. Only deliberate
// full-screen scroll gestures may forward generated wheel events through Control.
private final class ReadOnlyAgentTerminal: TerminalView {
    var agentCols = 80
    var agentRows = 24
    override var canBecomeFirstResponder: Bool { false }
    override func paste(_ sender: Any?) {}

    override func layoutSubviews() {
        super.layoutSubviews()
        if getTerminal().cols != agentCols || getTerminal().rows != agentRows {
            resize(cols: agentCols, rows: agentRows)
        }
    }
}

// Reports viewport size changes (keyboard, rotation) so the newest rows stay visible
// and the phone can claim a matching agent size.
private final class AnchoredScrollView: UIScrollView {
    var onViewportChange: (() -> Void)?
    private var lastSize = CGSize.zero
    override func layoutSubviews() {
        super.layoutSubviews()
        if bounds.size != lastSize { lastSize = bounds.size; onViewportChange?() }
    }
}

struct AgentTerminalView: UIViewRepresentable {
    @ObservedObject var model: AgentControlModel
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIView(context: Context) -> UIScrollView {
        let scroll = AnchoredScrollView()
        scroll.minimumZoomScale = 0.5; scroll.maximumZoomScale = 3
        scroll.backgroundColor = .black; scroll.delegate = context.coordinator
        scroll.alwaysBounceHorizontal = true; scroll.isDirectionalLockEnabled = true
        scroll.contentInsetAdjustmentBehavior = .never
        scroll.accessibilityLabel = "Agent terminal. Tap to hide the keyboard. Pinch to zoom, double-tap to fit width."
        let doubleTap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.toggleFit))
        doubleTap.numberOfTapsRequired = 2
        let tap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.dismissKeyboard))
        tap.require(toFail: doubleTap)
        scroll.addGestureRecognizer(doubleTap); scroll.addGestureRecognizer(tap)
        scroll.onViewportChange = { [weak coordinator = context.coordinator] in coordinator?.viewportChanged() }
        context.coordinator.scroll = scroll
        return scroll
    }
    func updateUIView(_ scroll: UIScrollView, context: Context) {
        // Read revision explicitly so output changes schedule a representable update.
        _ = model.terminalRevision
        context.coordinator.update(model)
    }
    static func dismantleUIView(_ uiView: UIScrollView, coordinator: Coordinator) {
        coordinator.teardown()
        coordinator.terminal?.terminalDelegate = nil
        coordinator.terminal?.removeFromSuperview(); uiView.delegate = nil
    }

    final class Coordinator: NSObject, UIScrollViewDelegate, TerminalViewDelegate, UIGestureRecognizerDelegate {
        weak var scroll: UIScrollView?
        fileprivate var terminal: ReadOnlyAgentTerminal?
        private weak var model: AgentControlModel?
        private var epoch: UUID?
        private var forwardingScroll = false
        private var scrollScreen = 0
        private var scrollEpoch: UUID?
        private let terminalFont = UIFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        private var lastCols = 0
        private var lastRows = 0
        // Follow the newest rows (where agents prompt) until the reader scrolls away.
        private var followBottom = true
        // Keyboard height is temporary: keep claiming the rows that fit without it.
        private var keyboardVisible = false
        private var fullHeight: CGFloat = 0
        private var pendingFit: DispatchWorkItem?
        private var lastFit: String?

        override init() {
            super.init()
            NotificationCenter.default.addObserver(self, selector: #selector(keyboardWillShow), name: UIResponder.keyboardWillShowNotification, object: nil)
            NotificationCenter.default.addObserver(self, selector: #selector(keyboardWillHide), name: UIResponder.keyboardWillHideNotification, object: nil)
        }
        func teardown() {
            pendingFit?.cancel(); pendingFit = nil
            NotificationCenter.default.removeObserver(self)
        }
        @objc private func keyboardWillShow() { keyboardVisible = true }
        @objc private func keyboardWillHide() { keyboardVisible = false }
        @objc func dismissKeyboard() { scroll?.window?.endEditing(true) }

        private var cellSize: CGSize {
            let font = terminalFont as CTFont
            let width = "W".size(withAttributes: [.font: terminalFont]).width
            let height = ceil(CTFontGetAscent(font) + CTFontGetDescent(font) + CTFontGetLeading(font))
            return CGSize(width: width, height: height)
        }

        func update(_ model: AgentControlModel) {
            self.model = model
            guard let scroll else { return }
            if epoch != model.terminalEpoch || terminal == nil {
                terminal?.terminalDelegate = nil; terminal?.removeFromSuperview()
                scroll.setZoomScale(1, animated: false)
                let view = ReadOnlyAgentTerminal(frame: CGRect(x: 0, y: 0, width: 800, height: 480), font: terminalFont)
                view.terminalDelegate = self; view.allowMouseReporting = false
                view.nativeBackgroundColor = .black; view.nativeForegroundColor = .white
                terminal = view; scroll.addSubview(view)
                let pan = UIPanGestureRecognizer(target: self, action: #selector(scrollAgent(_:)))
                pan.maximumNumberOfTouches = 1
                pan.delegate = self
                view.addGestureRecognizer(pan)
                view.panGestureRecognizer.require(toFail: pan)
                scroll.panGestureRecognizer.require(toFail: pan)
                epoch = model.terminalEpoch; lastCols = 0; lastRows = 0
                followBottom = true
                scroll.contentOffset = .zero
                resize(cols: model.terminalBaseCols, rows: model.terminalBaseRows)
                scrollToBottom()
            }
            guard let terminal else { return }
            let updates = model.drainTerminal()
            for update in updates {
                switch update {
                case .output(let text): terminal.feed(text: text)
                case .size(let cols, let rows): resize(cols: cols, rows: rows)
                }
            }
            if !updates.isEmpty { scrollToBottom() }
            scheduleFit()
        }
        func viewportChanged() {
            if !keyboardVisible, let scroll { fullHeight = scroll.bounds.height }
            updateMinimumZoom(); scrollToBottom(); scheduleFit()
        }
        /// The agent size that fills this viewport at 1× zoom, without the keyboard.
        private var fittingSize: (cols: Int, rows: Int)? {
            guard let scroll, scroll.bounds.width > 0, scroll.bounds.height > 0 else { return nil }
            let cell = cellSize
            let height = keyboardVisible && fullHeight > 0 ? fullHeight : scroll.bounds.height
            return (Int((scroll.bounds.width - 2) / cell.width), Int((height - 2) / cell.height))
        }
        // Debounced so rotation and keyboard animations settle before the agent reflows.
        private func scheduleFit() {
            // Forget the last key when a claim cannot be sent, so regaining Control re-claims the same size.
            guard let model, let fitting = fittingSize, model.controlEnabled, let stream = model.streamId else { lastFit = nil; return }
            let key = "\(stream):\(fitting.cols)x\(fitting.rows)"
            guard key != lastFit else { return }
            lastFit = key
            pendingFit?.cancel()
            let screen = model.screenToken
            let work = DispatchWorkItem { [weak self] in
                guard let self, let model = self.model, let fitting = self.fittingSize else { return }
                model.claimSize(cols: fitting.cols, rows: fitting.rows, screen: screen)
            }
            pendingFit = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
        }
        private var fitWidthZoom: CGFloat {
            guard let terminal, let scroll, terminal.bounds.width > 0, scroll.bounds.width > 0 else { return 1 }
            return scroll.bounds.width / terminal.bounds.width
        }
        private func updateMinimumZoom() {
            guard let scroll else { return }
            scroll.minimumZoomScale = min(0.5, fitWidthZoom)
        }
        private func clampedZoom(_ zoom: CGFloat) -> CGFloat {
            guard let scroll else { return zoom }
            return min(max(zoom, scroll.minimumZoomScale), scroll.maximumZoomScale)
        }
        private func scrollToBottom() {
            guard followBottom, let scroll else { return }
            let maxY = max(0, scroll.contentSize.height - scroll.bounds.height)
            if abs(scroll.contentOffset.y - maxY) > 0.5 { scroll.contentOffset = CGPoint(x: scroll.contentOffset.x, y: maxY) }
        }
        private func isAtBottom(_ scroll: UIScrollView) -> Bool {
            scroll.contentOffset.y >= scroll.contentSize.height - scroll.bounds.height - 24
        }
        // For agents the phone cannot size (view only, legacy daemons): fit width ↔ actual size.
        @objc func toggleFit() {
            guard let scroll else { return }
            let fit = clampedZoom(fitWidthZoom)
            scroll.setZoomScale(abs(scroll.zoomScale - fit) < 0.01 ? clampedZoom(1) : fit, animated: true)
            followBottom = true; scrollToBottom()
        }
        func scrollViewWillBeginDragging(_ scrollView: UIScrollView) { followBottom = false }
        func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
            if !decelerate { followBottom = isAtBottom(scrollView) }
        }
        func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) { followBottom = isAtBottom(scrollView) }
        func scrollViewDidEndZooming(_ scrollView: UIScrollView, with view: UIView?, atScale scale: CGFloat) {
            followBottom = followBottom || isAtBottom(scrollView)
            scrollToBottom()
        }
        private func resize(cols: Int, rows: Int) {
            guard let terminal, let scroll else { return }
            if lastCols != cols || lastRows != rows {
                terminal.agentCols = cols; terminal.agentRows = rows
                let cell = cellSize
                let size = CGSize(width: CGFloat(cols) * cell.width + 0.1, height: CGFloat(rows) * cell.height + 0.1)
                // Bounds are physical agent dimensions. Zoom changes only presentation.
                let zoom = scroll.zoomScale
                scroll.setZoomScale(1, animated: false)
                terminal.frame = CGRect(origin: .zero, size: size)
                terminal.resize(cols: cols, rows: rows)
                terminal.setNeedsLayout(); terminal.layoutIfNeeded()
                scroll.contentSize = size
                lastCols = cols; lastRows = rows
                updateMinimumZoom()
                // A size this phone claimed already fits at 1×; keep the reader's zoom otherwise.
                let fits = fittingSize.map { $0.cols == cols } ?? false
                scroll.setZoomScale(fits ? 1 : clampedZoom(zoom), animated: false)
                if fits { scroll.contentOffset = CGPoint(x: 0, y: scroll.contentOffset.y) }
            }
        }
        func viewForZooming(in scrollView: UIScrollView) -> UIView? { terminal }
        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func send(source: TerminalView, data: ArraySlice<UInt8>) {
            guard forwardingScroll, source === terminal, scrollEpoch == epoch else { return }
            model?.sendScroll(data, screen: scrollScreen)
        }

        func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            guard let pan = gestureRecognizer as? UIPanGestureRecognizer,
                  let terminal, let model, model.controlEnabled else { return false }
            let term = terminal.getTerminal()
            let velocity = pan.velocity(in: terminal)
            return term.isCurrentBufferAlternate && term.mouseMode != .off && abs(velocity.y) > abs(velocity.x)
        }

        @objc private func scrollAgent(_ pan: UIPanGestureRecognizer) {
            guard let terminal, let model else { return }
            if pan.state == .began {
                scrollScreen = model.screenToken
                scrollEpoch = epoch
            }
            guard pan.state == .changed, scrollEpoch == epoch,
                  scrollScreen == model.screenToken, model.controlEnabled else { return }
            let term = terminal.getTerminal()
            guard term.isCurrentBufferAlternate, term.mouseMode != .off else { return }
            let delta = pan.translation(in: terminal).y
            let ticks = min(8, Int(abs(delta) / 12))
            guard ticks > 0 else { return }
            // Keep movement beyond this batch's cap for the next update.
            let consumed = CGFloat(ticks) * 12 * (delta > 0 ? 1 : -1)
            pan.setTranslation(CGPoint(x: 0, y: delta - consumed), in: terminal)
            let point = pan.location(in: terminal)
            let x = max(0, min(term.cols - 1, Int(point.x / terminal.bounds.width * CGFloat(term.cols))))
            let y = max(0, min(term.rows - 1, Int((point.y - terminal.contentOffset.y) / terminal.bounds.height * CGFloat(term.rows))))
            let flags = term.encodeButton(button: delta > 0 ? 4 : 5, release: false, shift: false, meta: false, control: false)
            forwardingScroll = true
            defer { forwardingScroll = false }
            for _ in 0..<ticks { term.sendEvent(buttonFlags: flags, x: x, y: y) }
        }
        func scrolled(source: TerminalView, position: Double) {}
        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {}
        func bell(source: TerminalView) {}
        func clipboardCopy(source: TerminalView, content: Data) {}
        func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
}
