import XCTest

final class ChatMarkdownTests: XCTestCase {
    func testGivenMultilineReplyShouldPreserveLinesAndSeparateParagraphs() {
        XCTAssertEqual(ChatMarkdown.parse("First\nsecond\n\nThird"), [.paragraph("First\nsecond"), .paragraph("Third")])
    }
    func testGivenStructuredReplyShouldRenderHeadingsTasksAndNestedLists() {
        XCTAssertEqual(ChatMarkdown.parse("## Plan\n\n3. Ship\n   - [x] Desktop\n   - [ ] Phone"), [
            .heading(level: 2, text: "Plan"),
            .listItem(depth: 0, marker: "3.", checked: nil, text: "Ship"),
            .listItem(depth: 1, marker: "•", checked: true, text: "Desktop"),
            .listItem(depth: 1, marker: "•", checked: false, text: "Phone")])
    }
    func testGivenStreamingFenceShouldRetainCodeAndRespectFenceLength() {
        XCTAssertEqual(ChatMarkdown.parse("````swift\n```\nlet x = 1"), [.code(language: "swift", text: "```\nlet x = 1")])
        XCTAssertEqual(ChatMarkdown.parse("~~~sh\necho hi\n~~~\nDone"), [.code(language: "sh", text: "echo hi"), .paragraph("Done")])
    }
    func testGivenTableShouldKeepEscapedPipesAndCodeCells() {
        XCTAssertEqual(ChatMarkdown.parse("| Name | Value |\n| :--- | ---: |\n| a\\|b | `x|y` |"), [
            .table(headers: ["Name", "Value"], alignments: [.leading, .trailing], rows: [["a|b", "`x|y`"]])])
    }
    func testGivenWindowsLineEndingsShouldRenderParagraphsNormally() {
        XCTAssertEqual(ChatMarkdown.parse("First\r\nsecond\r\n\r\nThird"), [.paragraph("First\nsecond"), .paragraph("Third")])
    }
    func testGivenEmptyCodeFenceShouldRetainItsCopyablePanel() {
        XCTAssertEqual(ChatMarkdown.parse("```swift\n```"), [.code(language: "swift", text: "")])
    }
    func testGivenShortTableRowsShouldRetainColumnPositions() {
        XCTAssertEqual(ChatMarkdown.parse("A | B | C\n--- | --- | ---\nfirst | second"), [.table(headers: ["A", "B", "C"], alignments: [.leading, .leading, .leading], rows: [["first", "second", ""]])])
    }
    func testGivenImageReplyShouldExposeAnAccessibleRemoteImage() {
        XCTAssertEqual(ChatMarkdown.parse("![Release preview](https://example.com/preview.png)"), [.image(alt: "Release preview", url: URL(string: "https://example.com/preview.png")!)])
        XCTAssertEqual(ChatMarkdown.parse("![private](file:///tmp/private.png)"), [.paragraph("![private](file:///tmp/private.png)")])
    }
    func testGivenListWithCodeShouldRenderItsNestedPanelAndKeepCodeWhitespace() {
        XCTAssertEqual(ChatMarkdown.parse("1. Run this:\n   ```swift\n   if ready {\n       run()\n   }\n   ```\n2. Continue"), [
            .listItem(depth: 0, marker: "1.", checked: nil, text: "Run this:"),
            .listContinuation(depth: 1, blocks: [.code(language: "swift", text: "if ready {\n    run()\n}")]),
            .listItem(depth: 0, marker: "2.", checked: nil, text: "Continue")])
    }
    func testGivenListWithQuotedAdviceShouldRenderItsQuote() {
        XCTAssertEqual(ChatMarkdown.parse("- Check this:\n  > **Keep it readable.**\n- Done"), [
            .listItem(depth: 0, marker: "•", checked: nil, text: "Check this:"),
            .listContinuation(depth: 1, blocks: [.quote([.paragraph("**Keep it readable.**")])]),
            .listItem(depth: 0, marker: "•", checked: nil, text: "Done")])
    }
    func testGivenQuotedReplyShouldKeepItsStructure() {
        XCTAssertEqual(ChatMarkdown.parse("> ## Note\n> Text\n\n---"), [.quote([.heading(level: 2, text: "Note"), .paragraph("Text")]), .rule])
    }
    func testGivenIndentedListContinuationShouldKeepItWithItsItem() {
        XCTAssertEqual(ChatMarkdown.parse("- First\n  continued\n- Second"), [
            .listItem(depth: 0, marker: "•", checked: nil, text: "First\ncontinued"),
            .listItem(depth: 0, marker: "•", checked: nil, text: "Second")])
    }
    func testGivenDeepQuotesShouldBoundNestingAndRetainRemainingText() {
        let source = String(repeating: ">", count: 3_000) + " text"
        var blocks = ChatMarkdown.parse(source)
        var depth = 0
        while case .quote(let nested) = blocks.first {
            depth += 1
            blocks = nested
        }
        XCTAssertEqual(depth, 32)
        XCTAssertEqual(blocks, [.paragraph(String(repeating: ">", count: 2_968) + " text")])
    }
    func testGivenTableCodeSpanShouldMatchDelimiterLength() {
        XCTAssertEqual(ChatMarkdown.parse("A | B\n--- | ---\n``x|y`` | end"), [
            .table(headers: ["A", "B"], alignments: [.leading, .leading], rows: [["``x|y``", "end"]])])
        XCTAssertEqual(ChatMarkdown.cells("``x`|y`` | end"), ["``x`|y``", "end"])
        XCTAssertEqual(ChatMarkdown.cells("`unfinished | end"), ["`unfinished", "end"])
    }

    func testGivenCancelledParseShouldDiscardObsoleteReply() async {
        let result = await Task.detached {
            withUnsafeCurrentTask { $0?.cancel() }
            return ChatMarkdown.parse(String(repeating: "Long obsolete reply\n", count: 10_000))
        }.value
        XCTAssertTrue(result.isEmpty)
    }

}
