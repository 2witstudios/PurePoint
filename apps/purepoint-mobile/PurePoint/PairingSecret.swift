import Foundation
import Security
import CryptoKit

nonisolated struct TrustedHost: Codable, Sendable {
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
nonisolated enum PairingSecret {
    // The iPad app shares this file (PUREPOINT_IPAD); its messages name the iPad.
    #if PUREPOINT_IPAD
    static let device = "iPad"
    #else
    static let device = "phone"
    #endif
    private static func query(endpoint: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "PiMobile.TrustedHost.v1", kSecAttrAccount as String: endpoint]
    }
    private static func pendingQuery(endpoint: String) -> [String: Any] {
        [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "PiMobile.PendingTrust.v1", kSecAttrAccount as String: endpoint]
    }
    static func beginTrustChange(endpoint: String) throws {
        let identity = pendingQuery(endpoint: endpoint)
        let value: [String: Any] = [kSecValueData as String: Data("pending".utf8), kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        var status = SecItemUpdate(identity as CFDictionary, value as CFDictionary)
        if status == errSecItemNotFound { status = SecItemAdd(identity.merging(value) { _, new in new } as CFDictionary, nil) }
        guard status == errSecSuccess else { throw MobileError("Could not begin safe Mac trust storage. Unlock this \(device) and try again.") }
    }
    static func finishTrustChange(endpoint: String) throws {
        let status = SecItemDelete(pendingQuery(endpoint: endpoint) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw MobileError("Could not finalize Mac trust. Unlock this \(device) and re-pair deliberately.") }
    }
    static func readTrust(endpoint: String) throws -> TrustedHost? {
        let status = SecItemCopyMatching(pendingQuery(endpoint: endpoint) as CFDictionary, nil)
        guard status == errSecItemNotFound else {
            if status == errSecSuccess { throw TrustFailure("Mac pairing was interrupted before trust finalized. Scan a new QR code deliberately.") }
            throw MobileError("Unlock this \(device) to access its saved Mac trust, then reconnect.")
        }
        return try readTrustRecord(endpoint: endpoint)
    }
    private static func readTrustRecord(endpoint: String) throws -> TrustedHost? {
        var attributes = query(endpoint: endpoint)
        attributes[kSecReturnData as String] = true; attributes[kSecMatchLimit as String] = kSecMatchLimitOne
        var item: CFTypeRef?
        let status = SecItemCopyMatching(attributes as CFDictionary, &item)
        if status == errSecItemNotFound { return nil } // Legacy PiMobile.Pairing strings never become trust.
        guard status == errSecSuccess else { throw MobileError("Unlock this \(device) to access its saved Mac trust, then reconnect.") }
        guard let data = item as? Data, let record = try? JSONDecoder().decode(TrustedHost.self, from: data), record.valid, record.endpoint == endpoint else { throw TrustFailure("Saved Mac trust is invalid. Scan a new Mac QR code deliberately.") }
        return record
    }
    // Back up bytes and quarantine provenance without interpreting or authorizing them.
    // A deliberate enrollment can repair corrupt/unknown records; cancellation restores
    // exactly the prior fail-closed state, including a preexisting pending marker.
    static func backupTrust(endpoint: String) throws -> (@Sendable () throws -> Void) {
        var attributes = query(endpoint: endpoint); attributes[kSecReturnData as String] = true
        var item: CFTypeRef?
        let status = SecItemCopyMatching(attributes as CFDictionary, &item)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw MobileError("Unlock this \(device) to safely replace Mac trust.") }
        let previous = item as? Data
        guard status == errSecItemNotFound || previous != nil else { throw MobileError("Could not preserve previous Mac trust.") }
        let pending = SecItemCopyMatching(pendingQuery(endpoint: endpoint) as CFDictionary, nil)
        guard pending == errSecSuccess || pending == errSecItemNotFound else { throw MobileError("Unlock this \(device) to safely replace Mac trust.") }
        let wasPending = pending == errSecSuccess
        return {
            if let previous { try writeTrustData(previous, endpoint: endpoint) } else { try removeTrust(endpoint: endpoint) }
            if wasPending { try beginTrustChange(endpoint: endpoint) } else { try finishTrustChange(endpoint: endpoint) }
        }
    }
    private static func writeTrustData(_ data: Data, endpoint: String) throws {
        let attributes: [String: Any] = [kSecValueData as String: data, kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly]
        let identity = query(endpoint: endpoint)
        var status = SecItemUpdate(identity as CFDictionary, attributes as CFDictionary)
        if status == errSecItemNotFound { status = SecItemAdd(identity.merging(attributes) { _, new in new } as CFDictionary, nil) }
        guard status == errSecSuccess else { throw MobileError("Could not save Mac trust in Keychain (\(status)). Create a new QR and try again.") }
    }
    static func removeTrust(endpoint: String) throws {
        let status = SecItemDelete(query(endpoint: endpoint) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw MobileError("Could not discard canceled Mac trust. Unlock this \(device) and try again.") }
    }
    static func saveTrust(_ record: TrustedHost) throws {
        guard record.valid else { throw TrustFailure("The Mac returned invalid device trust. Scan a new QR code.") }
        try writeTrustData(JSONEncoder().encode(record), endpoint: record.endpoint)
    }
}
// Invalidation is immediate. All writes and rollback run on one queue; a canceled
// enrollment cannot leave a credential behind or overwrite a subsequent enrollment.
nonisolated final class PairingTrustWriter: @unchecked Sendable {
    private let queue = DispatchQueue(label: "pointguard.trust.commit", qos: .utility)
    private let lock = NSLock()
    private var generation = 0
    private var rollback: (@Sendable () throws -> Void)?
    private var pendingEndpoint: String?
    private let backup: (@Sendable (String) throws -> (@Sendable () throws -> Void))?
    private let begin: @Sendable (String) throws -> Void
    private let finish: @Sendable (String) throws -> Void
    private let save: @Sendable (TrustedHost) throws -> Void
    private let load: @Sendable (String) throws -> TrustedHost?
    private let remove: @Sendable (String) throws -> Void
    init(save: @escaping @Sendable (TrustedHost) throws -> Void,
         load: @escaping @Sendable (String) throws -> TrustedHost? = { try PairingSecret.readTrust(endpoint: $0) },
         remove: @escaping @Sendable (String) throws -> Void = { try PairingSecret.removeTrust(endpoint: $0) },
         begin: @escaping @Sendable (String) throws -> Void = { _ in },
         finish: @escaping @Sendable (String) throws -> Void = { _ in },
         backup: (@Sendable (String) throws -> (@Sendable () throws -> Void))? = nil) {
        self.save = save; self.load = load; self.remove = remove; self.begin = begin; self.finish = finish; self.backup = backup
    }
    func advance(to generation: Int) {
        lock.lock(); self.generation = generation
        let undo = rollback; rollback = nil; pendingEndpoint = nil; lock.unlock()
        if let undo { queue.async { try? undo() } }
    }
    func accept(generation: Int) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard generation == self.generation, rollback != nil, let endpoint = pendingEndpoint else { return false }
        rollback = nil; pendingEndpoint = nil
        queue.async { try? self.finish(endpoint) } // failure leaves the persistent quarantine closed
        return true
    }
    func drain() async {
        await withCheckedContinuation { (continuation: CheckedContinuation<Void, Never>) in queue.async { continuation.resume() } }
    }
    private func current(_ generation: Int) -> Bool { lock.lock(); defer { lock.unlock() }; return self.generation == generation }
    func commit(_ record: TrustedHost, generation: Int) async throws {
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            queue.async {
                guard self.current(generation) else { continuation.resume(throwing: CancellationError()); return }
                do {
                    let undo: @Sendable () throws -> Void
                    if let backup = self.backup { undo = try backup(record.endpoint) }
                    else {
                        let previous = try self.load(record.endpoint)
                        undo = {
                            if let previous { try self.save(previous) } else { try self.remove(record.endpoint) }
                            try self.finish(record.endpoint)
                        }
                    }
                    guard self.current(generation) else { throw CancellationError() }
                    try self.begin(record.endpoint)
                    do { try self.save(record) }
                    catch { try? undo(); throw error } // failed rollback leaves quarantine closed
                    self.lock.lock()
                    let valid = self.generation == generation
                    if valid { self.rollback = undo; self.pendingEndpoint = record.endpoint }
                    self.lock.unlock()
                    if !valid { try undo(); throw CancellationError() }
                    continuation.resume()
                } catch { continuation.resume(throwing: error) }
            }
        }
    }
}
nonisolated struct MobileError: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}
nonisolated struct TrustFailure: LocalizedError {
    let message: String
    init(_ message: String) { self.message = message }
    var errorDescription: String? { message }
}

// Dedicated session: no shared connection pool, redirects, cookies or challenge fallback.
nonisolated final class PinnedHostSession: NSObject, URLSessionDelegate, URLSessionTaskDelegate, @unchecked Sendable {
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
