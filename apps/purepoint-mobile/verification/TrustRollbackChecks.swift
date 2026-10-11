import Foundation
import Security

// Actual Keychain storage, unique fixture account only. Never touches saved user endpoints.
@main struct TrustRollbackChecks {
    static func main() async throws {
        let endpoint = "wss://fixture-" + UUID().uuidString.lowercased() + ".ts.net/v1"
        defer { try? PairingSecret.removeTrust(endpoint: endpoint); try? PairingSecret.finishTrustChange(endpoint: endpoint) }
        let host = UUID().uuidString, device = UUID().uuidString
        func trust(_ credential: String) -> TrustedHost {
            TrustedHost(version: 1, endpoint: endpoint, hostId: host, certificateSHA256: String(repeating: "a", count: 64), deviceId: device, clientId: device, credential: String(repeating: credential, count: 43))
        }
        func blocked() -> Bool { do { _ = try PairingSecret.readTrust(endpoint: endpoint); return false } catch { return true } }
        let first = trust("b"), replacement = trust("c")
        let writer = PairingTrustWriter(save: { try PairingSecret.saveTrust($0) }, begin: { try PairingSecret.beginTrustChange(endpoint: $0) }, finish: { try PairingSecret.finishTrustChange(endpoint: $0) }, backup: { try PairingSecret.backupTrust(endpoint: $0) })
        try PairingSecret.saveTrust(first)
        writer.advance(to: 1); try await writer.commit(replacement, generation: 1)
        precondition(blocked(), "Unaccepted trust must remain quarantined")
        writer.advance(to: 2); await writer.drain()
        precondition((try? PairingSecret.readTrust(endpoint: endpoint))?.credential == first.credential)
        let failing = PairingTrustWriter(save: { _ in throw MobileError("Fixture save failure") }, begin: { try PairingSecret.beginTrustChange(endpoint: $0) }, finish: { try PairingSecret.finishTrustChange(endpoint: $0) }, backup: { try PairingSecret.backupTrust(endpoint: $0) })
        failing.advance(to: 1)
        do { try await failing.commit(replacement, generation: 1); preconditionFailure("Expected save failure") } catch {}
        precondition((try? PairingSecret.readTrust(endpoint: endpoint))?.credential == first.credential, "Save failure must restore previously accepted trust")
        try PairingSecret.beginTrustChange(endpoint: endpoint)
        do { try await failing.commit(replacement, generation: 1); preconditionFailure("Expected save failure") } catch {}
        precondition(blocked(), "Save failure must retain preexisting quarantine")
        try await writer.commit(replacement, generation: 2)
        writer.advance(to: 3); await writer.drain()
        precondition(blocked(), "Cancellation must preserve preexisting quarantine")
        try await writer.commit(replacement, generation: 3)
        precondition(writer.accept(generation: 3)); await writer.drain()
        precondition((try? PairingSecret.readTrust(endpoint: endpoint))?.credential == replacement.credential)
        let identity: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "PiMobile.TrustedHost.v1", kSecAttrAccount as String: endpoint]
        for (offset, bytes) in [Data("corrupt saved trust".utf8), Data("{\"version\":999}".utf8)].enumerated() {
            precondition(SecItemUpdate(identity as CFDictionary, [kSecValueData as String: bytes] as CFDictionary) == errSecSuccess)
            precondition(blocked())
            let generation = 4 + offset * 2
            writer.advance(to: generation); try await writer.commit(first, generation: generation)
            writer.advance(to: generation + 1); await writer.drain()
            var query = identity; query[kSecReturnData as String] = true; var value: CFTypeRef?
            precondition(SecItemCopyMatching(query as CFDictionary, &value) == errSecSuccess)
            precondition(value as? Data == bytes && blocked(), "Canceled repair must restore opaque bytes without authorizing them")
            try await writer.commit(first, generation: generation + 1)
            precondition(writer.accept(generation: generation + 1)); await writer.drain()
            precondition((try? PairingSecret.readTrust(endpoint: endpoint))?.credential == first.credential)
        }
        print("Trust rollback: prior quarantine, raw corrupt/unknown bytes, cancel and deliberate repair PASS")
    }
}
