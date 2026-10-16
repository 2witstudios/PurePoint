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

struct AgentTerminalView: UIViewRepresentable {
    @ObservedObject var model: AgentControlModel
    func makeCoordinator() -> Coordinator { Coordinator() }
    func makeUIView(context: Context) -> UIScrollView {
        let scroll = UIScrollView()
        scroll.minimumZoomScale = 0.5; scroll.maximumZoomScale = 3
        scroll.backgroundColor = .black; scroll.delegate = context.coordinator
        scroll.alwaysBounceHorizontal = true; scroll.isDirectionalLockEnabled = true
        scroll.accessibilityLabel = "Agent terminal. Pinch to zoom and scroll sideways."
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
                scroll.contentOffset = .zero
                resize(cols: model.terminalBaseCols, rows: model.terminalBaseRows)
            }
            guard let terminal else { return }
            for update in model.drainTerminal() {
                switch update {
                case .output(let text): terminal.feed(text: text)
                case .size(let cols, let rows): resize(cols: cols, rows: rows)
                }
            }
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
                scroll.contentSize = size; scroll.setZoomScale(zoom, animated: false)
                lastCols = cols; lastRows = rows
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
