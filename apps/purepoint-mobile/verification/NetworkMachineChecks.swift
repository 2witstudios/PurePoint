import Foundation

extension ChatModel {
    var networkPendingForChecks: Int { pending.count }
    func networkLossForChecks() { connectionLost("Fixture unavailable") }
    var retriesForChecks: Int { retryBudget.attempts }
}
final class DelayedTrustRead: @unchecked Sendable {
    let vault: MachineTrust
    let endpoint: String
    private let lock = NSLock()
    private var delayed = false
    let started = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    init(vault: MachineTrust, endpoint: String) { self.vault = vault; self.endpoint = endpoint }
    func waitForStart() -> Bool { started.wait(timeout: .now() + 10) == .success }
    func read(_ address: String) -> TrustedHost? {
        lock.lock(); let delay = address == endpoint && !delayed; if delay { delayed = true }; lock.unlock()
        if delay { started.signal(); precondition(release.wait(timeout: .now() + 10) == .success) }
        return vault.read(address)
    }
}
@main struct NetworkMachineChecks {
    @MainActor static func wait(_ condition: () -> Bool) async throws {
        for _ in 0..<500 { if condition() { return }; try await Task.sleep(nanoseconds: 10_000_000) }
        preconditionFailure("Network fixture timed out")
    }
    @MainActor static func main() async throws {
        let directory = CommandLine.arguments[1]
        precondition(directory.hasPrefix("/tmp/pg-machine-network-"))
        func file(_ name: String) -> URL { URL(fileURLWithPath: directory + "/" + name) }
        func mark(_ name: String) throws { try Data("yes".utf8).write(to: file(name)) }
        func exists(_ name: String) -> Bool { FileManager.default.fileExists(atPath: file(name).path) }
        let a = try JSONDecoder().decode(TrustedHost.self, from: Data(contentsOf: file("A.json")))
        let b = try JSONDecoder().decode(TrustedHost.self, from: Data(contentsOf: file("B.json")))
        let vault = MachineTrust(); vault.save(a); vault.save(b)
        let defaults = MachineDefaults(suiteName: UUID().uuidString)!
        defaults.set(a.endpoint, forKey: "pi.endpoint")
        let model = ChatModel(defaults: defaults, trustLoader: { vault.read($0) }, trustSaver: { vault.save($0) }, trustRemover: { vault.remove($0) })
        model.connect(); try await wait { model.connected }
        let aToken = model.selectionToken
        let aKey = model.machineKey
        let dialogA = model.snapshot!.dialogs[0]
        model.draft = "A draft"; model.addAttachment(ComposerAttachment(id: "A", name: "A.txt", mimeType: "text/plain", data: Data("A".utf8)))
        model.pairedForChecks(b); try await wait { model.connected && model.snapshot?.title == "B" }
        let bKey = model.machineKey
        model.draft = "B draft"
        model.selectMachine(a.hostId); try await wait { model.connected && model.snapshot?.title == "A" }
        precondition(model.draft == "A draft" && model.attachments.map(\.id) == ["A"] && model.submissions.filter { $0.text == "A canceled" }.count == 1, "Given A/B/A over real pinned WSS, should retain drafts/files/canceled receipts without duplicates")
        for op in ["sessions", "history", "send", "answer"] { try mark("hold-A-" + op) }
        let history = Task { await model.browse(Conversation(id: "A-session", title: "A", date: nil)) }
        let sessions = Task { await model.loadConversations() }
        let answer = Task { await model.answer(model.snapshot!.dialogs[0], confirmed: true) }
        model.draft = "A submitted"; model.submit(mode: "send")
        try await wait { ["sessions", "history", "send", "answer"].allSatisfy { exists("started-A-" + $0) } }
        model.selectMachine(b.hostId); try await wait { model.connected && model.snapshot?.title == "B" }
        precondition(model.networkPendingForChecks == 0 && model.draft == "B draft" && model.attachments.isEmpty && model.submissions.allSatisfy { $0.text != "A submitted" }, "Given A in-flight work during switch, should drain pending callbacks without exposing A state")
        for op in ["sessions", "history", "send", "answer"] { try mark("release-A-" + op) }
        try mark("broadcast-A"); await history.value; await sessions.value; await answer.value
        try await Task.sleep(nanoseconds: 80_000_000)
        precondition(model.snapshot?.title == "B" && model.conversations.isEmpty && model.browsing == nil && model.editorOffer == nil && model.error == nil, "Given old receipts/history/editor/errors after switching, should leave B untouched")
        let stopped = await model.stop(selection: aToken)
        precondition(!stopped, "Given queued A Stop after switching to B, should reject old selection before sending")
        await model.answer(dialogA, confirmed: true, selection: aToken)
        precondition(model.error == nil && model.networkPendingForChecks == 0, "Given reused dialog IDs on B, should ignore old A dialog provenance")
        model.selectMachine(a.hostId); try await wait { model.connected && model.snapshot?.title == "A" }
        precondition(model.submissions.contains { $0.text == "A submitted" && $0.recoverable }, "Given canceled transport acknowledgement, should retain uncertain A submission only on A")
        model.selectMachine(b.hostId); try await wait { model.connected }
        let used = PairingCode.parse(try String(contentsOf: file("B-used.json"), encoding: .utf8))!
        model.selectMachine(a.hostId); try await wait { model.connected }
        model.pair(used); try await wait { !model.pairing }
        precondition(model.connected && model.selectedMachine?.id == a.hostId && model.pairingError != nil, "Given actual rejected B enrollment, should keep A live")
        let qr = PairingCode.parse(try String(contentsOf: file("B-qr.json"), encoding: .utf8))!
        let wrongPin = PairingCode(type: qr.type, version: qr.version, endpoint: qr.endpoint, hostId: qr.hostId, certificateSHA256: String(repeating: "0", count: 64), enrollmentToken: qr.enrollmentToken, expiresAt: qr.expiresAt)
        model.pair(wrongPin); try await wait { !model.pairing }
        precondition(model.connected && model.selectedMachine?.id == a.hostId && model.pairingError?.contains("certificate") == true, "Given actual B certificate rejection, should preserve A and refuse enrollment")
        try mark("hold-B-enrollment"); model.pair(qr); try await wait { exists("started-B-enrollment") }
        model.cancelPairing(); try mark("release-B-enrollment")
        try await Task.sleep(nanoseconds: 80_000_000)
        precondition(model.connected && model.selectedMachine?.id == a.hostId && vault.read(b.endpoint)?.clientId == b.clientId, "Given canceled remote enrollment, should retain existing B trust and A selection")
        model.networkLossForChecks(); precondition(model.retriesForChecks == 1 && model.selectedMachine?.id == a.hostId)
        model.selectMachine(b.hostId); try await wait { model.connected }
        try await Task.sleep(nanoseconds: 1_100_000_000)
        precondition(model.selectedMachine?.id == b.hostId && model.snapshot?.title == "B", "Given old A reconnect timer after switch, should reconnect only selected B")
        model.disconnect(); model.setForeground(false); model.setForeground(true); model.networkForChecks(true)
        precondition(!model.intentForChecks && model.noTransportForChecks && model.selectedMachine?.id == b.hostId, "Given explicit disconnect, should prevent network/foreground reconnect")
        // Delayed old Keychain lookup must not replace the newer selected connection/principal.
        model.selectMachine(a.hostId); model.disconnect()
        let delayedRead = DelayedTrustRead(vault: vault, endpoint: a.endpoint)
        let delayedModel = ChatModel(defaults: defaults, trustLoader: { delayedRead.read($0) })
        delayedModel.connect()
        let readStarted = await Task.detached { delayedRead.waitForStart() }.value; precondition(readStarted)
        delayedModel.selectMachine(b.hostId); try await wait { delayedModel.connected && delayedModel.snapshot?.title == "B" }
        delayedRead.release.signal(); try await Task.sleep(nanoseconds: 80_000_000)
        precondition(delayedModel.selectedMachine?.id == b.hostId && delayedModel.clientId == b.clientId && delayedModel.snapshot?.title == "B", "Given delayed A Keychain read after selection, should keep B principal and transport")
        delayedModel.disconnect()

        let requests = try JSONDecoder().decode([[String: JSONValue]].self, from: Data(contentsOf: file("requests.json")))
        precondition(requests.filter { $0["op"]?.text == "stop" }.isEmpty, "Given switching, should never Stop either Mac")
        precondition(requests.filter { $0["op"]?.text == "send" }.count == 1 && requests.first { $0["op"]?.text == "send" }?["machine"]?.text == "A", "Given switching/reconnect/recovery, should send exactly once to A and never replay on B")
        precondition(aKey != bKey)
        print("Network machine checks passed: two real pinned hosts; A/B/back; in-flight receipts/history/dialogs/editor; enrollment failure/cancel/pin; reconnect selection; explicit disconnect; zero Stop; one A send and no replay.")
    }
}
