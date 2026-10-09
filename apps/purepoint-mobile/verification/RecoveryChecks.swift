import Foundation

// Exercise preferences through UserDefaults lookup while keeping every write volatile.
final class RecoveryDefaults: UserDefaults, @unchecked Sendable {
    private let writeLock = NSLock()
    private var canceledWrites = 0
    var canceledWriteCount: Int { writeLock.lock(); defer { writeLock.unlock() }; return canceledWrites }
    override func set(_ value: Any?, forKey key: String) {
        if key == "pi.canceledIds" { writeLock.lock(); canceledWrites += 1; writeLock.unlock() }
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
        try LocalRecoveryStore.save([Submission](), name: "submissions")
        let originalIds = (0..<1000).map { "cancel-\($0)" }
        defaults.set(originalIds, forKey: "pi.canceledIds")
        let cancellationModel = ChatModel(defaults: defaults)
        await cancellationModel.hydrateForChecks()
        func canceledState(_ revision: Int, _ ids: [String]) -> Snapshot {
            Snapshot(version: 1, epoch: "cancellation-epoch", revision: revision, busy: false, sessionId: state.sessionId, title: state.title, messages: [], tools: [], queue: [], dialogs: [], notices: [], canceled: ids.map { CanceledText(id: $0, text: "Stopped", sessionId: state.sessionId) })
        }
        let latestIds = ["cancel-999", "cancel-1000", "cancel-1000"]
        try cancellationModel.applyForChecks(canceledState(1, latestIds))
        precondition(cancellationModel.submissions.map(\.id) == ["cancel-1000"], "Known and duplicate cancellation IDs create no duplicate recovery")
        cancellationModel.dismissSubmission("cancel-1000")
        await cancellationModel.flushForChecks()
        precondition(defaults.stringArray(forKey: "pi.canceledIds") == (1...1000).map { "cancel-\($0)" }, "Saturated cancellation history persists its changed content in chronological order")
        let cancellationRestart = ChatModel(defaults: defaults)
        await cancellationRestart.hydrateForChecks()
        try cancellationRestart.applyForChecks(canceledState(1, latestIds))
        precondition(cancellationRestart.submissions.isEmpty, "Dismissed cancellation stays dismissed after restart and repeated snapshot")
        let newerIds = (1001...1100).map { "cancel-\($0)" }
        try cancellationRestart.applyForChecks(canceledState(2, newerIds))
        cancellationRestart.submissions = []
        let newestIds = (1101...1200).map { "cancel-\($0)" }
        try cancellationRestart.applyForChecks(canceledState(3, newestIds))
        cancellationRestart.submissions = []
        try cancellationRestart.applyForChecks(canceledState(4, newestIds + newestIds))
        precondition(cancellationRestart.submissions.isEmpty, "Overcapacity updates retain current snapshot IDs without duplicate re-adds")
        await cancellationRestart.flushForChecks()
        precondition(defaults.stringArray(forKey: "pi.canceledIds") == (201...1200).map { "cancel-\($0)" }, "Multiple saturated updates retain exactly the newest 1000 IDs")
        defaults.set(originalIds + ["cancel-500", "cancel-1000", "cancel-1000"], forKey: "pi.canceledIds")
        let legacyRestart = ChatModel(defaults: defaults)
        await legacyRestart.hydrateForChecks()
        await legacyRestart.flushForChecks()
        let normalized = defaults.stringArray(forKey: "pi.canceledIds") ?? []
        precondition(normalized.count == 1000 && Set(normalized).count == 1000 && normalized.suffix(2) == ["cancel-500", "cancel-1000"], "Legacy arrays normalize duplicates and size while preserving available recent order")
        try legacyRestart.applyForChecks(canceledState(1, ["cancel-600"]))
        await legacyRestart.flushForChecks()
        precondition(defaults.stringArray(forKey: "pi.canceledIds") == normalized.filter { $0 != "cancel-600" } + ["cancel-600"], "Changed recency order persists even when membership and count stay the same")
        let writes = defaults.canceledWriteCount
        try legacyRestart.applyForChecks(canceledState(2, ["cancel-600", "cancel-600"]))
        await legacyRestart.flushForChecks()
        precondition(defaults.canceledWriteCount == writes && legacyRestart.submissions.isEmpty, "Unchanged cancellation history neither writes preferences nor re-adds recovery")
        try LocalRecoveryStore.save([Submission](), name: "submissions")
        let storageModel = ChatModel(defaults: defaults)
        await storageModel.hydrateForChecks()
        try storageModel.applyForChecks(state)
        storageModel.demo = true; storageModel.connected = true
        await storageModel.flushForChecks()
        let receiptsFile = support.appendingPathComponent("PiMobile/submissions.json")
        try FileManager.default.removeItem(at: receiptsFile)
        try FileManager.default.createDirectory(at: receiptsFile, withIntermediateDirectories: false)
        storageModel.draft = "Original unsent text"; storageModel.addAttachment(file)
        storageModel.submit(mode: "send")
        storageModel.draft = "Newer composer edit"
        let newerFile = ComposerAttachment(id: "newer-file", name: "later.txt", mimeType: "text/plain", data: Data("Newer file".utf8))
        storageModel.addAttachment(newerFile)
        for _ in 0..<200 where storageModel.submissions.first?.status == "Sending" { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(storageModel.snapshot?.messages.isEmpty == true && !storageModel.busy, "Failed recovery persistence must prevent even demo transmission")
        precondition(storageModel.submissions.first?.recoverable == true && storageModel.submissions.first?.text == "Original unsent text" && storageModel.submissions.first?.attachments?.first?.data == file.data, "Not-sent input and original files remain recoverable")
        precondition(storageModel.draft == "Newer composer edit" && storageModel.attachments.first?.id == newerFile.id && storageModel.error?.contains("not sent") == true, "Storage failure explains no transmission and keeps newer composer text/files")
        await storageModel.flushForChecks()
        try FileManager.default.removeItem(at: receiptsFile)
        try LocalRecoveryStore.save([Submission](), name: "submissions")
        let confirmedModel = ChatModel(defaults: defaults)
        // Submit twice before hydration starts, while both snapshots are idle.
        try confirmedModel.applyForChecks(state)
        confirmedModel.demo = true; confirmedModel.connected = true
        confirmedModel.draft = "First durable message"; confirmedModel.addAttachment(file)
        confirmedModel.submit(mode: "send")
        let firstReceipt = confirmedModel.submissions[0]
        confirmedModel.dismissSubmission(firstReceipt.id)
        confirmedModel.restore(firstReceipt)
        precondition(confirmedModel.submissions.count == 1 && confirmedModel.draft.isEmpty, "Dismiss/restore cannot remove an in-flight receipt before confirmed persistence")
        confirmedModel.draft = "Second durable message"; confirmedModel.addAttachment(file)
        confirmedModel.submit(mode: "send")
        confirmedModel.draft = "Later typing"
        confirmedModel.addAttachment(newerFile)
        for _ in 0..<200 where !confirmedModel.busy { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(confirmedModel.busy, "Successful confirmed storage allows demo transmission")
        let durable = LocalRecoveryStore.load("submissions", as: [Submission].self) ?? []
        precondition(durable.map(\.text) == ["First durable message", "Second durable message"] && durable.allSatisfy { $0.status == "Sending" && $0.attachments?.first?.data == file.data }, "Both originals and files are durable before either demo reply completes")
        precondition(confirmedModel.draft == "Later typing" && confirmedModel.attachments.first?.id == newerFile.id, "Hydration and persistence barriers preserve later composer text/files")
        for _ in 0..<200 where confirmedModel.submissions.contains(where: { $0.status == "Sending" }) { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(confirmedModel.submissions.allSatisfy { $0.status == "Accepted" }, "Concurrent confirmed submissions both settle")
        await confirmedModel.flushForChecks()
        print("Recovery checks passed: drafts, bounded receipts, cancellation retention and confirmed persistence before transmission.")
    }
}
