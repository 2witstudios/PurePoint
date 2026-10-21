#if os(iOS)
import CryptoKit
import Foundation
import Security
import Testing

@testable import PurePoint

/// Secrets in memory, standing in for the Keychain.
final class InMemorySecretStore: SecretStore, @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String: Data] = [:]
    var failReads = false

    func read(account: String) throws -> Data? {
        try lock.withLock {
            if failReads { throw KeychainError(status: errSecInteractionNotAllowed) }
            return items[account]
        }
    }

    func write(_ data: Data, account: String) throws { lock.withLock { items[account] = data } }
    func delete(account: String) throws { _ = lock.withLock { items.removeValue(forKey: account) } }
    var count: Int { lock.withLock { items.count } }
}

/// A known test-only key, built from a fixed seed so no private key file is committed.
private let fixedKey = try! Curve25519.Signing.PrivateKey(rawRepresentation: Data((1...32).map { UInt8($0) }))
private let sshKeygenKey = OpenSSHPrivateKey.encodeEd25519(fixedKey, comment: "test@example", checkInt: 0x1234_5678)
private let sshKeygenPublic = SSHPublicKey.ed25519(fixedKey.publicKey).openSSHLine(comment: "test@example")

struct OpenSSHKeyFormatTests {
    @Test func publicKeyLineRoundTrips() throws {
        let key = Curve25519.Signing.PrivateKey().publicKey
        let line = SSHPublicKey.ed25519(key).openSSHLine(comment: "purepoint-ipad")
        #expect(line.hasPrefix("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAI"))
        #expect(line.hasSuffix(" purepoint-ipad"))
        let parsed = try SSHPublicKey(openSSHLine: line)
        #expect(parsed == .ed25519(key))
        #expect(parsed.blob.suffix(32) == key.rawRepresentation)
    }

    @Test func fingerprintIsOpenSSHSHA256Form() throws {
        let key = try SSHPublicKey(openSSHLine: sshKeygenPublic)
        let expected = Data(SHA256.hash(data: key.blob)).base64EncodedString().replacingOccurrences(of: "=", with: "")
        #expect(key.fingerprint == "SHA256:" + expected)
        #expect(!key.fingerprint.hasSuffix("="))
        #expect(key.displayType == "ED25519")
    }

    @Test func publicKeyLineWithMismatchedTypeIsRefused() {
        let blob = SSHWire.string("ssh-ed25519") + SSHWire.string(Data(count: 32))
        #expect(throws: SSHKeyImportError.malformed) {
            try SSHPublicKey(openSSHLine: "ssh-rsa " + blob.base64EncodedString())
        }
    }

    @Test func privateKeyRoundTripsThroughOpenSSHFormat() throws {
        let key = Curve25519.Signing.PrivateKey()
        let text = OpenSSHPrivateKey.encodeEd25519(key, comment: "round-trip")
        #expect(text.hasPrefix(OpenSSHPrivateKey.beginMarker))
        let parsed = try OpenSSHPrivateKey.parseEd25519(text)
        #expect(parsed.rawRepresentation == key.rawRepresentation)
    }

    @Test func encodingMatchesTheSSHKeygenLayout() throws {
        // ssh-keygen writes 70-column-wide base64 lines; any line length must parse.
        let body = sshKeygenKey.components(separatedBy: "\n").dropFirst().dropLast(2).joined()
        let rewrapped = OpenSSHPrivateKey.beginMarker + "\n"
            + stride(from: 0, to: body.count, by: 70).map { offset in
                let start = body.index(body.startIndex, offsetBy: offset)
                return String(body[start..<(body.index(start, offsetBy: 70, limitedBy: body.endIndex) ?? body.endIndex)])
            }.joined(separator: "\n") + "\n" + OpenSSHPrivateKey.endMarker + "\n"
        let parsed = try OpenSSHPrivateKey.parseEd25519(rewrapped)
        #expect(SSHPublicKey.ed25519(parsed.publicKey) == (try SSHPublicKey(openSSHLine: sshKeygenPublic)))
    }

    @Test func refusesEncryptedKeys() throws {
        // Re-encode the sample with an encrypted header, as `ssh-keygen -N <passphrase>` does.
        var data = Data("openssh-key-v1".utf8) + [0]
        data += SSHWire.string("aes256-ctr") + SSHWire.string("bcrypt")
        data += SSHWire.string(SSHWire.string(Data(count: 16)) + SSHWire.uint32(24))
        data += SSHWire.uint32(1) + SSHWire.string(Data(count: 51)) + SSHWire.string(Data(count: 64))
        let text = OpenSSHPrivateKey.beginMarker + "\n" + data.base64EncodedString() + "\n" + OpenSSHPrivateKey.endMarker
        #expect(throws: SSHKeyImportError.passphraseRequired) { try OpenSSHPrivateKey.parseEd25519(text) }
    }

    // A throwaway key made for these tests only (`ssh-keygen -t ed25519 -N test-pass -C fixture`);
    // it is not used anywhere.
    static let protectedKey = """
-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABAoeEI1n7
IbQ25wPYsC7/KeAAAAGAAAAAEAAAAzAAAAC3NzaC1lZDI1NTE5AAAAIENNKIeD6kgKqZqI
YfSP0c0/yM5NUQtxmySCSUUScXACAAAAkE/EbYjWFpd2RloHCB4xbEOLxp8uCfaN6s60+2
eBuQfbmgfQnTtNaR4G47JE7eEzDW2d46ScObeo9VDzBzaebHVEzErcYJFT4Ix1TEgSICQl
2XaTV8CSp+ziPx1Kx5z1C2XbPpo9t2BrIMQqkU/LClBaHBNOemHyizB00PfGwr9ku0Kk0R
DP5OkQd7942FWlcA==
-----END OPENSSH PRIVATE KEY-----
"""
    /// `ssh-keygen -y` of `protectedKey`.
    static let protectedKeyPublicBlob = "AAAAC3NzaC1lZDI1NTE5AAAAIENNKIeD6kgKqZqIYfSP0c0/yM5NUQtxmySCSUUScXAC"

    @Test func decryptsAPassphraseProtectedKey() throws {
        let key = try OpenSSHPrivateKey.parseEd25519(Self.protectedKey, passphrase: "test-pass")
        let blob = try #require(Data(base64Encoded: Self.protectedKeyPublicBlob))
        #expect(blob.suffix(32) == key.publicKey.rawRepresentation)
    }

    @Test func aProtectedKeyAsksForItsPassphraseAndRefusesAWrongOne() {
        #expect(throws: SSHKeyImportError.passphraseRequired) { try OpenSSHPrivateKey.parseEd25519(Self.protectedKey) }
        #expect(throws: SSHKeyImportError.passphraseRequired) {
            try OpenSSHPrivateKey.parseEd25519(Self.protectedKey, passphrase: "")
        }
        #expect(throws: SSHKeyImportError.wrongPassphrase) {
            try OpenSSHPrivateKey.parseEd25519(Self.protectedKey, passphrase: "not-it")
        }
    }

    @Test func refusesCiphersItCannotDecrypt() {
        var data = Data("openssh-key-v1".utf8) + [0]
        data += SSHWire.string("chacha20-poly1305@openssh.com") + SSHWire.string("bcrypt")
        data += SSHWire.string(SSHWire.string(Data(count: 16)) + SSHWire.uint32(24))
        data += SSHWire.uint32(1) + SSHWire.string(Data(count: 51)) + SSHWire.string(Data(count: 64))
        let text = OpenSSHPrivateKey.beginMarker + "\n" + data.base64EncodedString() + "\n" + OpenSSHPrivateKey.endMarker
        #expect(throws: SSHKeyImportError.unsupportedEncryption("chacha20-poly1305@openssh.com")) {
            try OpenSSHPrivateKey.parseEd25519(text, passphrase: "x")
        }
    }

    @Test func refusesOtherKeyTypesAndGarbage() {
        #expect(throws: SSHKeyImportError.unsupportedKeyType("RSA")) {
            try OpenSSHPrivateKey.parseEd25519("-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----")
        }
        #expect(throws: SSHKeyImportError.notOpenSSHKey) { try OpenSSHPrivateKey.parseEd25519(sshKeygenPublic) }
        var truncated = sshKeygenKey.components(separatedBy: "\n")
        truncated.remove(at: 2)
        #expect(throws: SSHKeyImportError.malformed) {
            try OpenSSHPrivateKey.parseEd25519(truncated.joined(separator: "\n"))
        }
        let ecdsaBlob = SSHWire.string("ecdsa-sha2-nistp256") + SSHWire.string("nistp256") + SSHWire.string(Data(count: 65))
        var data = Data("openssh-key-v1".utf8) + [0]
        data += SSHWire.string("none") + SSHWire.string("none") + SSHWire.string(Data())
        data += SSHWire.uint32(1) + SSHWire.string(ecdsaBlob) + SSHWire.string(Data(count: 8))
        let text = OpenSSHPrivateKey.beginMarker + "\n" + data.base64EncodedString() + "\n" + OpenSSHPrivateKey.endMarker
        #expect(throws: SSHKeyImportError.unsupportedKeyType("ECDSA")) { try OpenSSHPrivateKey.parseEd25519(text) }
    }

    @Test func importErrorsNeverEchoKeyMaterial() {
        for error in [
            SSHKeyImportError.notOpenSSHKey, .passphraseRequired, .wrongPassphrase, .unsupportedEncryption("x"),
            .unsupportedKeyType("RSA"), .malformed,
        ] {
            let message = error.localizedDescription
            #expect(!message.contains("b3BlbnNzaC1rZXktdjE"))
            #expect(!message.isEmpty)
        }
    }
}

struct SSHIdentityStoreTests {
    @Test func firstReadGeneratesAndStoresAnIdentity() throws {
        let secrets = InMemorySecretStore()
        let first = try SSHIdentityStore(store: secrets).identity()
        #expect(secrets.count == 1)
        #expect(try secrets.read(account: SSHIdentityStore.account) == first.privateKey.rawRepresentation)
        // A new store on the same Keychain (the next launch) reads the same identity back.
        let again = try SSHIdentityStore(store: secrets).identity()
        #expect(again.privateKey.rawRepresentation == first.privateKey.rawRepresentation)
        #expect(first.authorizedKeyLine.hasPrefix("ssh-ed25519 "))
        #expect(first.authorizedKeyLine.hasSuffix(" purepoint-ipad"))
    }

    @Test func importReplacesTheIdentity() throws {
        let secrets = InMemorySecretStore()
        let store = SSHIdentityStore(store: secrets)
        _ = try store.identity()
        let imported = try store.importOpenSSHPrivateKey(sshKeygenKey)
        #expect(imported.publicKey == (try SSHPublicKey(openSSHLine: sshKeygenPublic)))
        #expect(try store.identity().publicKey == imported.publicKey)
        #expect(try SSHIdentityStore(store: secrets).identity().publicKey == imported.publicKey)
    }

    @Test func refusedImportKeepsTheCurrentIdentity() throws {
        let store = SSHIdentityStore(store: InMemorySecretStore())
        let current = try store.identity()
        #expect(throws: SSHKeyImportError.notOpenSSHKey) { try store.importOpenSSHPrivateKey("not a key") }
        #expect(try store.identity().publicKey == current.publicKey)
    }
}

/// Keychain items by account, behind the calls `KeychainSecretStore` makes.
private final class FakeKeychain: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String: [String: Any]] = [:]
    private var adds = 0
    /// What every update answers when the item exists (a locked device, say).
    var updateFailure: OSStatus?

    var addCount: Int { lock.withLock { adds } }

    func value(_ account: String) -> Data? { lock.withLock { items[account]?[kSecValueData as String] as? Data } }
    func accessible(_ account: String) -> String? {
        lock.withLock { items[account]?[kSecAttrAccessible as String] as? String }
    }

    var calls: KeychainCalls {
        KeychainCalls(
            update: { query, attributes in
                self.lock.withLock {
                    let account = query[kSecAttrAccount as String] as! String
                    guard self.items[account] != nil else { return errSecItemNotFound }
                    if let failure = self.updateFailure { return failure }
                    self.items[account]!.merge(attributes) { _, new in new }
                    return errSecSuccess
                }
            },
            add: { attributes in
                self.lock.withLock {
                    let account = attributes[kSecAttrAccount as String] as! String
                    guard self.items[account] == nil else { return errSecDuplicateItem }
                    self.adds += 1
                    self.items[account] = attributes
                    return errSecSuccess
                }
            })
    }
}

struct KeychainSecretStoreTests {
    @Test func firstWriteAddsTheItemThisDeviceOnly() throws {
        let keychain = FakeKeychain()
        let store = KeychainSecretStore(service: "test", keychain: keychain.calls)
        try store.write(Data([1]), account: "a")
        #expect(keychain.value("a") == Data([1]))
        #expect(keychain.accessible("a") == kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly as String)
        #expect(keychain.addCount == 1)
    }

    @Test func laterWritesUpdateInPlace() throws {
        let keychain = FakeKeychain()
        let store = KeychainSecretStore(service: "test", keychain: keychain.calls)
        try store.write(Data([1]), account: "a")
        try store.write(Data([2]), account: "a")
        #expect(keychain.value("a") == Data([2]))
        #expect(keychain.addCount == 1)
    }

    @Test func aFailedWriteKeepsTheOldValue() throws {
        let keychain = FakeKeychain()
        let store = KeychainSecretStore(service: "test", keychain: keychain.calls)
        try store.write(Data([1]), account: "a")
        keychain.updateFailure = errSecInteractionNotAllowed
        #expect(throws: KeychainError(status: errSecInteractionNotAllowed)) {
            try store.write(Data([2]), account: "a")
        }
        #expect(keychain.value("a") == Data([1]))
        #expect(keychain.addCount == 1)
    }
}

struct HostKeyPinStoreTests {
    private let machine = UUID()
    private let hostKey = SSHPublicKey.ed25519(Curve25519.Signing.PrivateKey().publicKey)
    private let otherKey = SSHPublicKey.ed25519(Curve25519.Signing.PrivateKey().publicKey)

    @Test func firstUseRequiresConfirmation() {
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        #expect(pins.evaluate(hostKey, machineId: machine) == .unconfirmed(hostKey))
        #expect(pins.pinnedKey(machineId: machine) == nil)
    }

    @Test func confirmedKeyMatchesAndPassesAfterward() throws {
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        try pins.pin(hostKey, machineId: machine)
        #expect(pins.evaluate(hostKey, machineId: machine) == .trusted)
        // Pins are per machine: another machine at the same key still asks.
        #expect(pins.evaluate(hostKey, machineId: UUID()) == .unconfirmed(hostKey))
    }

    @Test func changedKeyIsRefusedAndCannotBeRepinnedWithoutReset() throws {
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        try pins.pin(hostKey, machineId: machine)
        #expect(pins.evaluate(otherKey, machineId: machine) == .changed(pinned: hostKey, presented: otherKey))
        #expect(throws: HostKeyPinError.alreadyPinned) { try pins.pin(otherKey, machineId: machine) }
        #expect(pins.evaluate(otherKey, machineId: machine) == .changed(pinned: hostKey, presented: otherKey))
    }

    @Test func resetAllowsRepinning() throws {
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        try pins.pin(hostKey, machineId: machine)
        try pins.reset(machineId: machine)
        #expect(pins.evaluate(otherKey, machineId: machine) == .unconfirmed(otherKey))
        try pins.pin(otherKey, machineId: machine)
        #expect(pins.evaluate(otherKey, machineId: machine) == .trusted)
    }

    @Test func unreadablePinNeverReadsAsUnpinned() throws {
        let secrets = InMemorySecretStore()
        let pins = HostKeyPinStore(store: secrets)
        try pins.pin(hostKey, machineId: machine)
        secrets.failReads = true
        guard case .changed = pins.evaluate(hostKey, machineId: machine) else {
            Issue.record("an unreadable pin must refuse")
            return
        }
    }

    @Test func trustErrorsDescribeBothFingerprints() {
        let changed = SSHTrustError(.changed(pinned: hostKey, presented: otherKey))
        #expect(changed?.localizedDescription.contains(hostKey.fingerprint) == true)
        #expect(changed?.localizedDescription.contains(otherKey.fingerprint) == true)
        #expect(SSHTrustError(.trusted) == nil)
    }
}
#endif
