import Foundation

final class MachineDefaults: UserDefaults, @unchecked Sendable {}
final class MachineTrust: @unchecked Sendable {
    private let lock = NSLock()
    private var values: [String: TrustedHost] = [:]
    var delayNextWrite = false
    let writing = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    func read(_ endpoint: String) -> TrustedHost? { lock.lock(); defer { lock.unlock() }; return values[endpoint] }
    func save(_ record: TrustedHost) {
        lock.lock(); let delay = delayNextWrite; delayNextWrite = false; lock.unlock()
        if delay { writing.signal(); precondition(release.wait(timeout: .now() + 10) == .success) }
        lock.lock(); values[record.endpoint] = record; lock.unlock()
    }
    func waitForWrite() -> Bool { writing.wait(timeout: .now() + 10) == .success }
    func remove(_ endpoint: String) { lock.lock(); values.removeValue(forKey: endpoint); lock.unlock() }
}
final class DelayedRecoveryWriter: @unchecked Sendable {
    private let lock = NSLock()
    private var delayed = false
    let started = DispatchSemaphore(value: 0)
    let release = DispatchSemaphore(value: 0)
    func waitForStart() -> Bool { started.wait(timeout: .now() + 10) == .success }
    func save(_ value: [String: [Submission]]) throws {
        lock.lock()
        let delay = !delayed && value.values.joined().contains { $0.status == "Sending" }
        if delay { delayed = true }
        lock.unlock()
        if delay {
            started.signal(); precondition(release.wait(timeout: .now() + 10) == .success)
            throw MobileError("Fixture old persistence failure")
        }
        try LocalRecoveryStore.save(value, name: "machine-submissions-v1")
    }
}
actor MachineGate<Value: Sendable> {
    private var continuation: CheckedContinuation<Value, Never>?
    private var early: Value?
    func wait() async -> Value { if let early { return early }; return await withCheckedContinuation { continuation = $0 } }
    func open(_ value: Value) { if let continuation { self.continuation = nil; continuation.resume(returning: value) } else { early = value } }
}
extension ChatModel {
    func hydrateMachinesForChecks() async { await recoveryTask?.value }
    func flushMachinesForChecks() async { await withCheckedContinuation { c in writer.flush { c.resume() } } }
    func verifiedForChecks(_ trust: TrustedHost) throws { try bindVerifiedHost(trust); clientId = trust.clientId }
    func pairedForChecks(_ trust: TrustedHost) { activatePairedHost(trust) }
    func stateForChecks(_ state: Snapshot) throws { try apply(state) }
    func queuedReceiptForChecks(_ id: String, machine: String) { updateSubmission(id, status: "Queued", recoverable: false, machine: machine) }
    func receiptForChecks(_ id: String, machine: String) { updateSubmission(id, status: "Delivery uncertain", recoverable: true, machine: machine) }
    var noTransportForChecks: Bool { socket == nil && pending.isEmpty }
    var intentForChecks: Bool { wantsConnection }
    func networkForChecks(_ reachable: Bool) { networkChanged(reachable: reachable) }
}
@main struct MachineChecks {
    static let hostA = "11111111-1111-4111-8111-111111111111"
    static let hostB = "22222222-2222-4222-8222-222222222222"
    static func trust(_ host: String, endpoint: String, device: String = "33333333-3333-4333-8333-333333333333") -> TrustedHost {
        TrustedHost(version: 1, endpoint: endpoint, hostId: host, certificateSHA256: String(repeating: "a", count: 64), deviceId: device, clientId: device, credential: String(repeating: "A", count: 43))
    }
    static func state(_ id: String, client: String, canceled: String = "cancel") -> Snapshot {
        Snapshot(version: 1, epoch: "same-epoch", revision: 1, busy: false, sessionId: id, title: id, messages: [ChatMessage(id: "same", role: "assistant", text: id)], tools: [], queue: [], dialogs: [], notices: [], canceled: [CanceledText(clientId: client, id: canceled, text: id + " canceled", sessionId: id)])
    }
    static func code(_ trust: TrustedHost) -> PairingCode {
        PairingCode(type: "pi-mobile-pairing", version: 1, endpoint: trust.endpoint, hostId: trust.hostId, certificateSHA256: trust.certificateSHA256, enrollmentToken: String(repeating: "B", count: 43), expiresAt: Int64(Date().timeIntervalSince1970 * 1000) + 120000)
    }
    @MainActor static func eventually(_ condition: () -> Bool) async throws {
        for _ in 0..<300 { if condition() { return }; try await Task.sleep(nanoseconds: 10_000_000) }
        preconditionFailure("Fixture timed out")
    }
    @MainActor static func main() async throws {
        let home = ProcessInfo.processInfo.environment["CFFIXED_USER_HOME"] ?? ""
        precondition(home.hasPrefix("/tmp/pi-mobile-machines-") && FileManager.default.homeDirectoryForCurrentUser.path == home)
        let a = trust(hostA, endpoint: "wss://100.64.0.1:9443/v1")
        let b = trust(hostB, endpoint: "wss://100.64.0.2:9443/v1", device: "44444444-4444-4444-8444-444444444444")
        let defaults = MachineDefaults(suiteName: UUID().uuidString)!
        defaults.set(a.endpoint, forKey: "pi.endpoint"); defaults.set(a.clientId, forKey: "pi.clientId")
        defaults.set([a.endpoint + ":local": "Legacy text"], forKey: "pi.drafts")
        defaults.set(["old-canceled"], forKey: "pi.canceledIds")
        let fileA = ComposerAttachment(id: "A", name: "A.txt", mimeType: "text/plain", data: Data("A bytes".utf8))
        let fileB = ComposerAttachment(id: "B", name: "B.txt", mimeType: "text/plain", data: Data("B bytes".utf8))
        let receipt = Submission(id: "legacy", text: "A recovery", sessionId: "A session", status: "Sending", recoverable: false, attachments: [fileA])
        try LocalRecoveryStore.save([receipt], name: "submissions")
        try LocalRecoveryStore.save([a.endpoint + ":local": [fileA]], name: "attachment-drafts")
        let vault = MachineTrust(); vault.save(a); vault.save(b)
        let model = ChatModel(defaults: defaults, trustLoader: { vault.read($0) }, trustSaver: { vault.save($0) }, trustRemover: { vault.remove($0) })
        model.setForeground(false); await model.hydrateMachinesForChecks()
        precondition(model.endpoint == a.endpoint && model.draft == "Legacy text" && model.clientId == a.clientId && model.attachments.first?.data == fileA.data && model.submissions.first?.recoverable == true, "Given a legacy paired phone, should retain principal/text/attachments/uncertain recovery")
        try model.verifiedForChecks(a); try model.verifiedForChecks(a)
        precondition(model.machines.count == 1 && model.selectedMachine?.id == hostA && model.machineKey == a.endpoint, "Given verified migration twice, should bind idempotently without losing legacy state namespace")
        try model.stateForChecks(state("A session", client: a.clientId)); model.draft = "A edited"
        model.pairedForChecks(b); model.setForeground(false); try model.verifiedForChecks(b)
        precondition(model.snapshot == nil && model.submissions.isEmpty && model.attachments.isEmpty && model.draft.isEmpty, "Given B selected, should clear A transcript/transients and isolate all local recovery")
        try model.stateForChecks(state("B session", client: b.clientId))
        precondition(model.submissions.first?.text == "B session canceled", "Given identical receipt IDs on A and B, should retain B's own canceled receipt")
        model.draft = "B edited"; model.addAttachment(fileB)
        model.receiptForChecks("legacy", machine: a.endpoint)
        precondition(model.submissions.allSatisfy { $0.text != "A recovery" }, "Given A's late receipt while on B, should update only A")
        model.selectMachine(hostA); model.setForeground(false)
        precondition(model.draft == "A edited" && model.attachments.first?.id == "A" && model.submissions.contains { $0.id == "legacy" && $0.recoverable }, "Given switching back, should restore A text/files/recovery")
        model.renameMachine(hostA, name: " \u{0000} Desk Mac \n")
        model.renameMachine(hostB, name: "Travel 💻")
        precondition(model.selectedMachine?.name == "Desk Mac" && model.selectedMachine?.id == hostA && model.draft == "A edited", "Given renaming either Mac, should update its label without changing selection or drafts")
        model.renameMachine(hostA, name: " \n\u{0000} ")
        precondition(model.selectedMachine?.name == "Desk Mac", "Given an empty sanitized name, should retain the existing name")
        let namedRestart = ChatModel(defaults: defaults, trustLoader: { vault.read($0) })
        namedRestart.setForeground(false); await namedRestart.hydrateMachinesForChecks()
        precondition(namedRestart.selectedMachine?.name == "Desk Mac" && namedRestart.machines.first { $0.id == hostB }?.name == "Travel 💻", "Given relaunch, should persist names for both the selected and unselected Mac")
        model.renameMachine(hostB, name: String(repeating: "💻", count: 65))
        precondition(model.machines.first { $0.id == hostB }?.name == String(repeating: "💻", count: 60) && model.registry.valid, "Given a long Unicode name, should bound it by complete characters and preserve a valid registry")
        model.renameMachine(hostA, name: "Same name"); model.renameMachine(hostB, name: "Same name")
        model.submissions.append(Submission(id: "old-queued", text: "Old principal queued", sessionId: "A session", status: "Queued", recoverable: false, attachments: [fileA], clientId: a.clientId))
        model.editorOffer = "A pending editor"
        model.selectMachine(hostB); model.setForeground(false)
        precondition(model.editorOffer == nil, "Given B selected, should not show A's pending editor")
        model.selectMachine(hostA); model.setForeground(false)
        precondition(model.editorOffer == "A pending editor", "Given returning to A, should retain its pending offer")
        model.editorOffer = nil
        model.selectMachine(hostB); model.setForeground(false); model.selectMachine(hostA); model.setForeground(false)
        precondition(model.editorOffer == nil, "Given explicit dismissal, should not resurrect an editor offer")
        let moved = trust(hostA, endpoint: "wss://100.64.0.3:9443/v1", device: "55555555-5555-4555-8555-555555555555")
        model.pairedForChecks(moved); model.setForeground(false)
        precondition(model.machines.count == 2 && model.selectedMachine?.name == "Same name" && model.machineKey == a.endpoint && model.draft == "A edited" && model.submissions.contains { $0.id == "legacy" }, "Given same-host re-pair/address/principal change, should deduplicate while preserving names and prior principal's local recovery")
        precondition(model.submissions.contains { $0.id == "old-queued" && $0.recoverable && $0.attachments?.first?.id == "A" }, "Given re-pair with a new principal, should retain old queued text/files as uncertain recovery")
        model.queuedReceiptForChecks("old-queued", machine: a.endpoint)
        precondition(model.submissions.contains { $0.id == "old-queued" && $0.recoverable }, "Given late old-principal queued completion, should not hide recovery")
        await model.flushMachinesForChecks()
        let restarted = ChatModel(defaults: defaults, trustLoader: { vault.read($0) }); restarted.setForeground(false); await restarted.hydrateMachinesForChecks()
        precondition(restarted.selectedMachine?.id == hostA && restarted.endpoint == moved.endpoint && restarted.draft == "A edited" && restarted.attachments.first?.id == "A", "Given relaunch, should retain selected machine and per-session drafts")
        precondition(restarted.submissions.contains { $0.id == "old-queued" && $0.recoverable && $0.clientId == a.clientId }, "Given restart after re-pair, should preserve old receipt provenance and recovery")
        restarted.selectMachine(hostB); restarted.setForeground(false)
        precondition(restarted.draft == "B edited" && restarted.attachments.first?.id == "B" && restarted.submissions.first?.text == "B session canceled", "Given relaunch then B, should restore only B state")

        // Delay hydration across selection, edits and removal; saved data must not resurrect removed files.
        let gate = MachineGate<([String: [ComposerAttachment]], [String: [Submission]])>()
        let delayed = ChatModel(defaults: defaults, trustLoader: { vault.read($0) }, recoveryLoader: { await gate.wait() })
        delayed.setForeground(false); delayed.selectMachine(hostB); delayed.draft = "New B edit"; delayed.attachments = []
        await gate.open(([a.endpoint + ":A session": [fileA], hostB + ":B session": [fileB]], [a.endpoint: [receipt], hostB: [Submission(id: "b-recovery", text: "B recovery", sessionId: "B session", status: "Sending", recoverable: false)]]))
        await delayed.hydrateMachinesForChecks()
        precondition(delayed.selectedMachine?.id == hostB && delayed.draft == "New B edit" && delayed.attachments.isEmpty && delayed.submissions.map(\.text) == ["B recovery"], "Given late startup hydration after switch and edit/removal, should preserve B choice/edits and scope recovery")
        delayed.selectMachine(hostA); delayed.setForeground(false)
        precondition(delayed.attachments.first?.id == "A" && delayed.submissions.first?.text == "A recovery" && delayed.noTransportForChecks, "Given switching back after delayed hydration, should restore A without replay")

        // Late disk hydration must also apply a principal change that already succeeded.
        let principalGate = MachineGate<([String: [ComposerAttachment]], [String: [Submission]])>()
        let principalDelay = ChatModel(defaults: defaults, trustLoader: { vault.read($0) }, recoveryLoader: { await principalGate.wait() })
        principalDelay.setForeground(false); principalDelay.pairedForChecks(moved)
        await principalGate.open(([:], [a.endpoint: [Submission(id: "late-old-queued", text: "Late old queue", sessionId: "A session", status: "Queued", recoverable: false, attachments: [fileA], clientId: a.clientId)]]))
        await principalDelay.hydrateMachinesForChecks()
        precondition(principalDelay.submissions.contains { $0.id == "late-old-queued" && $0.recoverable && $0.attachments?.first?.id == "A" && $0.clientId == a.clientId }, "Given late hydration after re-pair, should retain old-principal queue as recovery without replay")

        // A remains usable across enrollment failure, mismatched identity, and late canceled success.
        let pairDefaults = MachineDefaults(suiteName: UUID().uuidString)!
        let pairGate = MachineGate<TrustedHost>()
        let pairing = ChatModel(defaults: pairDefaults, trustLoader: { vault.read($0) }, trustSaver: { vault.save($0) }, trustRemover: { vault.remove($0) }, enrollment: { _ in await pairGate.wait() })
        pairing.setForeground(false); await pairing.hydrateMachinesForChecks(); pairing.pairedForChecks(a); pairing.connected = true; pairing.draft = "Live A"
        pairing.pair(code(b)); precondition(pairing.connected && pairing.endpoint == a.endpoint && pairing.draft == "Live A", "Given starting B enrollment, should keep A usable")
        pairing.cancelPairing(); await pairGate.open(b); try await Task.sleep(nanoseconds: 50_000_000)
        precondition(pairing.selectedMachine?.id == hostA && pairing.machines.count == 1 && pairing.draft == "Live A", "Given late canceled enrollment, should not persist B metadata or select B")
        let failing = ChatModel(defaults: pairDefaults, trustLoader: { vault.read($0) }, enrollment: { _ in throw MobileError("Fixture failure") }); failing.setForeground(false); await failing.hydrateMachinesForChecks(); failing.connected = true; failing.draft = "A usable"
        failing.pair(code(b)); try await eventually { !failing.pairing }
        precondition(failing.connected && failing.selectedMachine?.id == hostA && failing.draft == "A usable" && failing.pairingError != nil && failing.error == nil, "Given B failure, should leave A active and report pairing-only error")
        let mismatch = ChatModel(defaults: pairDefaults, trustLoader: { vault.read($0) }, enrollment: { _ in a }); mismatch.setForeground(false); mismatch.connected = true; mismatch.pair(code(b)); try await eventually { !mismatch.pairing }
        precondition(mismatch.selectedMachine?.id == hostA && mismatch.pairingError != nil, "Given wrong enrollment host/pin, should refuse B")

        // A utility Keychain write can finish after UI cancellation; rollback precedes a newer accepted commit.
        let freshVault = MachineTrust(); freshVault.delayNextWrite = true
        let writer = PairingTrustWriter(save: { freshVault.save($0) }, load: { freshVault.read($0) }, remove: { freshVault.remove($0) })
        writer.advance(to: 1)
        let write = Task { try await writer.commit(b, generation: 1) }
        let started = await Task.detached { freshVault.waitForWrite() }.value; precondition(started)
        writer.advance(to: 2); freshVault.release.signal()
        do { try await write.value; preconditionFailure("Canceled write must refuse") } catch {}
        precondition(freshVault.read(b.endpoint) == nil, "Given canceled in-progress trust write, should remove stale credential")
        try await writer.commit(a, generation: 2); precondition(writer.accept(generation: 2))
        writer.advance(to: 3); precondition(freshVault.read(a.endpoint)?.clientId == a.clientId, "Given accepted trust commit, should retain it across later cancellation")
        try await writer.commit(b, generation: 3); writer.advance(to: 4)
        try await writer.commit(moved, generation: 4); precondition(writer.accept(generation: 4))
        precondition(freshVault.read(b.endpoint) == nil, "Given cancellation between save and UI acceptance, should roll back before the next commit")

        let storage = DelayedRecoveryWriter()
        let stalled = ChatModel(defaults: defaults, trustLoader: { vault.read($0) }, recoveryLoader: { ([:], [:]) }, recoverySaver: { try storage.save($0) })
        stalled.setForeground(false); await stalled.hydrateMachinesForChecks(); stalled.selectMachine(hostA)
        try stalled.verifiedForChecks(moved); try stalled.stateForChecks(state("A session", client: moved.clientId, canceled: "other"))
        stalled.connected = true; stalled.draft = "Delayed A send"; stalled.submit(mode: "send")
        let persistenceStarted = await Task.detached { storage.waitForStart() }.value; precondition(persistenceStarted)
        stalled.selectMachine(hostB); stalled.draft = "New B edit"; storage.release.signal()
        try await Task.sleep(nanoseconds: 100_000_000)
        precondition(stalled.selectedMachine?.id == hostB && stalled.draft == "New B edit" && stalled.error == nil && stalled.noTransportForChecks && stalled.submissions.allSatisfy { $0.text != "Delayed A send" }, "Given old-host queued save/error and unsent submission after switch, should preserve B edits without leaking error or transmitting old input")
        stalled.selectMachine(hostA); stalled.setForeground(false)
        precondition(stalled.submissions.contains { $0.text == "Delayed A send" && $0.recoverable }, "Given delayed pre-send persistence then switching back, should recover A without sending it")
        await stalled.flushMachinesForChecks()
        let corrupt = ChatModel(defaults: defaults, recoveryLoader: { throw MobileError("Fixture damaged storage") })
        corrupt.setForeground(false); await corrupt.hydrateMachinesForChecks(); corrupt.connected = true; try corrupt.stateForChecks(state("A session", client: a.clientId)); corrupt.draft = "Keep unsent"
        precondition(!corrupt.canSend && corrupt.error?.contains("recovery") == true, "Given unreadable recovery, should refuse sending without discarding saved data")

        restarted.disconnect(); restarted.setForeground(false); restarted.setForeground(true); restarted.networkForChecks(true)
        precondition(!restarted.intentForChecks && restarted.noTransportForChecks && restarted.selectedMachine?.id == hostB, "Given explicit disconnect plus foreground/network recovery, should preserve selected B without reconnect")
        print("Machine checks passed: legacy migration/relaunch; A/B/back drafts/files/receipts; dedup/address change; delayed hydration; failure/cancel/pin identity; trust rollback; explicit disconnect; no replay.")
    }
}
