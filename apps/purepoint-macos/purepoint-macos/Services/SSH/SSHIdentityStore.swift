#if os(iOS)
import CryptoKit
import Foundation
import Security

// MARK: - Secret storage

/// Small secrets keyed by account within one service. The Keychain in the app; in memory in
/// tests.
nonisolated protocol SecretStore: Sendable {
    func read(account: String) throws -> Data?
    func write(_ data: Data, account: String) throws
    func delete(account: String) throws
}

nonisolated struct KeychainError: LocalizedError, Equatable {
    let status: OSStatus

    var errorDescription: String? {
        let detail = SecCopyErrorMessageString(status, nil) as String? ?? "status \(status)"
        return "Keychain error: \(detail)"
    }
}

/// The Keychain calls a `KeychainSecretStore` makes: the Security framework in the app, a fake
/// in tests.
nonisolated struct KeychainCalls: Sendable {
    var update: @Sendable (_ query: [String: Any], _ attributes: [String: Any]) -> OSStatus
    var add: @Sendable (_ attributes: [String: Any]) -> OSStatus

    static let system = KeychainCalls(
        update: { SecItemUpdate($0 as CFDictionary, $1 as CFDictionary) },
        add: { SecItemAdd($0 as CFDictionary, nil) })
}

/// Generic-password Keychain items for one service, readable after the first unlock, kept on
/// this device only and never synchronized (IPAD-008).
nonisolated struct KeychainSecretStore: SecretStore {
    let service: String
    var keychain = KeychainCalls.system

    private func query(account: String) -> [String: Any] {
        [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: service,
            kSecAttrAccount as String: account,
            kSecAttrSynchronizable as String: kCFBooleanFalse as Any,
        ]
    }

    func read(account: String) throws -> Data? {
        var query = query(account: account)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess: return result as? Data
        case errSecItemNotFound: return nil
        default: throw KeychainError(status: status)
        }
    }

    /// Update the item in place, so a write that fails leaves the old value; add it only when
    /// there is none. The update also sets the accessibility class, so it is always the
    /// current one.
    func write(_ data: Data, account: String) throws {
        let values: [String: Any] = [
            kSecValueData as String: data,
            kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        ]
        var status = keychain.update(query(account: account), values)
        if status == errSecItemNotFound {
            status = keychain.add(query(account: account).merging(values) { _, new in new })
        }
        guard status == errSecSuccess else { throw KeychainError(status: status) }
    }

    func delete(account: String) throws {
        let status = SecItemDelete(query(account: account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw KeychainError(status: status) }
    }
}

// MARK: - Device identity

/// The iPad's SSH identity: one Ed25519 key used for every machine.
nonisolated struct SSHIdentity: Sendable {
    let privateKey: Curve25519.Signing.PrivateKey

    var publicKey: SSHPublicKey { .ed25519(privateKey.publicKey) }

    /// `ssh-ed25519 <base64> purepoint-ipad`, for the machine's `authorized_keys`.
    var authorizedKeyLine: String { publicKey.openSSHLine(comment: SSHIdentityStore.comment) }
}

/// Holds the device identity in the Keychain (`PurePoint.SSHIdentity.v1` / `default`, the raw
/// 32-byte private key). The first read generates one; an imported OpenSSH key replaces it.
/// The key never leaves the Keychain item: there is no export.
nonisolated final class SSHIdentityStore: @unchecked Sendable {
    static let service = "PurePoint.SSHIdentity.v1"
    static let account = "default"
    static let comment = "purepoint-ipad"

    static let shared = SSHIdentityStore(store: KeychainSecretStore(service: service))

    private let store: any SecretStore
    private let lock = NSLock()
    private var cached: SSHIdentity?

    init(store: any SecretStore) {
        self.store = store
    }

    /// The stored identity, generating and storing a new one when there is none.
    func identity() throws -> SSHIdentity {
        try lock.withLock {
            if let cached { return cached }
            let identity: SSHIdentity
            if let raw = try store.read(account: Self.account),
                let key = try? Curve25519.Signing.PrivateKey(rawRepresentation: raw)
            {
                identity = SSHIdentity(privateKey: key)
            } else {
                let key = Curve25519.Signing.PrivateKey()
                try store.write(key.rawRepresentation, account: Self.account)
                identity = SSHIdentity(privateKey: key)
            }
            cached = identity
            return identity
        }
    }

    /// Replace the identity with an imported OpenSSH Ed25519 private key. A key that does not
    /// parse is refused and the current identity stays.
    @discardableResult
    func importOpenSSHPrivateKey(_ text: String) throws -> SSHIdentity {
        let key = try OpenSSHPrivateKey.parseEd25519(text)
        return try lock.withLock {
            try store.write(key.rawRepresentation, account: Self.account)
            let identity = SSHIdentity(privateKey: key)
            cached = identity
            return identity
        }
    }
}
#endif
