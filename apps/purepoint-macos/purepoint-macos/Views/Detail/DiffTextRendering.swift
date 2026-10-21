#if os(macOS)
import AppKit
#else
import UIKit
#endif

/// The platform-neutral half of the diff and inline code bodies: the attributed text, row
/// colours, insets and gutter drawing. The Mac lays the text out with TextKit
/// (`DiffContentNSView`, `InlineCodeNSView`); the iPad twins lay the same text out with Core
/// Text, so both draw from one definition.
enum DiffTextRendering {
    // MARK: - Diff body

    /// `DiffContentNSView`'s text container inset.
    static let diffInset = CGSize(width: 4, height: 6)
    /// TextKit's default line fragment padding, which the diff body keeps.
    static let diffLineFragmentPadding: CGFloat = 5

    /// The diff body: right-aligned old and new line numbers, then the code, with each line's
    /// addition/deletion/context colours, and a centred `···` row between hunks.
    static func diffAttributedString(hunks: [Hunk]) -> NSAttributedString {
        let result = NSMutableAttributedString()
        let monoFont = NSFont.monospacedSystemFont(ofSize: 12, weight: .regular)
        let lineNoFont = NSFont.monospacedSystemFont(ofSize: 10, weight: .regular)

        let tabStops = [
            NSTextTab(textAlignment: .right, location: 36),
            NSTextTab(textAlignment: .right, location: 72),
            NSTextTab(textAlignment: .left, location: 82),
        ]

        for (hunkIdx, hunk) in hunks.enumerated() {
            // Hunk separator between hunks
            if hunkIdx > 0 {
                let sepPara = NSMutableParagraphStyle()
                sepPara.alignment = .center
                sepPara.paragraphSpacingBefore = 4
                sepPara.paragraphSpacing = 4
                result.append(
                    NSAttributedString(
                        string: "···\n",
                        attributes: [
                            .font: NSFont.systemFont(ofSize: 11),
                            .foregroundColor: NSColor.tertiaryLabelColor,
                            .paragraphStyle: sepPara,
                            .backgroundColor: Theme.hunkSeparator,
                        ]))
            }

            for line in hunk.lines {
                let para = NSMutableParagraphStyle()
                para.tabStops = tabStops

                let bgColor = diffBackground(for: line.type)
                let fgColor: NSColor
                switch line.type {
                case .addition:
                    fgColor = Theme.additionText
                case .deletion:
                    fgColor = Theme.deletionText
                case .context:
                    fgColor = Theme.primaryText
                }

                // Old line number
                let oldNo = line.oldLineNo.map { String($0) } ?? ""
                result.append(
                    NSAttributedString(
                        string: "\t\(oldNo)",
                        attributes: [
                            .font: lineNoFont,
                            .foregroundColor: NSColor.tertiaryLabelColor,
                            .backgroundColor: bgColor,
                            .paragraphStyle: para,
                        ]))

                // New line number
                let newNo = line.newLineNo.map { String($0) } ?? ""
                result.append(
                    NSAttributedString(
                        string: "\t\(newNo)",
                        attributes: [
                            .font: lineNoFont,
                            .foregroundColor: NSColor.tertiaryLabelColor,
                            .backgroundColor: bgColor,
                        ]))

                // Code content
                result.append(
                    NSAttributedString(
                        string: "\t\(line.content)\n",
                        attributes: [
                            .font: monoFont,
                            .foregroundColor: fgColor,
                            .backgroundColor: bgColor,
                        ]))
            }
        }

        return result
    }

    static func diffBackground(for type: LineType) -> NSColor {
        switch type {
        case .addition: Theme.additionBackground
        case .deletion: Theme.deletionBackground
        case .context: Theme.cardBackground
        }
    }

    // MARK: - Inline code body

    /// One source line of the inline code body (`line == nil` is the `⋯` row between hunks).
    struct InlineRow: Equatable {
        let range: NSRange
        let line: DiffLine?
    }

    /// Width of the line-number and +/- gutter left of the code.
    static let inlineGutterWidth: CGFloat = 42
    /// `InlineCodeNSView`'s text container inset (its line fragment padding is zero).
    static let inlineInset = CGSize(width: 4, height: 4)

    /// The inline code body: source only, one row per line. Line numbers and +/- markers are
    /// drawn in the gutter so they cannot confuse the syntax highlighter.
    static func inlineCode(hunks: [Hunk]) -> (text: NSAttributedString, rows: [InlineRow]) {
        let text = NSMutableAttributedString()
        var rows: [InlineRow] = []
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
            rows.append(InlineRow(range: NSRange(location: start, length: text.length - start), line: line))
        }
        for (index, hunk) in hunks.enumerated() {
            if index > 0 { append("⋯", line: nil) }
            for line in hunk.lines { append(line.content, line: line) }
        }
        return (text, rows)
    }

    static func inlineBackground(for type: LineType?) -> NSColor {
        switch type {
        case .addition: Theme.additionBackground
        case .deletion: Theme.deletionBackground
        case .context: Theme.cardBackground
        case nil: Theme.cardHeaderBackground
        }
    }

    /// Fill one inline row's background across `width` and draw its gutter: the line number
    /// right-aligned at x = 27 and the +/− marker at x = 32, top-aligned to the row. Draws into
    /// the current graphics context, which both platforms set up top-down (the Mac view is
    /// flipped).
    static func drawInlineRow(_ row: InlineRow, in rect: CGRect) {
        inlineBackground(for: row.line?.type).setFill()
        #if os(macOS)
        rect.fill()
        #else
        UIRectFill(rect)
        #endif
        guard let line = row.line else { return }
        let lineNo = line.newLineNo ?? line.oldLineNo
        if let lineNo {
            let number = NSString(string: String(lineNo))
            let attrs: [NSAttributedString.Key: Any] = [
                .font: NSFont.monospacedSystemFont(ofSize: 9, weight: .regular),
                .foregroundColor: NSColor.secondaryLabelColor,
            ]
            number.draw(
                at: CGPoint(x: 27 - number.size(withAttributes: attrs).width, y: rect.minY), withAttributes: attrs)
        }
        let marker = line.type == .addition ? "+" : line.type == .deletion ? "−" : ""
        NSString(string: marker).draw(
            at: CGPoint(x: 32, y: rect.minY),
            withAttributes: [
                .font: NSFont.monospacedSystemFont(ofSize: 10, weight: .regular),
                .foregroundColor: line.type == .addition ? Theme.additionText : Theme.deletionText,
            ])
    }
}
