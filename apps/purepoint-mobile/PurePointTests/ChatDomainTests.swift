import XCTest
@testable import PurePoint
final class ChatDomainTests: XCTestCase {
    @MainActor func testGivenExplicitDisconnectShouldStayDisconnectedOnForegroundWithSavedEndpoint() {
        let model = ChatModel()
        model.endpoint = "ws://100.100.1.2:8787/v1"
        model.disconnect()
        model.setForeground(false)
        model.setForeground(true)
        XCTAssertFalse(model.connected)
        XCTAssertFalse(model.connectionStatus.contains("Connecting"))
    }

    @MainActor func testGivenConnectionIntentShouldReconnectOnForeground() {
        let model = ChatModel()
        model.endpoint = "ws://100.100.1.2:8787/v1"
        model.connect()
        model.setForeground(false)
        model.setForeground(true)
        XCTAssertEqual(model.connectionStatus, "Connecting to your Mac…")
        model.disconnect()
    }

    func testGivenAttachedFilesShouldComposeNativeImagesAndReadableTextWithinBudgets() throws {
        let file = ComposerAttachment(id: "file", name: "notes.txt", mimeType: "text/plain", data: Data("Project notes".utf8))
        let photo = ComposerAttachment(id: "photo", name: "photo.jpg", mimeType: "image/jpeg", data: Data([255,216,255,1]))
        let content = try ComposerPayload.make(text: "Read these", attachments: [file,photo])
        XCTAssertTrue(content.message.contains("Project notes"))
        XCTAssertTrue(content.message.contains("notes.txt"))
        XCTAssertEqual(content.images.count, 1)
        XCTAssertEqual(content.images[0]["data"], photo.data.base64EncodedString())
        XCTAssertThrowsError(try ComposerPayload.make(text: "", attachments: Array(repeating: photo, count: 5)))
        XCTAssertThrowsError(try ComposerPayload.make(text: String(repeating: "x", count: 65537), attachments: []))
        let saved = Submission(id: "one", text: "Draft", sessionId: "session", status: "Delivery uncertain", recoverable: true, attachments: [photo])
        let recovered = try JSONDecoder().decode(Submission.self, from: JSONEncoder().encode(saved))
        XCTAssertEqual(recovered.attachments?.first?.data, photo.data)
    }
    func testGivenNativeAndLiveToolsShouldGroupOnceWithoutBlankAssistantRows() {
        let messages = [
            ChatMessage(id: "toolResult-1-call-a", role: "toolResult", text: "Read output", activity: "read"),
            ChatMessage(id: "assistant-2", role: "assistant", text: " "),
            ChatMessage(id: "toolResult-3-call-b", role: "toolResult", text: "Shell output", activity: "bash")
        ]
        let live = [ToolActivity(id: "call-a", name: "read", state: "finished", text: "Read output"), ToolActivity(id: "call-b", name: "bash", state: "finished", text: "Shell output"), ToolActivity(id: "call-c", name: "read", state: "running", text: "Working")]
        let rows = TranscriptRows.make(messages: messages, tools: live)
        XCTAssertEqual(rows.count, 1)
        guard let first = rows.first, case .activity(let tools) = first else { return XCTFail("Expected one activity group") }
        XCTAssertEqual(tools.map(\.id), ["call-a", "call-b", "call-c"])
        XCTAssertEqual(tools.last?.state, "running")
    }
    func testGivenProseBetweenToolsShouldPreserveConversationOrderAndErrors() {
        let rows = TranscriptRows.make(messages: [
            ChatMessage(id: "first", role: "toolResult", text: "Output", activity: "read"),
            ChatMessage(id: "explanation", role: "assistant", text: "What I found"),
            ChatMessage(id: "second", role: "toolResult", text: "", activity: "bash", error: "Permission denied"),
            ChatMessage(id: "error", role: "assistant", text: "", error: "Provider failed")
        ], tools: [])
        XCTAssertEqual(rows.count, 4)
        guard case .activity(let failed) = rows[2] else { return XCTFail("Expected tool group after prose") }
        XCTAssertEqual(failed[0].state, "failed")
        XCTAssertTrue(failed[0].text.contains("Permission denied"))
        XCTAssertEqual(rows[3].id, "message:error")
    }
    func testGivenPairingQRShouldAcceptNativePayloadAndRejectUnsafeOrUnsupportedCodes() {
        let payload = "{\"type\":\"pi-mobile-pairing\",\"version\":1,\"endpoint\":\"ws://100.94.14.74:8787/v1\",\"secret\":\"fixture-pairing-secret-0123456789abcdef\"}"
        XCTAssertEqual(PairingCode.parse(payload)?.endpoint, "ws://100.94.14.74:8787/v1")
        XCTAssertNil(PairingCode.parse(payload.replacingOccurrences(of: "100.94.14.74", with: "example.com")))
        XCTAssertNil(PairingCode.parse(payload.replacingOccurrences(of: "\"version\":1", with: "\"version\":2")))
        XCTAssertNil(PairingCode.parse(payload.replacingOccurrences(of: "fixture-pairing-secret-0123456789abcdef", with: "short")))
        XCTAssertNil(PairingCode.parse(String(repeating: "x", count: 8193)))
        XCTAssertNil(PairingCode.parse("https://example.com"))
    }
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
    func testGivenNumericLookingPublicHostnameShouldRejectManualAndQRPairing() {
        for host in ["100.64.1.2.attacker.example", "100.attacker.64.1.2", "100..64.1.2", "100.064.1.2", "100.63.1.2", "100.128.1.2", "100.64.1.256"] {
            XCTAssertNil(ConnectionAddress.url("ws://" + host + "/v1"))
            let qr = "{\"type\":\"pi-mobile-pairing\",\"version\":1,\"endpoint\":\"ws://" + host + "/v1\",\"secret\":\"fixture-pairing-secret-0123456789abcdef\"}"
            XCTAssertNil(PairingCode.parse(qr))
        }
        XCTAssertNotNil(ConnectionAddress.url("ws://100.64.0.1/v1"))
        XCTAssertNotNil(ConnectionAddress.url("ws://100.127.255.254/v1"))
    }
    func testGivenUnsafeEndpointShouldRejectPublicAndWildcardHosts() {
        XCTAssertNotNil(ConnectionAddress.url("ws://100.100.1.2:8787/v1"))
        XCTAssertNotNil(ConnectionAddress.url("wss://mac.example.ts.net/v1"))
        XCTAssertNil(ConnectionAddress.url("ws://0.0.0.0:8787/v1"))
        XCTAssertNil(ConnectionAddress.url("ws://example.com/v1"))
    }
}
