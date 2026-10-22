#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac diff body: the same `DiffTextRendering` text — per-line addition,
/// deletion and context colours, right-aligned line-number gutters and tab-stop alignment —
/// laid out and drawn with Core Text, selectable like the Mac's read-only text view.
class DiffContentNSView: CoreTextBodyView {
    private var hunks: [Hunk] = []

    init(hunks: [Hunk]) {
        self.hunks = hunks
        super.init(inset: DiffTextRendering.diffInset, padding: DiffTextRendering.diffLineFragmentPadding)
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

    private func renderDiff() {
        text = DiffTextRendering.diffAttributedString(hunks: hunks)
    }

    /// Measure bounded pages at the proposed width, never the initial zero-width frame.
    func height(for width: CGFloat) -> CGFloat {
        guard width > 0 else { return 20 }
        return textHeight(for: width)
    }

    override var intrinsicContentSize: CGSize {
        CGSize(width: UIView.noIntrinsicMetric, height: bounds.width > 0 ? height(for: bounds.width) : 20)
    }

    /// The card background, then each line fragment's `.backgroundColor` run across the line,
    /// as TextKit fills a background-coloured line through its newline.
    override func drawBackground(_ layout: CoreTextLayout, in rect: CGRect) {
        Theme.cardBackground.setFill()
        UIRectFill(rect)
        let padding = DiffTextRendering.diffLineFragmentPadding
        for line in layout.lines where line.frame.intersects(rect) {
            guard line.range.location < text.length,
                let color = text.attribute(.backgroundColor, at: line.range.location, effectiveRange: nil) as? UIColor
            else { continue }
            color.setFill()
            UIRectFill(line.frame.insetBy(dx: padding, dy: 0))
        }
    }
}

// MARK: - SwiftUI Bridge

struct DiffContentRepresentable: UIViewRepresentable {
    let hunks: [Hunk]

    func makeUIView(context: Context) -> DiffContentNSView {
        DiffContentNSView(hunks: hunks)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: DiffContentNSView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0 else { return nil }
        return CGSize(width: width, height: uiView.height(for: width))
    }

    func updateUIView(_ uiView: DiffContentNSView, context: Context) {
        uiView.update(hunks: hunks)
    }
}
#endif
