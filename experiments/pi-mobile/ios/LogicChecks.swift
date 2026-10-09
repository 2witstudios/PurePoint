import Foundation
@main struct LogicChecks {
    static func main() {
        var draft = DraftState(text: "First")
        let submitted = draft.takeSubmission(); draft.text = "Later"; draft.restore(submitted)
        precondition(draft.text == "Later\n\nFirst")
        var cursor = SnapshotCursor(); precondition(cursor.accept(epoch: "a", revision: 2)); precondition(!cursor.accept(epoch: "a", revision: 1)); precondition(cursor.accept(epoch: "b", revision: 1))
        precondition(MarkdownBlocks.split("Prose\n```swift\nlet x = 1\n```")[1] == .code(language: "swift", text: "let x = 1"))
        precondition(ConnectionAddress.url("ws://100.100.1.2:8787/v1") != nil)
        precondition(ConnectionAddress.url("ws://0.0.0.0/v1") == nil)
        precondition(ConnectionAddress.url("ws://example.com/v1") == nil)
        let pairing = "{\"type\":\"pi-mobile-pairing\",\"version\":1,\"endpoint\":\"ws://100.94.14.74:8787/v1\",\"secret\":\"fixture-pairing-secret-0123456789abcdef\"}"
        precondition(PairingCode.parse(pairing)?.endpoint == "ws://100.94.14.74:8787/v1")
        precondition(PairingCode.parse(pairing.replacingOccurrences(of: "100.94.14.74", with: "example.com")) == nil)
        precondition(PairingCode.parse(pairing.replacingOccurrences(of: "\"version\":1", with: "\"version\":2")) == nil)
        precondition(PairingCode.parse(pairing.replacingOccurrences(of: "fixture-pairing-secret-0123456789abcdef", with: "short")) == nil)
        precondition(PairingCode.parse("not a pairing code") == nil)
        print("Pi Mobile standalone domain checks passed")
    }
}
