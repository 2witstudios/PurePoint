#if os(iOS)
// iPad client of a Mac's Point Guard bridge (IPAD-011). The device trust record, Keychain
// storage and pinned session come from the iPhone app's shared `PairingSecret.swift`.
import Foundation

/// The shared `PairingSecret.swift` validates endpoints with the iPhone's name for this check.
typealias ConnectionAddress = PiConnectionAddress

/// A Mac's one-time enrollment QR (PGSET-004, QR v1).
nonisolated struct PiEnrollmentCode: Codable, Equatable, Sendable {
    let type: String
    let version: Int
    let endpoint: String
    let hostId: String
    let certificateSHA256: String
    let enrollmentToken: String
    let expiresAt: Int64

    static func parse(_ text: String, now: Date = Date()) -> PiEnrollmentCode? {
        let milliseconds = Int64(now.timeIntervalSince1970 * 1000)
        guard text.utf8.count <= 8192,
            let code = try? JSONDecoder().decode(Self.self, from: Data(text.utf8)),
            code.type == "pi-mobile-pairing", code.version == 1,
            PiConnectionAddress.url(code.endpoint)?.scheme == "wss", UUID(uuidString: code.hostId) != nil,
            code.expiresAt > milliseconds, code.expiresAt <= milliseconds + 300000,
            PiPairedMacs.isPin(code.certificateSHA256), PiPairedMacs.isToken(code.enrollmentToken)
        else { return nil }
        return code
    }

    /// Re-validates a code held since scanning, so an expired QR is never sent.
    func isCurrent(now: Date = Date()) -> Bool {
        guard let data = try? JSONEncoder().encode(self), let text = String(data: data, encoding: .utf8) else {
            return false
        }
        return Self.parse(text, now: now) != nil
    }
}

/// Saved Macs carry no credential, pin or device principal; those stay in the Keychain record.
nonisolated struct PiPairedMac: Codable, Identifiable, Equatable, Sendable {
    var id: String  // the bridge's verified hostId
    var endpoint: String
    var name: String

    static func normalizedName(_ name: String) -> String {
        String(
            name.filter {
                !$0.isNewline && !$0.unicodeScalars.contains(where: { CharacterSet.controlCharacters.contains($0) })
            }
            .trimmingCharacters(in: .whitespacesAndNewlines).prefix(60))
    }
}

nonisolated struct PiPairedMacs: Codable, Equatable, Sendable {
    var version = 1
    var machines: [PiPairedMac] = []
    var selectedId: String?

    var selected: PiPairedMac? { machines.first { $0.id == selectedId } }
    var valid: Bool {
        version == 1 && Set(machines.map(\.id)).count == machines.count
            && machines.allSatisfy {
                UUID(uuidString: $0.id) != nil && PiConnectionAddress.url($0.endpoint)?.scheme == "wss"
                    && !$0.name.isEmpty && $0.name.count <= 60
            }
            && (selectedId == nil || machines.contains { $0.id == selectedId })
    }

    static func defaultName(_ endpoint: String) -> String {
        String((URL(string: endpoint)?.host ?? "Mac").prefix(60))
    }

    /// A re-pair keeps the Mac's name; a Mac that moved address replaces its old entry.
    mutating func remember(hostId: String, endpoint: String) -> PiPairedMac {
        let existing = machines.first { $0.id == hostId }
        let machine = PiPairedMac(id: hostId, endpoint: endpoint, name: existing?.name ?? Self.defaultName(endpoint))
        machines.removeAll { $0.id == hostId || $0.endpoint == endpoint }
        machines.append(machine)
        selectedId = machine.id
        return machine
    }

    mutating func rename(_ id: String, to name: String) {
        let name = PiPairedMac.normalizedName(name)
        guard !name.isEmpty, let index = machines.firstIndex(where: { $0.id == id }) else { return }
        machines[index].name = name
    }

    mutating func forget(_ id: String) {
        machines.removeAll { $0.id == id }
        if selectedId == id { selectedId = machines.last?.id }
    }

    static func load(_ defaults: UserDefaults) -> PiPairedMacs? {
        guard let data = defaults.data(forKey: storageKey) else { return PiPairedMacs() }
        guard data.count <= 1024 * 1024, let decoded = try? JSONDecoder().decode(Self.self, from: data), decoded.valid
        else { return nil }
        return decoded
    }

    func save(_ defaults: UserDefaults) {
        defaults.set(try? JSONEncoder().encode(self), forKey: Self.storageKey)
    }

    static let storageKey = "pointguard.pairedMacs.v1"

    static func isPin(_ value: String) -> Bool {
        value.count == 64 && value.utf8.allSatisfy { (48...57).contains($0) || (97...102).contains($0) }
    }
    static func isToken(_ value: String) -> Bool {
        value.utf8.count == 43
            && value.utf8.allSatisfy {
                (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95
            }
    }
}

/// The bridge's pairing HTTPS routes and the decisions taken on their results.
nonisolated enum PiBridgeTrust {
    /// The name the Mac lists this device under in Point Guard setup.
    static let deviceName = "iPad"
    /// The bridge closes a revoked or rotated device's socket with this code.
    static let revokedCloseCode = 4001

    /// `/pair/enroll` and `/pair/verify` live on the same host and port as the `/v1` WSS endpoint.
    static func httpURL(_ endpoint: String, path: String) -> URL? {
        guard PiConnectionAddress.url(endpoint)?.scheme == "wss", var components = URLComponents(string: endpoint)
        else { return nil }
        components.scheme = "https"; components.path = path
        return components.url
    }

    /// The WSS upgrade carries the device credential and the bridge-issued client id.
    static func socketRequest(url: URL, credential: String, clientId: String) -> URLRequest {
        var request = URLRequest(url: url)
        request.setValue("Bearer " + credential, forHTTPHeaderField: "Authorization")
        request.setValue(clientId, forHTTPHeaderField: "X-PointGuard-Client-ID")
        return request
    }

    /// A revoked device or a changed certificate stops reconnecting until a deliberate re-pair.
    static func requiresRepair(closeCode: Int, identityRejected: Bool) -> Bool {
        closeCode == revokedCloseCode || identityRejected
    }

    struct EnrolledDevice: Decodable { let version: Int; let hostId: String; let deviceId: String; let clientId: String; let credential: String }
    struct VerifiedDevice: Decodable { let version: Int; let hostId: String; let deviceId: String; let clientId: String }

    /// Turns an enrollment response into the trust record, refusing anything not bound to the QR.
    static func trust(enrolled data: Data, status: Int?, code: PiEnrollmentCode) throws -> TrustedHost {
        guard status == 200 else { throw TrustFailure("Enrollment expired, used or revoked. Create a new Mac QR code.") }
        let device = try JSONDecoder().decode(EnrolledDevice.self, from: data)
        guard device.version == 1, device.hostId == code.hostId else {
            throw TrustFailure("Mac identity changed. Scan a new QR code deliberately.")
        }
        let saved = TrustedHost(
            version: 1, endpoint: code.endpoint, hostId: code.hostId, certificateSHA256: code.certificateSHA256,
            deviceId: device.deviceId, clientId: device.clientId, credential: device.credential)
        guard saved.valid else { throw TrustFailure("The Mac returned mismatched device trust. Scan a new QR code.") }
        return saved
    }

    /// Checks `/pair/verify` before the credential is offered to the WSS endpoint. 401 and 403 mean
    /// this device was revoked or lost its trust, so retrying cannot help.
    static func verify(_ data: Data, status: Int?, saved: TrustedHost) throws {
        if status == 401 || status == 403 {
            throw TrustFailure("This iPad’s trust was revoked or lost. Scan a new Mac QR code deliberately.")
        }
        guard status == 200 else { throw MobileError("Mac trust verification is unavailable.") }
        let verified = try JSONDecoder().decode(VerifiedDevice.self, from: data)
        guard verified.version == 1, verified.hostId == saved.hostId, verified.deviceId == saved.deviceId,
            verified.clientId == saved.clientId
        else { throw TrustFailure("Saved Mac identity changed. Scan a new QR code deliberately.") }
    }
}

/// Network seam for pairing, so the model's re-pair transitions are testable without a Mac.
nonisolated struct PiBridgeClient: Sendable {
    var enroll: @Sendable (PiEnrollmentCode, URLSession) async throws -> TrustedHost
    var verify: @Sendable (TrustedHost, URLSession) async throws -> Void

    static let live = PiBridgeClient(
        enroll: { code, session in
            guard let url = PiBridgeTrust.httpURL(code.endpoint, path: "/pair/enroll") else {
                throw TrustFailure("This QR has an invalid Mac address. Create a new code on your Mac.")
            }
            var request = URLRequest(url: url)
            request.httpMethod = "POST"; request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: [
                "version": 1, "enrollmentToken": code.enrollmentToken, "name": PiBridgeTrust.deviceName,
            ])
            let (data, response) = try await session.data(for: request)
            return try PiBridgeTrust.trust(enrolled: data, status: (response as? HTTPURLResponse)?.statusCode, code: code)
        },
        verify: { saved, session in
            guard let url = PiBridgeTrust.httpURL(saved.endpoint, path: "/pair/verify") else {
                throw TrustFailure("Saved Mac trust is invalid. Scan a new QR code deliberately.")
            }
            var request = URLRequest(url: url)
            request.setValue("Bearer " + saved.credential, forHTTPHeaderField: "Authorization")
            let (data, response) = try await session.data(for: request)
            try PiBridgeTrust.verify(data, status: (response as? HTTPURLResponse)?.statusCode, saved: saved)
        })
}

/// Keychain access for trust records, replaceable in tests.
nonisolated struct PiTrustStore: Sendable {
    var load: @Sendable (String) throws -> TrustedHost?
    var save: @Sendable (TrustedHost) throws -> Void
    var remove: @Sendable (String) throws -> Void
    var begin: @Sendable (String) throws -> Void = { _ in }
    var finish: @Sendable (String) throws -> Void = { _ in }
    var backup: (@Sendable (String) throws -> (@Sendable () throws -> Void))? = nil

    static let keychain = PiTrustStore(
        load: { try PairingSecret.readTrust(endpoint: $0) }, save: { try PairingSecret.saveTrust($0) },
        remove: { try PairingSecret.removeTrust(endpoint: $0) }, begin: { try PairingSecret.beginTrustChange(endpoint: $0) },
        finish: { try PairingSecret.finishTrustChange(endpoint: $0) }, backup: { try PairingSecret.backupTrust(endpoint: $0) })

    func writer() -> PairingTrustWriter {
        PairingTrustWriter(save: save, load: load, remove: remove, begin: begin, finish: finish, backup: backup)
    }
}
#endif
