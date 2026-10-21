#if os(iOS)
import Testing
import UIKit

@testable import PurePoint

@MainActor
struct DiffRenderingIOSTests {
    private let hunk = Hunk(
        header: "@@ -1,2 +1,2 @@",
        lines: [
            DiffLine(type: .context, content: "let a = 1", oldLineNo: 12, newLineNo: 12),
            DiffLine(type: .deletion, content: "let b = 2", oldLineNo: 13, newLineNo: nil),
            DiffLine(type: .addition, content: "let b = 3", oldLineNo: nil, newLineNo: 13),
        ])

    private func diffLayout(_ hunks: [Hunk], width: CGFloat = 600) -> CoreTextLayout {
        CoreTextLayout(
            text: DiffTextRendering.diffAttributedString(hunks: hunks), width: width - 8,
            inset: DiffTextRendering.diffInset, padding: DiffTextRendering.diffLineFragmentPadding)
    }

    @Test func diffTabStopsAlignToTheMacGutters() {
        let layout = diffLayout([hunk])
        let line = layout.lines[0]
        let string = layout.string
        // "\t12\t12\tlet a = 1\n": the old number ends at the right tab (36), the new number at
        // the second (72), and the code starts at the left tab (82), all from the container edge.
        let inset = DiffTextRendering.diffInset.width
        let oldEnd = string.range(of: "12").upperBound
        let newEnd = string.range(of: "12", range: NSRange(location: oldEnd, length: 6)).upperBound
        let codeStart = string.range(of: "let a").location
        #expect(abs(layout.caretX(oldEnd, on: line) - (inset + 36)) < 0.5)
        #expect(abs(layout.caretX(newEnd, on: line) - (inset + 72)) < 0.5)
        #expect(abs(layout.caretX(codeStart, on: line) - (inset + 82)) < 0.5)
    }

    @Test func diffHeightReflectsWrappingAndHunkSeparators() {
        let long = DiffLine(type: .addition, content: String(repeating: "source text ", count: 20), oldLineNo: nil, newLineNo: 1)
        let wide = diffLayout([Hunk(header: "", lines: [long])], width: 2000).height
        let narrow = diffLayout([Hunk(header: "", lines: [long])], width: 320).height
        #expect(narrow > wide)

        let one = diffLayout([hunk]).height
        let two = diffLayout([hunk, hunk]).height
        // The second hunk adds its three lines, the `···` row and its 4 + 4 points of spacing.
        #expect(two > one * 1.9)
        #expect(diffLayout([hunk, hunk]).lines.count == 7)
    }

    @Test func diffViewMeasuresAtTheProposedWidth() {
        let view = DiffContentNSView(hunks: [hunk])
        #expect(view.height(for: 0) == 20)
        let measured = view.height(for: 600)
        #expect(measured == ceil(diffLayout([hunk]).height))
        view.update(hunks: [Hunk(header: "", lines: [hunk.lines[0]])])
        #expect(view.height(for: 600) < measured)
    }

    @Test func hitTestingRoundTripsAndStaysBeforeTheNewline() {
        let layout = diffLayout([hunk])
        let line = layout.lines[1]
        let codeStart = layout.string.range(of: "let b = 2").location
        let point = CGPoint(x: layout.caretX(codeStart + 4, on: line) + 1, y: line.frame.midY)
        #expect(layout.characterIndex(at: point) == codeStart + 4)
        // Far right of the line: the caret sits before the newline, not on the next line.
        let end = layout.characterIndex(at: CGPoint(x: 5000, y: line.frame.midY))
        #expect(end == line.range.upperBound - 1)
    }

    @Test func selectionThroughANewlineExtendsToTheContainerEdge() {
        let layout = diffLayout([hunk])
        let rects = layout.selectionRects(for: layout.lines[0].range)
        #expect(rects.count == 1)
        #expect(rects[0].maxX == layout.lines[0].frame.maxX)
        let partial = layout.selectionRects(for: NSRange(location: layout.lines[0].range.location, length: 2))
        #expect(partial[0].maxX < layout.lines[0].frame.maxX)
    }

    @Test func wordAndParagraphRanges() {
        let layout = diffLayout([hunk])
        let index = layout.string.range(of: "let a").location + 1
        #expect(layout.string.substring(with: layout.wordRange(at: index)) == "let")
        #expect(layout.string.substring(with: layout.paragraphRange(at: index)) == "\t12\t12\tlet a = 1\n")
    }

    @Test func viewCopiesTheSelection() {
        let view = DiffContentNSView(hunks: [hunk])
        #expect(!view.canPerformAction(#selector(UIResponder.copy(_:)), withSender: nil))
        view.selectAll(nil)
        #expect(view.canPerformAction(#selector(UIResponder.copy(_:)), withSender: nil))
        #expect(view.selectedText == DiffTextRendering.diffAttributedString(hunks: [hunk]).string)
    }

    @Test func inlineRowsCoverEachSourceLineAndTheHunkGap() {
        let (text, rows) = DiffTextRendering.inlineCode(hunks: [hunk, hunk])
        #expect(rows.count == 7)
        #expect(rows[3].line == nil)
        #expect((text.string as NSString).substring(with: rows[1].range) == "let b = 2\n")
        let layout = CoreTextLayout(
            text: text, width: 400, inset: CGSize(width: 46, height: 4), padding: 0)
        // Each row's lines are contiguous: backgrounds tile with no gaps.
        let first = layout.boundingRect(for: rows[0].range)
        let second = layout.boundingRect(for: rows[1].range)
        #expect(first.maxY == second.minY)
        #expect(first.minX == 46)
    }

    @Test func inlineViewHighlightsSwiftSource() {
        let runs = SyntaxHighlightManager.highlightRuns(in: "let value = 1\n", language: .swift, fontSize: 11)
        let keyword = runs.first { $0.range == NSRange(location: 0, length: 3) }
        #expect(keyword != nil)
        #expect((keyword?.attributes[.font] as? UIFont)?.pointSize == 11)
        #expect(SyntaxHighlightManager.highlightRuns(in: "let", language: .plaintext, fontSize: 11).isEmpty)
    }
}
#endif
