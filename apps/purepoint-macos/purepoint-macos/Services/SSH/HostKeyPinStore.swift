#if os(iOS)
import Foundation
import NIOCore
import NIOSSH

/// What a presented host key means for a machine (IPAD-009).
nonisolated enum HostKeyVerdict: Equatable, Sendable {
    /// The key matches the machine's pin.
    case trusted
    /// No pin yet: the user must confirm this key before authentication proceeds.
    case unconfirmed(SSHPublicKey)
    /// The key differs from the pin. Refused until the pin is deliberately reset.
    case changed(pinned: SSHPublicKey, presented: SSHPublicKey)
}

/// A host key the connection refused, carried to the UI so the user can compare and decide.
nonisolated enum SSHTrustError: LocalizedError, Equatable {
    case hostKeyUnconfirmed(SSHPublicKey)
    case hostKeyChanged(pinned: SSHPublicKey, presented: SSHPublicKey)

    init?(_ verdict: HostKeyVerdict) {
        switch verdict {
        case .trusted: return nil
        case .unconfirmed(let key): self = .hostKeyUnconfirmed(key)
        case .changed(let pinned, let presented): self = .hostKeyChanged(pinned: pinned, presented: presented)
        }
    }

    var errorDescription: String? {
        switch self {
        case .hostKeyUnconfirmed(let key):
            "Confirm the host key before connecting (\(key.displayType) \(key.fingerprint))."
        case .hostKeyChanged(let pinned, let presented):
            "The host key changed. Expected \(pinned.displayType) \(pinned.fingerprint), got \(presented.displayType) \(presented.fingerprint). Reset the host key only if you know why it changed."
        }
    }
}

nonisolated enum HostKeyPinError: LocalizedError, Equatable {
    /// Re-pinning a machine whose pin differs needs a deliberate reset first.
    case alreadyPinned

    var errorDescription: String? {
        "This machine already trusts a different host key. Reset the host key first."
    }
}

/// One pinned host key per machine, in the Keychain (`PurePoint.HostKeyPin.v1` /
/// `<machine id>`, the OpenSSH public key line). Trust on first use, after explicit
/// confirmation; a different key is refused until `reset`.
nonisolated final class HostKeyPinStore: @unchecked Sendable {
    static let service = "PurePoint.HostKeyPin.v1"

    static let shared = HostKeyPinStore(store: KeychainSecretStore(service: service))

    private let store: any SecretStore
    private let lock = NSLock()

    init(store: any SecretStore) {
        self.store = store
    }

    private static func account(_ machineId: UUID) -> String { machineId.uuidString.lowercased() }

    /// The pinned key. An unreadable pin reads as present-but-unmatchable, never as absent:
    /// treating it as absent would let the next key be confirmed without a reset.
    func pinnedKey(machineId: UUID) -> SSHPublicKey? {
        lock.withLock { readPin(machineId) }
    }

    func evaluate(_ presented: SSHPublicKey, machineId: UUID) -> HostKeyVerdict {
        lock.withLock {
            guard let pinned = readPin(machineId) else { return .unconfirmed(presented) }
            return pinned == presented ? .trusted : .changed(pinned: pinned, presented: presented)
        }
    }

    /// Pin a key the user confirmed. Pinning the key already pinned is a no-op; a different
    /// pin must be reset first.
    func pin(_ key: SSHPublicKey, machineId: UUID) throws {
        try lock.withLock {
            if let pinned = readPin(machineId) {
                guard pinned == key else { throw HostKeyPinError.alreadyPinned }
                return
            }
            try store.write(Data(key.openSSHLine.utf8), account: Self.account(machineId))
        }
    }

    /// Forget the pin (Reset Host Key, or the machine was removed). The next connection asks
    /// for confirmation again.
    func reset(machineId: UUID) throws {
        try lock.withLock { try store.delete(account: Self.account(machineId)) }
    }

    private func readPin(_ machineId: UUID) -> SSHPublicKey? {
        let data: Data?
        do {
            data = try store.read(account: Self.account(machineId))
        } catch {
            return Self.unreadablePin
        }
        guard let data else { return nil }
        return (try? SSHPublicKey(openSSHLine: String(decoding: data, as: UTF8.self))) ?? Self.unreadablePin
    }

    /// Stands in for a pin that exists but cannot be read; it matches no real key.
    private static let unreadablePin = SSHPublicKey(type: "unreadable-pin", blob: Data())
}

/// Citadel's host-key hook: checks the presented key against the machine's pin before
/// authentication and refuses anything but a match. The verdict is kept so the caller can
/// show the key that was refused, whatever error NIOSSH reports for the failed handshake.
nonisolated final class PinningHostKeyValidator: NIOSSHClientServerAuthenticationDelegate, @unchecked Sendable {
    let machineId: UUID
    private let pins: HostKeyPinStore
    private let lock = NSLock()
    private var lastVerdict: HostKeyVerdict?

    init(machineId: UUID, pins: HostKeyPinStore) {
        self.machineId = machineId
        self.pins = pins
    }

    /// The verdict of the most recent handshake, nil before one presented a key.
    var verdict: HostKeyVerdict? { lock.withLock { lastVerdict } }

    func validateHostKey(hostKey: NIOSSHPublicKey, validationCompletePromise: EventLoopPromise<Void>) {
        guard let key = try? SSHPublicKey(openSSHLine: String(openSSHPublicKey: hostKey)) else {
            validationCompletePromise.fail(SSHKeyImportError.malformed)
            return
        }
        let verdict = pins.evaluate(key, machineId: machineId)
        lock.withLock { lastVerdict = verdict }
        if let refusal = SSHTrustError(verdict) {
            validationCompletePromise.fail(refusal)
        } else {
            validationCompletePromise.succeed(())
        }
    }
}
#endif
