import SwiftUI
import SwiftTerm
import UIKit
import CoreText

// The terminal has no input path. Even terminal-generated responses, mouse events,
// paste and hardware keys are dropped; deliberate control uses the fixed bridge ops.
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

// Reports viewport size changes (keyboard, rotation) so the newest rows stay visible.
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
        scroll.accessibilityLabel = "Agent terminal. Pinch to zoom, scroll sideways, double-tap to fit width."
        let doubleTap = UITapGestureRecognizer(target: context.coordinator, action: #selector(Coordinator.toggleFit))
        doubleTap.numberOfTapsRequired = 2
        scroll.addGestureRecognizer(doubleTap)
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
        coordinator.terminal?.terminalDelegate = nil
        coordinator.terminal?.removeFromSuperview(); uiView.delegate = nil
    }

    final class Coordinator: NSObject, UIScrollViewDelegate, TerminalViewDelegate {
        weak var scroll: UIScrollView?
        fileprivate var terminal: ReadOnlyAgentTerminal?
        private var epoch: UUID?
        private let terminalFont = UIFont.monospacedSystemFont(ofSize: 14, weight: .regular)
        private var lastCols = 0
        private var lastRows = 0
        // Follow the newest rows (where agents prompt) until the reader scrolls away.
        private var followBottom = true
        private static let zoomKey = "pi.agentTerminal.zoom.v1"
        private var preferredZoom: CGFloat {
            get { let value = UserDefaults.standard.double(forKey: Self.zoomKey); return value > 0 ? CGFloat(value) : 1 }
            set { UserDefaults.standard.set(Double(newValue), forKey: Self.zoomKey) }
        }

        func update(_ model: AgentControlModel) {
            guard let scroll else { return }
            if epoch != model.terminalEpoch || terminal == nil {
                terminal?.terminalDelegate = nil; terminal?.removeFromSuperview()
                scroll.setZoomScale(1, animated: false)
                let view = ReadOnlyAgentTerminal(frame: CGRect(x: 0, y: 0, width: 800, height: 480), font: terminalFont)
                view.terminalDelegate = self; view.allowMouseReporting = false
                view.nativeBackgroundColor = .black; view.nativeForegroundColor = .white
                terminal = view; scroll.addSubview(view)
                epoch = model.terminalEpoch; lastCols = 0; lastRows = 0
                followBottom = true
                scroll.contentOffset = .zero
                resize(cols: model.terminalBaseCols, rows: model.terminalBaseRows)
                scroll.setZoomScale(clampedZoom(preferredZoom), animated: false)
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
        }
        func viewportChanged() { updateMinimumZoom(); scrollToBottom() }
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
        @objc func toggleFit() {
            guard let scroll else { return }
            let fit = clampedZoom(fitWidthZoom)
            let target: CGFloat = abs(scroll.zoomScale - fit) < 0.01 ? clampedZoom(1) : fit
            scroll.setZoomScale(target, animated: true)
            preferredZoom = target
            followBottom = true; scrollToBottom()
        }
        func scrollViewDidEndDragging(_ scrollView: UIScrollView, willDecelerate decelerate: Bool) {
            if !decelerate { followBottom = isAtBottom(scrollView) }
        }
        func scrollViewDidEndDecelerating(_ scrollView: UIScrollView) { followBottom = isAtBottom(scrollView) }
        func scrollViewWillBeginDragging(_ scrollView: UIScrollView) { followBottom = false }
        func scrollViewDidEndZooming(_ scrollView: UIScrollView, with view: UIView?, atScale scale: CGFloat) {
            preferredZoom = scale
            followBottom = isAtBottom(scrollView) || followBottom
            scrollToBottom()
        }
        private func resize(cols: Int, rows: Int) {
            guard let terminal, let scroll else { return }
            if lastCols != cols || lastRows != rows {
                terminal.agentCols = cols; terminal.agentRows = rows
                let font = terminalFont as CTFont
                let cellWidth = "W".size(withAttributes: [.font: terminalFont]).width
                let cellHeight = ceil(CTFontGetAscent(font) + CTFontGetDescent(font) + CTFontGetLeading(font))
                let size = CGSize(
                    width: CGFloat(cols) * cellWidth + 0.1, height: CGFloat(rows) * cellHeight + 0.1)
                // Bounds are physical agent dimensions. Zoom changes only presentation.
                let zoom = scroll.zoomScale
                scroll.setZoomScale(1, animated: false)
                terminal.frame = CGRect(origin: .zero, size: size)
                terminal.resize(cols: cols, rows: rows)
                terminal.setNeedsLayout(); terminal.layoutIfNeeded()
                scroll.contentSize = size
                lastCols = cols; lastRows = rows
                updateMinimumZoom()
                scroll.setZoomScale(clampedZoom(zoom), animated: false)
            }
        }
        func viewForZooming(in scrollView: UIScrollView) -> UIView? { terminal }
        func sizeChanged(source: TerminalView, newCols: Int, newRows: Int) {}
        func setTerminalTitle(source: TerminalView, title: String) {}
        func hostCurrentDirectoryUpdate(source: TerminalView, directory: String?) {}
        func send(source: TerminalView, data: ArraySlice<UInt8>) {}
        func scrolled(source: TerminalView, position: Double) {}
        func requestOpenLink(source: TerminalView, link: String, params: [String: String]) {}
        func bell(source: TerminalView) {}
        func clipboardCopy(source: TerminalView, content: Data) {}
        func iTermContent(source: TerminalView, content: ArraySlice<UInt8>) {}
        func rangeChanged(source: TerminalView, startY: Int, endY: Int) {}
    }
}
