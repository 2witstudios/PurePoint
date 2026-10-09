import Foundation

// Compiled in the actual model's file to exercise its transport without writing Keychain.
extension PiChatModel {
    func connectFixture(_ url: URL, secret: String) async {
        await recoveryTask?.value
        endpoint = url.absoluteString
        startConnection(url: url, secret: secret)
    }
    func flushFixture() async {
        await withCheckedContinuation { continuation in writer.flush { continuation.resume() } }
    }
}

extension ChatModel {
    func connectFixture(_ url: URL, secret: String) async {
        await recoveryTask?.value
        endpoint = url.absoluteString
        startConnection(url: url, secret: secret)
    }
    func flushFixture() async {
        await withCheckedContinuation { continuation in writer.flush { continuation.resume() } }
    }
}

@main struct PiChatChecks {
    @MainActor static func wait(_ description: String, until condition: () -> Bool) async throws {
        for _ in 0..<500 {
            if condition() { return }
            try await Task.sleep(for: .milliseconds(20))
        }
        fatalError("Timed out: " + description)
    }
    @MainActor static func main() async throws {
        let env = ProcessInfo.processInfo.environment
        let home = env["CFFIXED_USER_HOME"]!
        precondition(
            home.hasPrefix("/tmp/pointguard-checks-") && FileManager.default.homeDirectoryForCurrentUser.path == home)
        let defaults = UserDefaults(suiteName: "pointguard-fixture-" + UUID().uuidString)!
        let model = PiChatModel(defaults: defaults)
        let phone = ChatModel(defaults: UserDefaults(suiteName: "phone-fixture-" + UUID().uuidString)!)
        precondition(model.clientId != phone.clientId)
        let url = URL(string: "ws://127.0.0.1:\(env["POINTGUARD_FIXTURE_PORT"]!)/v1")!
        await model.connectFixture(url, secret: env["POINTGUARD_FIXTURE_TOKEN"]!)
        try await wait("initial authoritative sync") { model.connected }
        await phone.connectFixture(url, secret: env["POINTGUARD_FIXTURE_TOKEN"]!)
        try await wait("both connected") { phone.connected && model.connected }
        await model.loadConversations()
        precondition(model.conversations.first?.id == "fixture-history")
        model.draft = "Build Point Guard"
        model.submit(mode: "send")
        model.draft = "Later typing stays intact"
        try await wait("accepted completed response") {
            model.submissions.first?.status == "Accepted" && !model.busy
                && model.snapshot?.messages.contains { $0.role == "assistant" && $0.text.contains("```swift") } == true
        }
        try await wait("phone sees the same rich reply") { phone.snapshot?.revision == model.snapshot?.revision }
        precondition(phone.snapshot!.messages.map(\.text) == model.snapshot!.messages.map(\.text))
        precondition(phone.snapshot!.tools.map(\.text) == model.snapshot!.tools.map(\.text))
        precondition(model.draft == "Later typing stays intact" && phone.draft.isEmpty)
        precondition(
            model.transcriptRows.contains {
                if case .activity = $0 { return true }; return false
            })
        let userCount = model.snapshot!.messages.filter { $0.role == "user" }.count
        model.disconnect()
        await model.connectFixture(url, secret: env["POINTGUARD_FIXTURE_TOKEN"]!)
        try await wait("reconnect") { model.connected }
        precondition(
            model.snapshot!.messages.filter { $0.role == "user" }.count == userCount,
            "Reconnect must not replay messages")
        model.draft = "/fixture-slow"
        model.submit(mode: "send")
        try await wait("slow run starts") { model.busy && model.submissions.last?.status == "Accepted" }
        model.draft = "Queued follow-up"
        model.submit(mode: "after")
        try await wait("queued receipt") { model.submissions.last?.status == "Queued" }
        try await wait("phone sees running Pi") { phone.busy }
        phone.draft = "Phone follow-up"
        phone.submit(mode: "after")
        try await wait("phone queue accepted") { phone.submissions.last?.status == "Queued" }
        let stopped = await phone.stop()
        precondition(stopped)
        try await wait("queue recovery after Stop") {
            !model.busy && model.recoverable.contains { $0.text == "Queued follow-up" }
        }
        try await wait("phone recovers its own message") { phone.recoverable.contains { $0.text == "Phone follow-up" } }
        precondition(!model.recoverable.contains { $0.text == "Phone follow-up" })
        precondition(!phone.recoverable.contains { $0.text == "Queued follow-up" })
        let recovery = model.recoverable.first { $0.text == "Queued follow-up" }!
        model.draft = "Newer draft"
        model.restore(recovery)
        precondition(model.draft == "Newer draft\n\nQueued follow-up")
        model.draft = "/fixture-select"
        model.submit(mode: "send")
        try await wait("native extension dialog") { model.snapshot?.dialogs.first?.method == "select" }
        let dialog = model.snapshot!.dialogs.first!
        await model.answer(dialog, optionId: dialog.optionIds!.first!)
        try await wait("dialog settles") { !model.busy && model.snapshot?.dialogs.isEmpty == true }
        await model.browse(model.conversations[0])
        precondition(model.browsing?.messages.first?.text == "History is read-only.")
        model.draft = "Draft for the live conversation"
        precondition(!model.canSend, "Browsing history must not send into the live conversation")
        let submissionsBeforeBrowseSend = model.submissions.count
        model.submit(mode: "send")
        precondition(model.submissions.count == submissionsBeforeBrowseSend)
        await model.changeSession(to: "fixture-history")
        precondition(model.snapshot?.sessionId == "fixture-history" && model.browsing == nil)
        await model.changeSession()
        precondition(model.snapshot?.messages.isEmpty == true)
        try await wait("both follow the same session change") { phone.snapshot?.epoch == model.snapshot?.epoch }
        precondition(phone.snapshot?.messages.isEmpty == true)
        model.disconnect()
        precondition(phone.connected, "Disconnecting desktop leaves the phone connected")
        phone.disconnect()
        await phone.flushFixture()
        await model.flushFixture()
        print(
            "Point Guard simultaneous phone/desktop checks passed: shared state, local drafts, owner-specific cancellation recovery, send, rich responses, tools, typing, reconnect without replay, queue/Stop recovery, extension selection, history, resume and new conversation."
        )
    }
}
