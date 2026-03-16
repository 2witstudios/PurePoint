import AppKit

class LineNumberRulerView: NSRulerView {
    private let lineNumberFont = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
    private let textColor = NSColor.tertiaryLabelColor
    private let gutterWidth: CGFloat = 40
    private var boundsObserver: NSObjectProtocol?
    private var lineStarts: [Int] = [0]

    init(textView: NSTextView) {
        guard let scrollView = textView.enclosingScrollView else {
            fatalError("LineNumberRulerView requires textView to be embedded in an NSScrollView")
        }
        super.init(scrollView: scrollView, orientation: .verticalRuler)
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

        rebuildLineStarts()
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
        rebuildLineStarts()
        needsDisplay = true
    }

    func rebuildLineStarts() {
        guard let textView = clientView as? NSTextView else { return }
        let s = textView.string as NSString
        lineStarts = [0]
        var idx = 0
        while idx < s.length {
            let range = s.lineRange(for: NSRange(location: idx, length: 0))
            let next = NSMaxRange(range)
            if next < s.length { lineStarts.append(next) }
            idx = next
        }
    }

    private func lineIndex(forCharacter charIndex: Int) -> Int {
        var lo = 0, hi = lineStarts.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if lineStarts[mid] <= charIndex { lo = mid + 1 } else { hi = mid }
        }
        return lo - 1
    }

    override func drawHashMarksAndLabels(in rect: NSRect) {
        guard let textView = clientView as? NSTextView,
            let layoutManager = textView.layoutManager,
            let textContainer = textView.textContainer
        else { return }

        let visibleRect = scrollView?.contentView.bounds ?? rect
        let containerOrigin = textView.textContainerOrigin

        // Ensure layout for the visible region
        let glyphRange = layoutManager.glyphRange(forBoundingRect: visibleRect, in: textContainer)
        if glyphRange.length == 0 {
            let attrs: [NSAttributedString.Key: Any] = [
                .font: lineNumberFont,
                .foregroundColor: textColor,
            ]
            let numStr = "1" as NSString
            let strSize = numStr.size(withAttributes: attrs)
            let drawPoint = NSPoint(
                x: gutterWidth - strSize.width - 6,
                y: containerOrigin.y
            )
            numStr.draw(at: drawPoint, withAttributes: attrs)
            return
        }
        let visibleCharRange = layoutManager.characterRange(forGlyphRange: glyphRange, actualGlyphRange: nil)

        let firstLine = lineIndex(forCharacter: visibleCharRange.location)

        let attrs: [NSAttributedString.Key: Any] = [
            .font: lineNumberFont,
            .foregroundColor: textColor,
        ]

        for i in firstLine..<lineStarts.count {
            let charIdx = lineStarts[i]
            if charIdx >= NSMaxRange(visibleCharRange) { break }

            let glyphIdx = layoutManager.glyphIndexForCharacter(at: charIdx)
            var lineRect = layoutManager.lineFragmentRect(forGlyphAt: glyphIdx, effectiveRange: nil)
            lineRect.origin.y += containerOrigin.y

            let lineNumber = i + 1
            let numStr = "\(lineNumber)" as NSString
            let strSize = numStr.size(withAttributes: attrs)
            let drawPoint = NSPoint(
                x: gutterWidth - strSize.width - 6,
                y: lineRect.origin.y + (lineRect.height - strSize.height) / 2
            )
            numStr.draw(at: drawPoint, withAttributes: attrs)
        }
    }
}
