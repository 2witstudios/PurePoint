import AppKit

class LineNumberRulerView: NSRulerView {
    private let lineNumberFont = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
    private let textColor = NSColor.tertiaryLabelColor
    private let gutterWidth: CGFloat = 40
    private var boundsObserver: NSObjectProtocol?

    init(textView: NSTextView) {
        super.init(scrollView: textView.enclosingScrollView!, orientation: .verticalRuler)
        clientView = textView
        ruleThickness = gutterWidth

        // Observe scroll and text changes
        if let clipView = textView.enclosingScrollView?.contentView {
            boundsObserver = NotificationCenter.default.addObserver(
                forName: NSView.boundsDidChangeNotification,
                object: clipView,
                queue: .main
            ) { [weak self] _ in
                self?.needsDisplay = true
            }
        }

        NotificationCenter.default.addObserver(
            self,
            selector: #selector(textDidChange(_:)),
            name: NSText.didChangeNotification,
            object: textView
        )
    }

    required init(coder: NSCoder) {
        fatalError("init(coder:) not supported")
    }

    deinit {
        if let observer = boundsObserver {
            NotificationCenter.default.removeObserver(observer)
        }
        NotificationCenter.default.removeObserver(self)
    }

    @objc private func textDidChange(_ notification: Notification) {
        needsDisplay = true
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView = clientView as? NSTextView,
            let layoutManager = textView.layoutManager,
            let textContainer = textView.textContainer
        else { return }

        let visibleRect = scrollView?.contentView.bounds ?? rect
        let visibleGlyphRange = layoutManager.glyphRange(
            forBoundingRect: visibleRect,
            in: textContainer
        )
        let visibleCharRange = layoutManager.characterRange(
            forGlyphRange: visibleGlyphRange,
            actualGlyphRange: nil
        )

        let content = textView.string as NSString
        let containerOrigin = textView.textContainerOrigin
        let attrs: [NSAttributedString.Key: Any] = [
            .font: lineNumberFont,
            .foregroundColor: textColor,
        ]

        var lineNumber = 1
        // Count lines before visible range
        var idx = 0
        while idx < visibleCharRange.location {
            let lineRange = content.lineRange(for: NSRange(location: idx, length: 0))
            lineNumber += 1
            idx = NSMaxRange(lineRange)
        }

        // Draw visible line numbers
        idx = visibleCharRange.location
        let endIdx = NSMaxRange(visibleCharRange)
        while idx < endIdx {
            let lineRange = content.lineRange(for: NSRange(location: idx, length: 0))
            let glyphRange = layoutManager.glyphRange(forCharacterRange: lineRange, actualCharacterRange: nil)
            var lineRect = layoutManager.lineFragmentRect(forGlyphAt: glyphRange.location, effectiveRange: nil)
            lineRect.origin.y += containerOrigin.y

            let numStr = "\(lineNumber)" as NSString
            let strSize = numStr.size(withAttributes: attrs)
            let drawPoint = NSPoint(
                x: gutterWidth - strSize.width - 6,
                y: lineRect.origin.y + (lineRect.height - strSize.height) / 2
            )
            numStr.draw(at: drawPoint, withAttributes: attrs)

            lineNumber += 1
            idx = NSMaxRange(lineRange)
        }
    }
}
