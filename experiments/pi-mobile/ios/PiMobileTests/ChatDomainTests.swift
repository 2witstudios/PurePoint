import XCTest
@testable import PiMobile
final class ChatDomainTests: XCTestCase {
    func testGivenLaterTypingShouldPreserveItWhenRestoringSubmission() {
        var draft = DraftState(text: "First instruction")
        let submitted = draft.takeSubmission()
        draft.text = "A later thought"
        draft.restore(submitted)
        XCTAssertEqual(draft.text, "A later thought\n\nFirst instruction")
    }
    func testGivenRepeatedSnapshotShouldRejectOlderRevision() {
        var cursor = SnapshotCursor()
        XCTAssertTrue(cursor.accept(epoch: "one", revision: 5))
        XCTAssertFalse(cursor.accept(epoch: "one", revision: 4))
        XCTAssertFalse(cursor.accept(epoch: "one", revision: 5))
        XCTAssertTrue(cursor.accept(epoch: "two", revision: 1))
    }
    func testGivenFencedCodeShouldKeepCodeSeparateAndCopyable() {
        let blocks = MarkdownBlocks.split("Hello\n```swift\nlet x = 1\n```\nGoodbye")
        XCTAssertEqual(blocks.count, 3)
        XCTAssertEqual(blocks[1], .code(language: "swift", text: "let x = 1"))
        XCTAssertEqual(MarkdownBlocks.split("```\npartial").last, .code(language: "", text: "partial"))
    }
    func testGivenUnsafeEndpointShouldRejectPublicAndWildcardHosts() {
        XCTAssertNotNil(ConnectionAddress.url("ws://100.100.1.2:8787/v1"))
        XCTAssertNotNil(ConnectionAddress.url("wss://mac.example.ts.net/v1"))
        XCTAssertNil(ConnectionAddress.url("ws://0.0.0.0:8787/v1"))
        XCTAssertNil(ConnectionAddress.url("ws://example.com/v1"))
    }
}
