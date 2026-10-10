import Foundation

// Read the actual private model state, compiling alongside ChatModel in the same file.
extension ChatModel {
    func connectionLossForChecks() { connectionLost("Fixture network loss") }
    var retryCountForChecks: Int { retryBudget.attempts }
    var wantsConnectionForChecks: Bool { wantsConnection }
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
        print("Trust checks passed: real TLS pin success/mismatch, authoritative identity, saved trust, foreground reconnect and no replay.")
    }
}
