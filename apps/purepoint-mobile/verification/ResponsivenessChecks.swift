import Foundation

@main struct ResponsivenessChecks {
    static func main() async throws {
        let queue = DispatchQueue(label: "pi.test.persistence")
        let gate = DispatchSemaphore(value: 0)
        queue.async { gate.wait() }
        let writer = CoalescedWriter(queue: queue, delay: 0.01)
        let completed = DispatchSemaphore(value: 0)
        let file = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: file) }
        for value in 0..<100 {
            writer.schedule(key: "draft") {
                precondition(!Thread.isMainThread, "Persistence must not block UI")
                try! Data(String(value).utf8).write(to: file)
            }
        }
        writer.flush { completed.signal() }
        gate.signal()
        precondition(completed.wait(timeout: .now() + 3) == .success)
        let saved = try String(contentsOf: file, encoding: .utf8)
        precondition(saved == "99", "Latest draft must win")
        let snapshot = Snapshot(version: 1, epoch: "test", revision: 1, busy: true, sessionId: "session", title: "Test", messages: [ChatMessage(id: "a", role: "assistant", text: "Partial")], tools: [], queue: [], dialogs: [], notices: [])
        var object = try JSONSerialization.jsonObject(with: JSONEncoder().encode(snapshot)) as! [String: Any]
        object["type"] = "snapshot"
        guard case .snapshot(let decoded, let rows) = try IncomingRecord.decode(JSONSerialization.data(withJSONObject: object)) else { fatalError("Expected snapshot") }
        precondition(decoded.messages.first?.text == "Partial" && rows.count == 1)
        guard case .envelope(let receipt) = try IncomingRecord.decode(Data(#"{"type":"receipt","id":"one","ok":true,"data":{"disposition":"accepted"}}"#.utf8)) else { fatalError("Expected receipt") }
        precondition(receipt.data?["disposition"].text == "accepted")
        let rich = MarkdownBlocks.render("A **bold** thought\n\n```swift\nlet value = 1\n```")
        precondition(rich.count == 2)
        guard case .prose(let prose) = rich[0], case .code(let language, let code) = rich[1] else { fatalError("Expected prose then code") }
        precondition(String(prose.characters).contains("bold") && language == "swift" && code == "let value = 1")
        let secondFile = FileManager.default.temporaryDirectory.appendingPathComponent(UUID().uuidString)
        defer { try? FileManager.default.removeItem(at: secondFile) }
        let secondDone = DispatchSemaphore(value: 0)
        writer.schedule(key: "draft") { try! Data("newer".utf8).write(to: file) }
        writer.schedule(key: "attachments") { try! Data("independent".utf8).write(to: secondFile) }
        writer.flush { secondDone.signal() }
        precondition(secondDone.wait(timeout: .now() + 3) == .success)
        let latest = try String(contentsOf: file, encoding: .utf8)
        let independent = try String(contentsOf: secondFile, encoding: .utf8)
        precondition(latest == "newer" && independent == "independent")
        print("Responsiveness checks passed: off-main persistence, latest-write ordering, typed snapshot/receipt decoding.")
    }
}
