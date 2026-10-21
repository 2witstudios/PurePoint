#if os(macOS)
import AppKit
import SwiftUI

/// AppKit view that renders diff content using NSTextView with per-line background colors,
/// line-number gutters, and tab-stop alignment. Wrapped via DiffContentRepresentable for SwiftUI.
class DiffContentNSView: NSView {
    private let textView = NSTextView()
    private var hunks: [Hunk] = []
    private var cachedHeight: CGFloat = 20
    private var cachedWidth: CGFloat?

    init(hunks: [Hunk]) {
        self.hunks = hunks
        super.init(frame: .zero)
        setupTextView()
        renderDiff()
    }

    required init?(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    func update(hunks: [Hunk]) {
        guard hunks != self.hunks else { return }
        self.hunks = hunks
        renderDiff()
    }

    private func setupTextView() {
        textView.isEditable = false
        textView.isSelectable = true
        textView.drawsBackground = true
        textView.backgroundColor = Theme.cardBackground
        textView.isVerticallyResizable = true
        textView.isHorizontallyResizable = false
        textView.textContainer?.widthTracksTextView = false
        textView.textContainerInset = DiffTextRendering.diffInset
        textView.autoresizingMask = [.width]
        textView.translatesAutoresizingMaskIntoConstraints = false
        addSubview(textView)

        NSLayoutConstraint.activate([
            textView.topAnchor.constraint(equalTo: topAnchor),
            textView.leadingAnchor.constraint(equalTo: leadingAnchor),
            textView.trailingAnchor.constraint(equalTo: trailingAnchor),
            textView.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
    }

    private func renderDiff() {
        let attributed = buildDiffAttributedString()
        textView.textStorage?.setAttributedString(attributed)

        cachedWidth = nil
        invalidateIntrinsicContentSize()
    }

    /// Measure bounded pages at the proposed width, never the initial zero-width frame.
    func height(for width: CGFloat) -> CGFloat {
        guard width > 0 else { return 20 }
        if cachedWidth == width { return cachedHeight }
        if let container = textView.textContainer, let manager = textView.layoutManager {
            container.containerSize = NSSize(width: max(1, width - 8), height: .greatestFiniteMagnitude)
            manager.ensureLayout(for: container)
            cachedHeight = ceil(manager.usedRect(for: container).height) + 12
        }
        cachedWidth = width
        return cachedHeight
    }

    override var intrinsicContentSize: NSSize {
        NSSize(width: NSView.noIntrinsicMetric, height: cachedHeight)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        textView.backgroundColor = Theme.cardBackground
        renderDiff()
    }

    private func buildDiffAttributedString() -> NSAttributedString {
        DiffTextRendering.diffAttributedString(hunks: hunks)
    }
}

// MARK: - SwiftUI Bridge

struct DiffContentRepresentable: NSViewRepresentable {
    let hunks: [Hunk]

    func makeNSView(context: Context) -> DiffContentNSView {
        DiffContentNSView(hunks: hunks)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: DiffContentNSView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0 else { return nil }
        return CGSize(width: width, height: nsView.height(for: width))
    }

    func updateNSView(_ nsView: DiffContentNSView, context: Context) {
        nsView.update(hunks: hunks)
    }
}
#endif
