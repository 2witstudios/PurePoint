import Foundation

// Read the actual private model state, compiling alongside ChatModel in the same file.
extension ChatModel {
    func connectionLossForChecks() { connectionLost("Fixture network loss") }
    var retryCountForChecks: Int { retryBudget.attempts }
    var wantsConnectionForChecks: Bool { wantsConnection }
}
final class DelayedTrustStore: @unchecked Sendable {
    private let lock = NSLock()
    let releaseFirst = DispatchSemaphore(value: 0)
    private var records: [TrustedHost] = []
    private var started = 0
    var startedCount: Int { lock.lock(); defer { lock.unlock() }; return started }
    var savedRecords: [TrustedHost] { lock.lock(); defer { lock.unlock() }; return records }
    func save(_ record: TrustedHost) {
        precondition(!Thread.isMainThread, "Keychain commits stay off the UI actor")
        lock.lock(); started += 1; let first = started == 1; lock.unlock()
        if first { precondition(releaseFirst.wait(timeout: .now() + 10) == .success) }
        lock.lock(); records.append(record); lock.unlock()
    }
    func read() -> TrustedHost? { lock.lock(); defer { lock.unlock() }; return records.last }
}
@main struct TrustChecks {
    @MainActor static func main() async throws {
        let directory = CommandLine.arguments[1]
        precondition(directory.hasPrefix("/tmp/pg-mobile-trust-"))
        let data = try Data(contentsOf: URL(fileURLWithPath: directory + "/record.json"))
        let saved = try JSONDecoder().decode(TrustedHost.self, from: data)
        precondition(saved.valid)
        let restarted = try JSONDecoder().decode(TrustedHost.self, from: JSONEncoder().encode(saved))
        precondition(restarted.clientId == saved.clientId, "Trust survives serialization/update")
        let persistence = DelayedTrustStore()
        let pairingDefaults = RecoveryDefaults(suiteName: "pg-trust-pair-" + UUID().uuidString)!
        let pairingModel = ChatModel(defaults: pairingDefaults, trustLoader: { _ in persistence.read() }, trustSaver: { persistence.save($0) })
        let firstCode = PairingCode.parse(try String(contentsOfFile: directory + "/enrollment-a.json", encoding: .utf8))!
        let secondCode = PairingCode.parse(try String(contentsOfFile: directory + "/enrollment-b.json", encoding: .utf8))!
        pairingModel.pair(firstCode)
        for _ in 0..<200 where persistence.startedCount == 0 { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(persistence.startedCount == 1, "First save is delayed at the utility IO boundary")
        pairingModel.setForeground(false); pairingModel.setForeground(true); pairingModel.pair(secondCode)
        try await Task.sleep(nanoseconds: 100_000_000)
        precondition(persistence.startedCount == 1, "Newer commit cannot overtake an older in-progress write")
        persistence.releaseFirst.signal()
        for _ in 0..<300 where !pairingModel.connected { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(pairingModel.connected && persistence.savedRecords.count == 2)
        precondition(persistence.savedRecords[0].clientId != persistence.savedRecords[1].clientId)
        precondition(persistence.read()?.clientId == pairingModel.clientId, "Canceled enrollment never overwrites the newer persisted principal")
        pairingModel.disconnect()
        let pin = PinnedHostSession(endpoint: saved.endpoint, certificateSHA256: saved.certificateSHA256)
        let session = pin.makeSession()
        var request = URLRequest(url: URL(string: saved.endpoint.replacingOccurrences(of: "wss:", with: "https:").replacingOccurrences(of: "/v1", with: "/pair/verify"))!)
        request.setValue("Bearer " + saved.credential, forHTTPHeaderField: "Authorization")
        let (_, response) = try await session.data(for: request)
        precondition((response as? HTTPURLResponse)?.statusCode == 200 && !pin.identityRejected, "Pinned certificate permits authenticated verification")
        session.invalidateAndCancel()
        let wrong = PinnedHostSession(endpoint: saved.endpoint, certificateSHA256: String(repeating: "0", count: 64))
        let wrongSession = wrong.makeSession()
        do { _ = try await wrongSession.data(for: request); preconditionFailure("Identity mismatch must refuse") } catch { precondition(wrong.identityRejected) }
        wrongSession.invalidateAndCancel()
        var budget = ReconnectBudget()
        precondition((0..<5).compactMap { _ in budget.nextDelay() } == [1,2,4,8,16])
        precondition(budget.nextDelay() == nil); budget.reset(); precondition(budget.nextDelay() == 1)
        let defaults = RecoveryDefaults(suiteName: "pg-trust-" + UUID().uuidString)!
        defaults.set(saved.endpoint, forKey: "pi.endpoint")
        let model = ChatModel(defaults: defaults, trustLoader: { _ in saved })
        model.connect()
        for _ in 0..<200 where !model.connected { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(model.connected && model.clientId == saved.clientId, "Credentials determine client identity before sync")
        model.draft = "Never replay this uncertain prompt"
        model.submissions = [Submission(id: "uncertain", text: "Not safe to resend", sessionId: "fixture", status: "Delivery uncertain", recoverable: true)]
        model.setForeground(false); precondition(!model.connected)
        model.setForeground(true)
        for _ in 0..<200 where !model.connected { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(model.connected && model.draft == "Never replay this uncertain prompt" && model.submissions.contains { $0.id == "uncertain" && $0.recoverable }, "Foreground reconnect retains input; no prompt replay")
        try Data("revoke".utf8).write(to: URL(fileURLWithPath: directory + "/revoke"))
        for _ in 0..<300 where model.connectionStatus != "Re-pair with your Mac" { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(!model.wantsConnectionForChecks && model.connectionStatus == "Re-pair with your Mac", "Revocation closes socket and stops retries until deliberate re-pair")
        model.disconnect(); model.setForeground(false); model.setForeground(true)
        precondition(!model.wantsConnectionForChecks && !model.connected, "User disconnect survives foreground")
        let legacy = ChatModel(defaults: defaults, trustLoader: { _ in nil }); legacy.connect()
        for _ in 0..<100 where legacy.connectionStatus != "Re-pair with your Mac" { try await Task.sleep(nanoseconds: 10_000_000) }
        precondition(!legacy.wantsConnectionForChecks && legacy.connectionStatus == "Re-pair with your Mac", "Lost/legacy trust never falls back or retries")
        print("Trust checks passed: real TLS pin success/mismatch, authoritative identity, saved trust, delayed-save supersession, foreground reconnect and no replay.")
    }
}
