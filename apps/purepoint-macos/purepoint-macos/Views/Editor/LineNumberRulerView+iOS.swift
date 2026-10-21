#if os(iOS)
import UIKit

/// iOS twin of the Mac line-number gutter. UIKit has no scroll-view rulers, so the gutter is a
/// 40-point view beside the text view that redraws as the text scrolls, changes or relays out.
/// Each number sits centered on the first line fragment of its line, as on the Mac.
class LineNumberRulerView: UIView {
    private let lineNumberFont = NSFont.monospacedSystemFont(ofSize: 11, weight: .regular)
    private let textColor = NSColor.tertiaryLabelColor
    static let gutterWidth: CGFloat = 40
    private var gutterWidth: CGFloat { Self.gutterWidth }
    private weak var textView: UITextView?
    private var lineStarts: [Int] = [0]
    private var textObserver: NSObjectProtocol?

    init(textView: UITextView) {
        self.textView = textView
        super.init(frame: CGRect(x: 0, y: 0, width: Self.gutterWidth, height: 0))
        backgroundColor = textView.backgroundColor
        isOpaque = true
        contentMode = .redraw
        isUserInteractionEnabled = false
        isAccessibilityElement = false

        textObserver = NotificationCenter.default.addObserver(
            forName: UITextView.textDidChangeNotification,
            object: textView,
            queue: .main
        ) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.rebuildLineStarts()
                self?.setNeedsDisplay()
            }
        }

        registerForTraitChanges([UITraitUserInterfaceStyle.self]) { (self: Self, _) in
            self.setNeedsDisplay()
        }

        rebuildLineStarts()
    }

    required init?(coder: NSCoder) { fatalError("init(coder:) not supported") }

    deinit {
        if let textObserver { NotificationCenter.default.removeObserver(textObserver) }
    }

    func rebuildLineStarts() {
        guard let textView else { return }
        lineStarts = Self.lineStarts(in: textView.text as NSString)
    }

    // MARK: - Line Index

    /// The UTF-16 offset each line starts at. A trailing newline does not start a numbered
    /// line, matching the Mac gutter.
    nonisolated static func lineStarts(in s: NSString) -> [Int] {
        var starts = [0]
        var idx = 0
        while idx < s.length {
            let range = s.lineRange(for: NSRange(location: idx, length: 0))
            let next = NSMaxRange(range)
            if next < s.length { starts.append(next) }
            idx = next
        }
        return starts
    }

    /// The line holding `charIndex`, by binary search over `lineStarts`.
    nonisolated static func lineIndex(forCharacter charIndex: Int, in lineStarts: [Int]) -> Int {
        var lo = 0, hi = lineStarts.count
        while lo < hi {
            let mid = (lo + hi) / 2
            if lineStarts[mid] <= charIndex { lo = mid + 1 } else { hi = mid }
        }
        return lo - 1
    }

    // MARK: - Drawing

    override func draw(_ rect: CGRect) {
        guard let textView,
            let layoutManager = textView.textLayoutManager,
            let contentManager = layoutManager.textContentManager
        else { return }

        textView.backgroundColor?.setFill()
        UIRectFill(rect)

        let attrs: [NSAttributedString.Key: Any] = [
            .font: lineNumberFont,
            .foregroundColor: textColor,
        ]
        // Text-container coordinates → gutter coordinates.
        let originY = textView.textContainerInset.top - textView.contentOffset.y
        let visibleTop = textView.contentOffset.y - textView.textContainerInset.top
        let visibleBottom = visibleTop + textView.bounds.height

        if textView.text.isEmpty {
            draw("1", top: originY, attrs: attrs)
            return
        }

        let documentStart = contentManager.documentRange.location
        let startLocation =
            layoutManager.textLayoutFragment(for: CGPoint(x: 0, y: max(visibleTop, 0)))?.rangeInElement.location
            ?? documentStart

        layoutManager.enumerateTextLayoutFragments(from: startLocation, options: [.ensuresLayout]) { fragment in
            let frame = fragment.layoutFragmentFrame
            if frame.minY > visibleBottom { return false }

            let charIdx = contentManager.offset(from: documentStart, to: fragment.rangeInElement.location)
            let line = Self.lineIndex(forCharacter: charIdx, in: lineStarts)
            // Only fragments that begin a numbered line get a number (not the empty fragment
            // after a trailing newline).
            guard line >= 0, lineStarts[line] == charIdx else { return true }

            var lineRect = frame
            if let first = fragment.textLineFragments.first {
                lineRect = first.typographicBounds.offsetBy(dx: frame.minX, dy: frame.minY)
            }
            let numStr = "\(line + 1)" as NSString
            let strSize = numStr.size(withAttributes: attrs)
            numStr.draw(
                at: CGPoint(
                    x: gutterWidth - strSize.width - 6,
                    y: originY + lineRect.minY + (lineRect.height - strSize.height) / 2
                ),
                withAttributes: attrs
            )
            return true
        }
    }

    private func draw(_ string: String, top: CGFloat, attrs: [NSAttributedString.Key: Any]) {
        let numStr = string as NSString
        let strSize = numStr.size(withAttributes: attrs)
        numStr.draw(at: CGPoint(x: gutterWidth - strSize.width - 6, y: top), withAttributes: attrs)
    }

}
#endif
