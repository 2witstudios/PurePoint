import Foundation

// Exercise preferences through UserDefaults lookup while keeping every write volatile.
final class RecoveryDefaults: UserDefaults, @unchecked Sendable {
    override func set(_ value: Any?, forKey key: String) {
        var values = volatileDomain(forName: UserDefaults.argumentDomain)
        values[key] = value
        setVolatileDomain(values, forName: UserDefaults.argumentDomain)
    }
    override func removeObject(forKey key: String) { set(nil, forKey: key) }
}

// Run verification/check-recovery.sh to compile the actual model with private
// implementation access, volatile defaults and isolated temporary recovery files.
extension ChatModel {
    func hydrateForChecks() async { await recoveryTask?.value }
    func flushForChecks() async { await withCheckedContinuation { continuation in writer.flush { continuation.resume() } } }
    func applyForChecks(_ state: Snapshot) throws { try apply(state) }
    func acknowledgeForChecks(_ id: String) { updateSubmission(id, status: "Accepted", recoverable: false) }
    func uncertainForChecks(_ id: String) { updateSubmission(id, status: "Delivery uncertain", recoverable: true) }
    var hasNoTransmissionForChecks: Bool { socket == nil && pending.isEmpty }
}

@main struct RecoveryChecks {
    @MainActor static func main() async throws {
        let home = ProcessInfo.processInfo.environment["CFFIXED_USER_HOME"] ?? ""
        precondition(home.hasPrefix("/tmp/pi-mobile-recovery-") && FileManager.default.homeDirectoryForCurrentUser.path == home, "Use an isolated temporary home; never run against owner storage")
        let support = try FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask, appropriateFor: nil, create: true)
        precondition(support.path.hasPrefix(home + "/"), "Recovery storage must remain in the isolated home")
        let address = "ws://127.0.0.1:8787/v1"
        let defaults = RecoveryDefaults(suiteName: "pi-mobile-recovery-" + UUID().uuidString)!
        defaults.set(address, forKey: "pi.endpoint")
        let file = ComposerAttachment(id: "attachment", name: "note.txt", mimeType: "text/plain", data: Data("Original file".utf8))
        let state = Snapshot(version: 1, epoch: "session-epoch", revision: 1, busy: false, sessionId: "native-session", title: "Test", messages: [], tools: [], queue: [], dialogs: [], notices: [])
        let model = ChatModel(defaults: defaults)
        await model.hydrateForChecks()
        model.addAttachment(file)
        try model.applyForChecks(state)
        precondition(model.attachments.map(\.id) == [file.id], "Initial session retains the local draft")
        model.attachments = []
        await model.flushForChecks()
        let drafts = LocalRecoveryStore.load("attachment-drafts", as: [String: [ComposerAttachment]].self) ?? [:]
        precondition(drafts[address + ":local"] == nil, "Migrating a draft clears the persisted origin")
        let restarted = ChatModel(defaults: defaults)
        await restarted.hydrateForChecks()
        try restarted.applyForChecks(state)
        precondition(restarted.attachments.isEmpty, "Removing migrated files must survive restart and another first snapshot")
        await restarted.flushForChecks()

        let uncertain = Submission(id: "uncertain", text: "Keep original text", sessionId: state.sessionId, status: "Delivery uncertain", recoverable: true, attachments: [file])
        let sending = Submission(id: "sending", text: "In flight", sessionId: state.sessionId, status: "Sending", recoverable: false, attachments: [file])
        let accepted = (0..<70).map { Submission(id: "accepted-\($0)", text: "Accepted", sessionId: state.sessionId, status: "Accepted", recoverable: false, attachments: [file]) }
        try LocalRecoveryStore.save([uncertain, sending] + accepted, name: "submissions")
        let hydrated = ChatModel(defaults: defaults)
        await hydrated.hydrateForChecks()
        precondition(hydrated.submissions.count == 50, "Hydration bounds live receipts")
        precondition(hydrated.submissions.filter { !$0.recoverable }.allSatisfy { ($0.attachments ?? []).isEmpty }, "Accepted receipts release original file bytes")
        precondition(hydrated.submissions.first { $0.id == uncertain.id }?.attachments?.first?.data == file.data, "Older uncertain recovery outranks settled receipts")
        precondition(hydrated.submissions.first { $0.id == sending.id }?.recoverable == true, "Interrupted sends remain recoverable")
        precondition(hydrated.hasNoTransmissionForChecks, "Hydration never automatically resends")
        hydrated.submissions.append(Submission(id: "live", text: "Live", sessionId: state.sessionId, status: "Sending", recoverable: false, attachments: [file]))
        precondition(hydrated.submissions.count == 50 && hydrated.submissions.last?.attachments?.first?.data == file.data, "Live sends retain files within the bound")
        hydrated.uncertainForChecks("live")
        precondition(hydrated.submissions.last?.recoverable == true && hydrated.submissions.last?.attachments?.first?.data == file.data, "Status changes retain uncertain files atomically")
        hydrated.acknowledgeForChecks("live")
        precondition(hydrated.submissions.last?.attachments == nil, "Acceptance releases file payloads")
        await hydrated.flushForChecks()
        let persisted = LocalRecoveryStore.load("submissions", as: [Submission].self) ?? []
        precondition(persisted.count == 50 && persisted.last?.attachments == nil, "Disk and live retention agree")
        hydrated.submissions = (0..<50).map { Submission(id: "recovery-\($0)", text: "Uncertain", sessionId: state.sessionId, status: "Delivery uncertain", recoverable: true, attachments: [file]) }
        try hydrated.applyForChecks(state)
        hydrated.connected = true
        hydrated.draft = "Keep this draft"
        hydrated.addAttachment(file)
        hydrated.submit(mode: "send")
        precondition(hydrated.submissions.count == 50 && hydrated.draft == "Keep this draft" && hydrated.attachments.count == 1, "Saturated recovery refuses a send without losing receipts or draft files")
        precondition(hydrated.error?.contains("recovery") == true, "Saturated recovery gives actionable guidance")
        hydrated.submissions[0] = sending
        let overflow = Snapshot(version: 1, epoch: state.epoch, revision: 2, busy: false, sessionId: state.sessionId, title: state.title, messages: [], tools: [], queue: [], dialogs: [], notices: [], canceled: [CanceledText(id: "canceled", text: "Stopped queued text", sessionId: state.sessionId)])
        try hydrated.applyForChecks(overflow)
        precondition(hydrated.submissions.count == 50 && hydrated.submissions.first { $0.id == sending.id }?.attachments?.first?.data == file.data, "Cancellation overflow remains bounded and protects in-flight files")
        precondition(hydrated.error?.contains("Older recovery receipts") == true, "Unavoidable cancellation overflow warns about omitted recovery")
        await hydrated.flushForChecks()
        print("Recovery checks passed: draft migration/restart, live/hydrated retention, receipt updates, persistence and saturation.")
    }
}
