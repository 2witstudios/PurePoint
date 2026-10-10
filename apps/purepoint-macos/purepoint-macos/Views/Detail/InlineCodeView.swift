import AppKit
import SwiftUI

/// A selectable, syntax-highlighted code body with no nested scroll view.
/// The sidebar owns scrolling; TextKit measures wrapped lines at its actual width.
struct InlineCodeView: NSViewRepresentable {
    let hunks: [Hunk]
    let language: EditorLanguage

    func makeNSView(context: Context) -> InlineCodeNSView {
        let view = InlineCodeNSView()
        view.update(hunks: hunks, language: language)
        return view
    }

    func updateNSView(_ view: InlineCodeNSView, context: Context) {
        view.update(hunks: hunks, language: language)
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: InlineCodeNSView, context: Context) -> CGSize? {
        guard let width = proposal.width, width > 0 else { return nil }
        return CGSize(width: width, height: nsView.height(for: width))
    }
}

final class InlineCodeNSView: NSView {
    private struct Row {
        let range: NSRange
        let line: DiffLine?
    }

    private let textView: NSTextView
    private var highlighter: SyntaxHighlightManager?
    private var hunks: [Hunk] = []
    private var language: EditorLanguage = .plaintext
    private var rows: [Row] = []
    private let gutterWidth: CGFloat = 42

    init() {
        let storage = NSTextStorage()
        let layout = NSLayoutManager()
        storage.addLayoutManager(layout)
        let container = NSTextContainer(containerSize: NSSize(width: 300, height: CGFloat.greatestFiniteMagnitude))
        container.widthTracksTextView = false
        container.lineFragmentPadding = 0
        layout.addTextContainer(container)
        textView = NSTextView(frame: .zero, textContainer: container)
        super.init(frame: .zero)
        textView.isEditable = false
        textView.isSelectable = true
        textView.drawsBackground = false
        textView.isRichText = false
        textView.textContainerInset = NSSize(width: 4, height: 4)
        textView.isHorizontallyResizable = false
        textView.isVerticallyResizable = true
        // TextKit can resize the text view during layout. Keep it within the
        // SwiftUI-assigned body instead of painting over the surrounding rows.
        clipsToBounds = true
        addSubview(textView)
        highlighter = SyntaxHighlightManager(textView: textView, fontSize: 11)
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    override var isFlipped: Bool { true }

    func update(hunks: [Hunk], language: EditorLanguage) {
        guard self.hunks != hunks || self.language != language else { return }
        self.hunks = hunks
        self.language = language
        render()
    }

    func height(for width: CGFloat) -> CGFloat {
        let textWidth = max(1, width - gutterWidth)
        textView.textContainer?.containerSize = NSSize(
            width: max(1, textWidth - 8), height: CGFloat.greatestFiniteMagnitude)
        guard let container = textView.textContainer, let layout = textView.layoutManager else { return 8 }
        layout.ensureLayout(for: container)
        let height = ceil(layout.usedRect(for: container).height) + 8
        return height
    }

    override func layout() {
        super.layout()
        let height = height(for: bounds.width)
        textView.frame = NSRect(x: gutterWidth, y: 0, width: max(1, bounds.width - gutterWidth), height: height)
        needsDisplay = true
    }

    private func render() {
        let text = NSMutableAttributedString()
        rows = []
        let paragraph = NSMutableParagraphStyle()
        paragraph.lineSpacing = 2
        paragraph.defaultTabInterval = 24
        func append(_ content: String, line: DiffLine?) {
            let start = text.length
            text.append(
                NSAttributedString(
                    string: content + "\n",
                    attributes: [
                        .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .regular),
                        .foregroundColor: line == nil ? NSColor.tertiaryLabelColor : Theme.primaryText,
                        .paragraphStyle: paragraph,
                    ]))
            rows.append(Row(range: NSRange(location: start, length: text.length - start), line: line))
        }
        for (index, hunk) in hunks.enumerated() {
            if index > 0 { append("⋯", line: nil) }
            for line in hunk.lines { append(line.content, line: line) }
        }
        textView.textStorage?.setAttributedString(text)
        // Source only: line numbers and +/- markers are drawn in the gutter,
        // so they cannot confuse the existing tree-sitter syntax highlighter.
        highlighter?.setLanguage(language)
        highlighter?.invalidate()
        needsLayout = true
        needsDisplay = true
    }

    override func draw(_ dirtyRect: NSRect) {
        super.draw(dirtyRect)
        Theme.cardBackground.setFill()
        dirtyRect.fill()
        guard let layout = textView.layoutManager, let container = textView.textContainer else { return }
        let origin = textView.textContainerOrigin
        for row in rows {
            let glyphs = layout.glyphRange(forCharacterRange: row.range, actualCharacterRange: nil)
            var rect = layout.boundingRect(forGlyphRange: glyphs, in: container)
            rect.origin.y += origin.y
            rect.origin.x = 0
            rect.size.width = bounds.width
            guard rect.intersects(dirtyRect) else { continue }
            let color: NSColor
            switch row.line?.type {
            case .addition: color = Theme.additionBackground
            case .deletion: color = Theme.deletionBackground
            case .context: color = Theme.cardBackground
            case nil: color = Theme.cardHeaderBackground
            }
            color.setFill()
            rect.fill()
            guard let line = row.line else { continue }
            let lineNo = line.newLineNo ?? line.oldLineNo
            if let lineNo {
                let number = NSString(string: String(lineNo))
                let attrs: [NSAttributedString.Key: Any] = [
                    .font: NSFont.monospacedSystemFont(ofSize: 9, weight: .regular),
                    .foregroundColor: NSColor.secondaryLabelColor,
                ]
                number.draw(
                    at: NSPoint(x: 27 - number.size(withAttributes: attrs).width, y: rect.minY), withAttributes: attrs)
            }
            let marker = line.type == .addition ? "+" : line.type == .deletion ? "−" : ""
            NSString(string: marker).draw(
                at: NSPoint(x: 32, y: rect.minY),
                withAttributes: [
                    .font: NSFont.monospacedSystemFont(ofSize: 10, weight: .regular),
                    .foregroundColor: line.type == .addition ? Theme.additionText : Theme.deletionText,
                ])
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        render()
    }
}
