#if os(iOS)
import CoreText
import UIKit

/// Lays a `DiffTextRendering` attributed string out with Core Text the way the Mac's TextKit
/// text container does: wrapped at the container width, tab stops measured from the container
/// edge, paragraph spacing and line spacing from each paragraph's style, and a trailing extra
/// line for the final newline (TextKit's extra line fragment, which the Mac's measured height
/// includes).
nonisolated struct CoreTextLayout {
    struct Line {
        let ctLine: CTLine
        /// Characters on this line; a paragraph's last line also owns its newline.
        let range: NSRange
        /// The line fragment, in view coordinates (top-down).
        let frame: CGRect
        /// Where the line's pen starts, in view coordinates.
        let origin: CGPoint
        let baseline: CGFloat
    }

    let string: NSString
    let lines: [Line]
    /// Total height including the top and bottom insets.
    let height: CGFloat

    /// - Parameters:
    ///   - text: The text, with UIKit attributes.
    ///   - width: The text container's width.
    ///   - inset: The text container's inset (its origin in the view).
    ///   - padding: The line fragment padding inside the container.
    ///   - traits: Resolves dynamic colours.
    init(
        text: NSAttributedString, width: CGFloat, inset: CGSize, padding: CGFloat,
        traits: UITraitCollection = .current
    ) {
        string = text.string as NSString
        let typeset = Self.coreTextString(text, padding: padding, traits: traits)
        let typesetter = CTTypesetterCreateWithAttributedString(typeset)
        let lineWidth = max(1, Double(width - padding * 2))
        var lines: [Line] = []
        var y = inset.height
        var location = 0
        var lastStyle: NSParagraphStyle?
        let length = string.length

        while location < length {
            let paragraph = string.paragraphRange(for: NSRange(location: location, length: 0))
            let style = text.attribute(.paragraphStyle, at: paragraph.location, effectiveRange: nil) as? NSParagraphStyle
            if !lines.isEmpty { y += style?.paragraphSpacingBefore ?? 0 }
            let contentEnd = Self.contentEnd(of: paragraph, in: string)
            var start = paragraph.location
            repeat {
                let count: Int
                if start < contentEnd {
                    let suggested = CTTypesetterSuggestLineBreak(typesetter, start, lineWidth)
                    count = max(1, min(suggested, contentEnd - start))
                } else {
                    // An empty source line: the line is just its newline.
                    count = paragraph.upperBound - start
                }
                let ctLine = CTTypesetterCreateLine(typesetter, CFRange(location: start, length: count))
                var ascent: CGFloat = 0
                var descent: CGFloat = 0
                var leading: CGFloat = 0
                CTLineGetTypographicBounds(ctLine, &ascent, &descent, &leading)
                let lineHeight = ascent + descent + leading
                let isLast = start + count >= contentEnd
                let owned = isLast ? paragraph.upperBound - start : count
                let x = Self.penOffset(ctLine, style: style, lineWidth: lineWidth)
                lines.append(
                    Line(
                        ctLine: ctLine, range: NSRange(location: start, length: owned),
                        frame: CGRect(
                            x: inset.width, y: y, width: width, height: lineHeight + (style?.lineSpacing ?? 0)),
                        origin: CGPoint(x: inset.width + padding + x, y: y), baseline: y + ascent))
                y += lineHeight + (style?.lineSpacing ?? 0)
                start += owned
            } while start < paragraph.upperBound
            y += style?.paragraphSpacing ?? 0
            lastStyle = style
            location = paragraph.upperBound
        }

        // TextKit's extra line fragment after a final newline.
        if length == 0 || string.character(at: length - 1) == 0x0A {
            let font = length > 0
                ? (text.attribute(.font, at: length - 1, effectiveRange: nil) as? UIFont) : nil
            let lineFont = font ?? UIFont.systemFont(ofSize: 12)
            y += lineFont.ascender - lineFont.descender + lineFont.leading + (lastStyle?.lineSpacing ?? 0)
        }
        self.lines = lines
        height = y + inset.height
    }

    // MARK: - Hit testing

    /// The line nearest to `point`'s y.
    func lineIndex(at point: CGPoint) -> Int? {
        guard !lines.isEmpty else { return nil }
        if point.y < lines[0].frame.minY { return 0 }
        return lines.firstIndex { point.y < $0.frame.maxY } ?? lines.count - 1
    }

    /// The caret position (a UTF-16 offset) nearest to `point`.
    func characterIndex(at point: CGPoint) -> Int {
        guard let index = lineIndex(at: point) else { return 0 }
        let line = lines[index]
        let local = CGPoint(x: point.x - line.origin.x, y: 0)
        var offset = CTLineGetStringIndexForPosition(line.ctLine, local)
        if offset == kCFNotFound { offset = line.range.location }
        // Never place the caret after a line's newline: that position belongs to the next line.
        let lastCaret = max(line.range.location, line.range.upperBound - (endsWithNewline(line) ? 1 : 0))
        return min(max(offset, line.range.location), lastCaret)
    }

    /// The x of the caret at `index` on `line`, in view coordinates.
    func caretX(_ index: Int, on line: Line) -> CGFloat {
        line.origin.x + CTLineGetOffsetForStringIndex(line.ctLine, index, nil)
    }

    /// Rectangles covering `range`, one per line. A selection running past a line's end
    /// extends to the container's right edge, as TextKit draws it.
    func selectionRects(for range: NSRange) -> [CGRect] {
        guard range.length > 0 else { return [] }
        return lines.compactMap { line in
            let overlap = NSIntersectionRange(range, line.range)
            guard overlap.length > 0 else { return nil }
            let startX = caretX(overlap.location, on: line)
            let throughEnd = overlap.upperBound >= line.range.upperBound && endsWithNewline(line)
            let endX = throughEnd ? line.frame.maxX : caretX(overlap.upperBound, on: line)
            return CGRect(x: startX, y: line.frame.minY, width: max(1, endX - startX), height: line.frame.height)
        }
    }

    /// The union of the line fragments holding `range`.
    func boundingRect(for range: NSRange) -> CGRect {
        lines.filter { NSIntersectionRange(range, $0.range).length > 0 || $0.range.location == range.location }
            .reduce(CGRect.null) { $0.union($1.frame) }
    }

    /// The word around `index`, for double-tap selection.
    func wordRange(at index: Int) -> NSRange {
        let length = string.length
        guard length > 0 else { return NSRange(location: 0, length: 0) }
        let at = min(index, length - 1)
        func isWord(_ i: Int) -> Bool {
            guard let scalar = UnicodeScalar(string.character(at: i)) else { return true }
            return CharacterSet.alphanumerics.contains(scalar) || scalar == "_"
        }
        guard isWord(at) else { return NSRange(location: at, length: 1) }
        var start = at
        var end = at + 1
        while start > 0, isWord(start - 1) { start -= 1 }
        while end < length, isWord(end) { end += 1 }
        return NSRange(location: start, length: end - start)
    }

    /// The paragraph (source line) around `index`, for triple-tap selection.
    func paragraphRange(at index: Int) -> NSRange {
        string.paragraphRange(for: NSRange(location: min(index, max(0, string.length - 1)), length: 0))
    }

    // MARK: - Private

    private func endsWithNewline(_ line: Line) -> Bool {
        line.range.length > 0 && string.character(at: line.range.upperBound - 1) == 0x0A
    }

    private static func contentEnd(of paragraph: NSRange, in string: NSString) -> Int {
        var end = paragraph.upperBound
        while end > paragraph.location, [0x0A, 0x0D].contains(string.character(at: end - 1)) { end -= 1 }
        return end
    }

    private static func penOffset(_ line: CTLine, style: NSParagraphStyle?, lineWidth: Double) -> CGFloat {
        switch style?.alignment {
        case .center: CGFloat(CTLineGetPenOffsetForFlush(line, 0.5, lineWidth))
        case .right: CGFloat(CTLineGetPenOffsetForFlush(line, 1, lineWidth))
        default: 0
        }
    }

    /// The same runs with Core Text's own attributes: resolved `CGColor` foregrounds and a
    /// `CTParagraphStyle` carrying the tab stops. Tab locations are measured from the container
    /// edge in TextKit but from the line's pen start in Core Text, so they move left by the
    /// line fragment padding.
    private static func coreTextString(
        _ text: NSAttributedString, padding: CGFloat, traits: UITraitCollection
    ) -> NSAttributedString {
        let result = NSMutableAttributedString(string: text.string)
        let whole = NSRange(location: 0, length: text.length)
        text.enumerateAttributes(in: whole) { attributes, range, _ in
            var converted: [NSAttributedString.Key: Any] = [:]
            if let font = attributes[.font] as? UIFont {
                converted[NSAttributedString.Key(kCTFontAttributeName as String)] = font as CTFont
            }
            if let color = attributes[.foregroundColor] as? UIColor {
                converted[NSAttributedString.Key(kCTForegroundColorAttributeName as String)] =
                    color.resolvedColor(with: traits).cgColor
            }
            if let style = attributes[.paragraphStyle] as? NSParagraphStyle {
                converted[NSAttributedString.Key(kCTParagraphStyleAttributeName as String)] =
                    paragraphStyle(style, padding: padding)
            }
            result.addAttributes(converted, range: range)
        }
        // Paragraph styles apply per paragraph in TextKit: spread each paragraph's first style
        // across the paragraph so a line's tab stops do not depend on which run they fall in.
        let string = text.string as NSString
        var location = 0
        while location < string.length {
            let paragraph = string.paragraphRange(for: NSRange(location: location, length: 0))
            if let style = text.attribute(.paragraphStyle, at: paragraph.location, effectiveRange: nil)
                as? NSParagraphStyle
            {
                result.addAttribute(
                    NSAttributedString.Key(kCTParagraphStyleAttributeName as String),
                    value: paragraphStyle(style, padding: padding), range: paragraph)
            }
            location = paragraph.upperBound
        }
        return result
    }

    private static func paragraphStyle(_ style: NSParagraphStyle, padding: CGFloat) -> CTParagraphStyle {
        let tabs: [CTTextTab] = style.tabStops.map { tab in
            let alignment: CTTextAlignment =
                switch tab.alignment {
                case .right: .right
                case .center: .center
                default: .left
                }
            return CTTextTabCreate(alignment, Double(max(0, tab.location - padding)), nil)
        }
        var interval = style.defaultTabInterval
        let tabArray = tabs as CFArray
        return withUnsafeBytes(of: &interval) { intervalBytes in
            withUnsafeBytes(of: tabArray) { tabBytes in
                let settings = [
                    CTParagraphStyleSetting(
                        spec: .defaultTabInterval, valueSize: MemoryLayout<CGFloat>.size,
                        value: intervalBytes.baseAddress!),
                    CTParagraphStyleSetting(
                        spec: .tabStops, valueSize: MemoryLayout<CFArray>.size, value: tabBytes.baseAddress!),
                ]
                return CTParagraphStyleCreate(settings, settings.count)
            }
        }
    }
}
#endif
