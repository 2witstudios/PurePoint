import Foundation
import Security
import CryptoKit

struct TrustedHost: Codable, Sendable {
    let version: Int
    let endpoint: String
    let hostId: String
    let certificateSHA256: String
    let deviceId: String
    let clientId: String
    let credential: String
    var valid: Bool {
        version == 1 && ConnectionAddress.url(endpoint)?.scheme == "wss" && UUID(uuidString: hostId) != nil && UUID(uuidString: deviceId) != nil && clientId == deviceId && certificateSHA256.count == 64 && certificateSHA256.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) } && credential.utf8.count == 43 && credential.utf8.allSatisfy { (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95 }
    }
}
enum PairingSecret {
    private static func query(endpoint: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "PiMobile.TrustedHost.v1", kSecAttrAccount as String: endpoint]
    }
    static func readTrust(endpoint: String) throws -> TrustedHost? {
        var attributes = query(endpoint: endpoint)
        attributes[kSecReturnData as String] = true; attributes[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(attributes as CFDictionary, &item)
        if status == errSecItemNotFound { return nil } // Legacy PiMobile.Pairing strings never become trust.
        guard status == errSecSuccess else { throw MobileError("Unlock this phone to access its saved Mac trust, then reconnect.") }
        guard let data = item as? Data, let record = try? JSONDecoder().decode(TrustedHost.self, from: data), record.valid, record.endpoint == endpoint else { throw TrustFailure("Saved Mac trust is invalid. Scan a new Mac QR code deliberately.") }
        return record
    }
    static func saveTrust(_ record: TrustedHost) throws {
        guard record.valid else { throw TrustFailure("The Mac returned invalid device trust. Scan a new QR code.") }
        let attributes: [String: Any] = [kSecValueData as String: try JSONEncoder().encode(record), kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        let identity = query(endpoint: record.endpoint)
        var status = SecItemUpdate(identity as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound { status = SecItemAdd(identity.merging(attributes) { _, new in new } as CFDictionary, nil) }
        guard status == errSecSuccess else { throw MobileError("Could not save Mac trust in Keychain (\(status)). Create a new QR and try again.") }
    }
}
// Generation updates and writes share one ordered utility queue. Cancellation never
// lets an older in-progress Keychain write finish after a newer accepted commit.
final class PairingTrustWriter: @unchecked Sendable {
    private let queue = DispatchQueue(label: "pointguard.trust.commit", qos: .utility)
    private var generation = 0 // Access only on queue; UI invalidation is nonblocking.
    private let save: @Sendable (TrustedHost) throws -> Void
    init(save: @escaping @Sendable (TrustedHost) throws -> Void) { self.save = save }
    func advance(to generation: Int) { queue.async { self.generation = generation } }
    func commit(_ record: TrustedHost, generation: Int) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            queue.async {
                guard generation == self.generation else { continuation.resume(throwing: CancellationError()); return }
                do { try self.save(record); continuation.resume() }
                catch { continuation.resume(throwing: error) }
            }
        }
    }
}
struct MobileError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
struct TrustFailure: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

// Dedicated session: no shared connection pool, redirects, cookies or challenge fallback.
final class PinnedHostSession: NSObject, URLSessionDelegate, URLSessionTaskDelegate, @unchecked Sendable {
    private let pin: String
    private let host: String
    private let lock = NSLock()
    private var failedIdentity = false
    var identityRejected: Bool { lock.lock(); defer { lock.unlock() }; return failedIdentity }
    init(endpoint: String, certificateSHA256: String) {
        pin = certificateSHA256; host = URL(string: endpoint)?.host?.replacingOccurrences(of: "[", with: "").replacingOccurrences(of: "]", with: "") ?? ""
    }
    func makeSession() -> URLSession {
        let config = URLSessionConfiguration.ephemeral
        config.timeoutIntervalForRequest = 10; config.timeoutIntervalForResource = 15
        config.httpShouldSetCookies = false; config.urlCache = nil
        return URLSession(configuration: config, delegate: self, delegateQueue: nil)
    }
    func urlSession(_ session: URLSession, didReceive challenge: URLAuthenticationChallenge, completionHandler: @escaping @Sendable (URLSession.AuthChallengeDisposition, URLCredential?) -> Void) {
        guard challenge.protectionSpace.authenticationMethod == NSURLAuthenticationMethodServerTrust,
              challenge.protectionSpace.host == host,
              let trust = challenge.protectionSpace.serverTrust,
              let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate], let certificate = chain.first,
              SHA256.hash(data: SecCertificateCopyData(certificate) as Data).map({ String(format: "%02x", $0) }).joined() == pin else {
            lock.lock(); failedIdentity = true; lock.unlock()
            completionHandler(.cancelAuthenticationChallenge, nil); return
        }
        // The exact QR-pinned leaf is the anchor; its possession authenticates the host.
        SecTrustSetPolicies(trust, SecPolicyCreateBasicX509())
        SecTrustSetAnchorCertificates(trust, [certificate] as CFArray)
        SecTrustSetAnchorCertificatesOnly(trust, true)
        guard SecTrustEvaluateWithError(trust, nil) else {
            lock.lock(); failedIdentity = true; lock.unlock()
            completionHandler(.cancelAuthenticationChallenge, nil); return
        }
        completionHandler(.useCredential, URLCredential(trust: trust))
    }
    func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse, newRequest request: URLRequest, completionHandler: @escaping @Sendable (URLRequest?) -> Void) { completionHandler(nil) }
}
