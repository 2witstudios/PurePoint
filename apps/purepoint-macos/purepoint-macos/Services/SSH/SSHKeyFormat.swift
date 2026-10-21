#if os(iOS)
import Citadel
import CryptoKit
import Foundation

// MARK: - Wire encoding

/// The SSH wire format (RFC 4251 §5) for the few types keys use: `uint32` and length-prefixed
/// `string`.
nonisolated enum SSHWire {
    static func uint32(_ value: UInt32) -> Data {
        Data([UInt8(value >> 24 & 0xFF), UInt8(value >> 16 & 0xFF), UInt8(value >> 8 & 0xFF), UInt8(value & 0xFF)])
    }

    static func string(_ bytes: Data) -> Data {
        uint32(UInt32(bytes.count)) + bytes
    }

    static func string(_ text: String) -> Data {
        string(Data(text.utf8))
    }

    /// Reads values in order; every read past the end throws `SSHKeyImportError.malformed`.
    struct Reader {
        private let bytes: [UInt8]
        private(set) var offset = 0

        init(_ data: Data) {
            bytes = Array(data)
        }

        var isAtEnd: Bool { offset == bytes.count }
        var remaining: Int { bytes.count - offset }

        mutating func uint32() throws -> UInt32 {
            guard remaining >= 4 else { throw SSHKeyImportError.malformed }
            let value = bytes[offset..<offset + 4].reduce(UInt32(0)) { $0 << 8 | UInt32($1) }
            offset += 4
            return value
        }

        mutating func bytes(count: Int) throws -> Data {
            guard count >= 0, remaining >= count else { throw SSHKeyImportError.malformed }
            defer { offset += count }
            return Data(bytes[offset..<offset + count])
        }

        mutating func string() throws -> Data {
            try bytes(count: Int(try uint32()))
        }

        mutating func text() throws -> String {
            guard let text = String(data: try string(), encoding: .utf8) else { throw SSHKeyImportError.malformed }
            return text
        }
    }
}

// MARK: - Public keys

/// An SSH public key as OpenSSH writes it: `<type> <base64 blob> [comment]`. The blob is the
/// key's wire encoding and starts with its own type name.
nonisolated struct SSHPublicKey: Hashable, Sendable {
    static let ed25519Type = "ssh-ed25519"

    let type: String
    let blob: Data

    init(type: String, blob: Data) {
        self.type = type
        self.blob = blob
    }

    /// Parse an OpenSSH public key line (`authorized_keys` / `known_hosts` key form). The
    /// comment, if any, is ignored.
    init(openSSHLine: String) throws {
        let fields = openSSHLine.split(whereSeparator: { $0 == " " || $0 == "\t" })
        guard fields.count >= 2, let blob = Data(base64Encoded: String(fields[1])) else {
            throw SSHKeyImportError.malformed
        }
        var reader = SSHWire.Reader(blob)
        let embedded = try reader.text()
        guard embedded == String(fields[0]) else { throw SSHKeyImportError.malformed }
        self.init(type: embedded, blob: blob)
    }

    static func ed25519(_ key: Curve25519.Signing.PublicKey) -> SSHPublicKey {
        SSHPublicKey(type: ed25519Type, blob: SSHWire.string(ed25519Type) + SSHWire.string(key.rawRepresentation))
    }

    var base64: String { blob.base64EncodedString() }

    /// `<type> <base64>`, without a comment.
    var openSSHLine: String { type + " " + base64 }

    func openSSHLine(comment: String) -> String { openSSHLine + " " + comment }

    /// OpenSSH's fingerprint form: `SHA256:` and the unpadded base64 of the blob's SHA-256.
    var fingerprint: String {
        let digest = Data(SHA256.hash(data: blob)).base64EncodedString()
        return "SHA256:" + digest.trimmingCharacters(in: CharacterSet(charactersIn: "="))
    }

    /// The key type as people name it: `ED25519`, `ECDSA`, `RSA`.
    var displayType: String {
        switch type {
        case Self.ed25519Type: "ED25519"
        case "ssh-rsa": "RSA"
        case let other where other.hasPrefix("ecdsa-sha2-"): "ECDSA"
        case let other: other
        }
    }
}

// MARK: - Private keys

/// Why an imported private key was refused. Messages never include key material.
nonisolated enum SSHKeyImportError: LocalizedError, Equatable {
    case notOpenSSHKey
    /// A passphrase-protected key, imported without its passphrase.
    case passphraseRequired
    case wrongPassphrase
    /// A protected key whose cipher or key derivation PurePoint cannot decrypt.
    case unsupportedEncryption(String)
    case unsupportedKeyType(String)
    case malformed

    var errorDescription: String? {
        switch self {
        case .notOpenSSHKey:
            "This is not an OpenSSH private key. Paste the whole key file, from “-----BEGIN OPENSSH PRIVATE KEY-----” to the END line."
        case .passphraseRequired:
            "This key is protected by a passphrase. Enter it to import the key; PurePoint does not keep the passphrase."
        case .wrongPassphrase:
            "The passphrase is not right for this key."
        case .unsupportedEncryption(let cipher):
            "This key is encrypted with \(cipher), which PurePoint cannot decrypt. Re-protect a copy with “ssh-keygen -p -f <copy>” (OpenSSH's default is aes256-ctr) and import that copy."
        case .unsupportedKeyType(let type):
            "Only Ed25519 keys can be imported; this is a\(type.first.map { "AEIOU".contains($0) } == true ? "n" : "") \(type) key."
        case .malformed:
            "The key is damaged or incomplete."
        }
    }
}

/// The `openssh-key-v1` private key format (OpenSSH `PROTOCOL.key`), for Ed25519: unencrypted,
/// or protected with OpenSSH's bcrypt key derivation and an AES-CTR cipher.
nonisolated enum OpenSSHPrivateKey {
    static let beginMarker = "-----BEGIN OPENSSH PRIVATE KEY-----"
    static let endMarker = "-----END OPENSSH PRIVATE KEY-----"
    private static let magic = Data("openssh-key-v1".utf8) + Data([0])

    /// Ciphers Citadel's OpenSSH key reader decrypts, with `bcrypt` key derivation.
    static let decryptableCiphers: Set<String> = ["aes256-ctr", "aes128-ctr"]

    /// The Ed25519 key in `text`. A protected key needs its `passphrase`, which is used only to
    /// decrypt and is never kept. Refuses other key types and anything that is not exactly one
    /// well-formed OpenSSH private key.
    static func parseEd25519(_ text: String, passphrase: String? = nil) throws -> Curve25519.Signing.PrivateKey {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let begin = trimmed.range(of: beginMarker), let end = trimmed.range(of: endMarker),
            begin.upperBound <= end.lowerBound
        else {
            if let type = pemKeyType(trimmed) { throw SSHKeyImportError.unsupportedKeyType(type) }
            throw SSHKeyImportError.notOpenSSHKey
        }
        let body = trimmed[begin.upperBound..<end.lowerBound].filter { !$0.isWhitespace }
        guard let data = Data(base64Encoded: String(body)), data.starts(with: magic) else {
            throw SSHKeyImportError.malformed
        }
        var reader = SSHWire.Reader(data.dropFirst(magic.count))
        let cipher = try reader.text()
        let kdf = try reader.text()
        _ = try reader.string()  // kdf options
        if cipher != "none" || kdf != "none" {
            return try decryptEd25519(trimmed, cipher: cipher, kdf: kdf, passphrase: passphrase)
        }
        guard try reader.uint32() == 1 else { throw SSHKeyImportError.malformed }
        let publicBlob = try reader.string()
        var publicReader = SSHWire.Reader(publicBlob)
        let publicType = try publicReader.text()
        guard publicType == SSHPublicKey.ed25519Type else {
            throw SSHKeyImportError.unsupportedKeyType(SSHPublicKey(type: publicType, blob: publicBlob).displayType)
        }

        var secret = SSHWire.Reader(try reader.string())
        guard try secret.uint32() == secret.uint32() else { throw SSHKeyImportError.malformed }
        guard try secret.text() == SSHPublicKey.ed25519Type else { throw SSHKeyImportError.malformed }
        let publicKey = try secret.string()
        let privateKey = try secret.string()
        _ = try secret.string()  // comment
        // Padding is 1, 2, 3, … up to the cipher block size.
        var expected: UInt8 = 1
        while !secret.isAtEnd {
            guard try secret.bytes(count: 1).first == expected else { throw SSHKeyImportError.malformed }
            expected += 1
        }
        // The private field is the 32-byte seed followed by the 32-byte public key.
        guard publicKey.count == 32, privateKey.count == 64, privateKey.suffix(32) == publicKey,
            publicBlob == SSHWire.string(SSHPublicKey.ed25519Type) + SSHWire.string(publicKey)
        else { throw SSHKeyImportError.malformed }
        let key: Curve25519.Signing.PrivateKey
        do {
            key = try Curve25519.Signing.PrivateKey(rawRepresentation: privateKey.prefix(32))
        } catch {
            throw SSHKeyImportError.malformed
        }
        guard key.publicKey.rawRepresentation == publicKey else { throw SSHKeyImportError.malformed }
        return key
    }

    /// A protected key, decrypted by Citadel (bcrypt_pbkdf, then AES-CTR, then its check ints).
    private static func decryptEd25519(
        _ text: String, cipher: String, kdf: String, passphrase: String?
    ) throws -> Curve25519.Signing.PrivateKey {
        guard kdf == "bcrypt", decryptableCiphers.contains(cipher) else {
            throw SSHKeyImportError.unsupportedEncryption(kdf == "bcrypt" ? cipher : "\(cipher) (\(kdf))")
        }
        guard let passphrase, !passphrase.isEmpty else { throw SSHKeyImportError.passphraseRequired }
        let decrypted: Curve25519.Signing.PrivateKey
        do {
            decrypted = try Curve25519.Signing.PrivateKey(sshEd25519: text, decryptionKey: Data(passphrase.utf8))
        } catch {
            // A wrong passphrase decrypts to noise, which fails the key's check ints.
            throw SSHKeyImportError.wrongPassphrase
        }
        // Re-validated through the unencrypted reader, so both paths accept the same keys.
        return try parseEd25519(encodeEd25519(decrypted, comment: ""))
    }

    /// `key` in the unencrypted OpenSSH format `ssh-keygen` writes. Used to round-trip the
    /// parser; the app never exports its identity.
    static func encodeEd25519(
        _ key: Curve25519.Signing.PrivateKey, comment: String, checkInt: UInt32 = .random(in: .min ... .max)
    ) -> String {
        let publicKey = key.publicKey.rawRepresentation
        let publicBlob = SSHWire.string(SSHPublicKey.ed25519Type) + SSHWire.string(publicKey)
        var secret = SSHWire.uint32(checkInt) + SSHWire.uint32(checkInt)
        secret += SSHWire.string(SSHPublicKey.ed25519Type)
        secret += SSHWire.string(publicKey)
        secret += SSHWire.string(key.rawRepresentation + publicKey)
        secret += SSHWire.string(comment)
        var pad: UInt8 = 1
        while secret.count % 8 != 0 {
            secret.append(pad)
            pad += 1
        }
        var data = magic
        data += SSHWire.string("none") + SSHWire.string("none") + SSHWire.string(Data())
        data += SSHWire.uint32(1) + SSHWire.string(publicBlob) + SSHWire.string(secret)
        let base64 = data.base64EncodedString(options: [.lineLength64Characters, .endLineWithLineFeed])
        return beginMarker + "\n" + base64 + "\n" + endMarker + "\n"
    }

    /// The key type a non-OpenSSH PEM block names (`-----BEGIN RSA PRIVATE KEY-----`).
    private static func pemKeyType(_ text: String) -> String? {
        for (marker, type) in [("BEGIN RSA PRIVATE KEY", "RSA"), ("BEGIN EC PRIVATE KEY", "ECDSA"),
            ("BEGIN DSA PRIVATE KEY", "DSA")]
        where text.contains(marker) {
            return type
        }
        return nil
    }
}
#endif
