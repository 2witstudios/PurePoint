#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac inline code body, with the same inputs and measuring.
struct InlineCodeView: UIViewRepresentable {
    let hunks: [Hunk]
    let language: EditorLanguage

    func makeUIView(context: Context) -> InlineCodeNSView {
        let view = InlineCodeNSView()
        view.update(hunks: hunks, language: language)
        return view
    }

    func updateUIView(_ view: InlineCodeNSView, context: Context) {
        view.update(hunks: hunks, language: language)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, uiView: InlineCodeNSView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0 else { return nil }
        return CGSize(width: width, height: uiView.height(for: width))
    }
}

/// iOS twin of the Mac inline code view: the same `DiffTextRendering` source rows and gutter,
/// syntax-highlighted by the shared tree-sitter highlighter, drawn with Core Text and
/// selectable. No nested scroll view: the sidebar owns scrolling.
final class InlineCodeNSView: CoreTextBodyView {
    private var hunks: [Hunk] = []
    private var language: EditorLanguage = .plaintext
    private var rows: [DiffTextRendering.InlineRow] = []
    private let fontSize: CGFloat = 11

    init() {
        super.init(
            inset: DiffTextRendering.inlineInset, padding: 0, textLeft: DiffTextRendering.inlineGutterWidth)
        // Keep the drawing within the SwiftUI-assigned body, as the Mac view clips TextKit.
        clipsToBounds = true
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    func update(hunks: [Hunk], language: EditorLanguage) {
        guard self.hunks != hunks || self.language != language else { return }
        self.hunks = hunks
        self.language = language
        render()
    }

    func height(for width: CGFloat) -> CGFloat {
        textHeight(for: width)
    }

    private func render() {
        let (source, rows) = DiffTextRendering.inlineCode(hunks: hunks)
        self.rows = rows
        // Source only: line numbers and +/- markers are drawn in the gutter,
        // so they cannot confuse the tree-sitter syntax highlighter.
        let highlighted = NSMutableAttributedString(attributedString: source)
        for run in SyntaxHighlightManager.highlightRuns(in: source.string, language: language, fontSize: fontSize) {
            highlighted.addAttributes(run.attributes, range: run.range)
        }
        text = highlighted
    }

    override func drawBackground(_ layout: CoreTextLayout, in rect: CGRect) {
        Theme.cardBackground.setFill()
        UIRectFill(rect)
        for row in rows {
            var rowRect = layout.boundingRect(for: row.range)
            guard !rowRect.isNull else { continue }
            rowRect.origin.x = 0
            rowRect.size.width = bounds.width
            guard rowRect.intersects(rect) else { continue }
            DiffTextRendering.drawInlineRow(row, in: rowRect)
        }
    }
}
#endif
