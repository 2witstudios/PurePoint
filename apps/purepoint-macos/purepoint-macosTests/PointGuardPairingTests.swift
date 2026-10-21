#if os(iOS)
import Foundation
import Security
import Testing

@testable import PurePoint

// MARK: - Fixtures

private let endpoint = "wss://100.100.1.2:8787/v1"
private let hostId = "0E6A2B1C-6E4B-4F0B-9A55-1D2C3B4A5F60"
private let deviceId = "7A1B2C3D-4E5F-4A6B-8C7D-9E0F1A2B3C4D"
private let pin = String(repeating: "a", count: 64)
private let token = String(repeating: "t", count: 43)
private let credential = String(repeating: "c", count: 43)

private func codeJSON(
    endpoint: String = endpoint, hostId: String = hostId, pin: String = pin, token: String = token,
    expiresIn seconds: Double = 120, now: Date = Date()
) -> String {
    let expires = Int64((now.timeIntervalSince1970 + seconds) * 1000)
    return """
        {"type":"pi-mobile-pairing","version":1,"endpoint":"\(endpoint)","hostId":"\(hostId)",\
        "certificateSHA256":"\(pin)","enrollmentToken":"\(token)","expiresAt":\(expires)}
        """
}

private func trusted(endpoint: String = endpoint, credential: String = credential) -> TrustedHost {
    TrustedHost(
        version: 1, endpoint: endpoint, hostId: hostId, certificateSHA256: pin, deviceId: deviceId,
        clientId: deviceId, credential: credential)
}

private func json(_ object: [String: Any]) -> Data { try! JSONSerialization.data(withJSONObject: object) }

/// In-memory Keychain stand-in for model tests.
private final class MemoryTrust: @unchecked Sendable {
    private let lock = NSLock()
    private var records: [String: TrustedHost] = [:]
    subscript(endpoint: String) -> TrustedHost? {
        get { lock.lock(); defer { lock.unlock() }; return records[endpoint] }
        set { lock.lock(); records[endpoint] = newValue; lock.unlock() }
    }
    var store: PiTrustStore {
        PiTrustStore(
            load: { [self] in self[$0] }, save: { [self] in self[$0.endpoint] = $0 },
            remove: { [self] in self[$0] = nil })
    }
}

/// Records what the model sent to the fake bridge.
private final class BridgeLog: @unchecked Sendable {
    private let lock = NSLock()
    private var _verified: [TrustedHost] = []
    private var _enrolled: [PiEnrollmentCode] = []
    var verified: [TrustedHost] { lock.lock(); defer { lock.unlock() }; return _verified }
    var enrolled: [PiEnrollmentCode] { lock.lock(); defer { lock.unlock() }; return _enrolled }
    func verify(_ host: TrustedHost) { lock.lock(); _verified.append(host); lock.unlock() }
    func enroll(_ code: PiEnrollmentCode) { lock.lock(); _enrolled.append(code); lock.unlock() }
}

@MainActor
private func waitUntil(_ condition: @MainActor () -> Bool) async {
    for _ in 0..<300 where !condition() { try? await Task.sleep(nanoseconds: 10_000_000) }
}

@MainActor
private func makeModel(trust: MemoryTrust, log: BridgeLog, verify: @escaping @Sendable (TrustedHost) throws -> Void)
    -> PiChatModel
{
    let model = PiChatModel(defaults: UserDefaults(suiteName: "PointGuardPairingTests." + UUID().uuidString)!)
    model.trustStore = trust.store
    model.bridge = PiBridgeClient(
        enroll: { code, _ in
            log.enroll(code)
            return TrustedHost(
                version: 1, endpoint: code.endpoint, hostId: code.hostId, certificateSHA256: code.certificateSHA256,
                deviceId: deviceId, clientId: deviceId, credential: credential)
        },
        verify: { host, _ in log.verify(host); try verify(host) })
    return model
}

// MARK: - Enrollment code

@Suite("Point Guard enrollment code")
struct PointGuardEnrollmentCodeTests {
    @Test func parsesCurrentQR() throws {
        let code = try #require(PiEnrollmentCode.parse(codeJSON()))
        #expect(code.endpoint == endpoint)
        #expect(code.hostId == hostId)
        #expect(code.isCurrent())
    }

    @Test func rejectsExpiredOrFarFutureCodes() {
        #expect(PiEnrollmentCode.parse(codeJSON(expiresIn: -1)) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(expiresIn: 301)) == nil)
        let code = PiEnrollmentCode.parse(codeJSON(expiresIn: 5))!
        #expect(!code.isCurrent(now: Date().addingTimeInterval(10)))
    }

    @Test func requiresPinnedWSSOnTheTailnet() {
        #expect(PiEnrollmentCode.parse(codeJSON(endpoint: "ws://100.100.1.2:8787/v1")) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(endpoint: "wss://example.com:8787/v1")) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(endpoint: "wss://100.100.1.2:8787/other")) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(endpoint: "wss://mac.tail1234.ts.net:8787/v1")) != nil)
    }

    @Test func rejectsMalformedIdentityPinOrToken() {
        #expect(PiEnrollmentCode.parse(codeJSON(hostId: "not-a-uuid")) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(pin: String(repeating: "A", count: 64))) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(pin: String(repeating: "a", count: 63))) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(token: String(repeating: "t", count: 42))) == nil)
        #expect(PiEnrollmentCode.parse(codeJSON(token: String(repeating: "+", count: 43))) == nil)
        #expect(PiEnrollmentCode.parse(#"{"type":"pi-mobile-pairing","version":1,"endpoint":"ws://100.100.1.2:8787/v1","secret":"x"}"#) == nil)
    }
}

// MARK: - Bridge trust decisions

@Suite("Point Guard bridge trust")
struct PointGuardBridgeTrustTests {
    @Test func pairingRoutesShareTheWSSHostAndPort() {
        #expect(PiBridgeTrust.httpURL(endpoint, path: "/pair/verify")?.absoluteString == "https://100.100.1.2:8787/pair/verify")
        #expect(PiBridgeTrust.httpURL(endpoint, path: "/pair/enroll")?.absoluteString == "https://100.100.1.2:8787/pair/enroll")
        #expect(PiBridgeTrust.httpURL("ws://100.100.1.2:8787/v1", path: "/pair/verify") == nil)
    }

    @Test func socketUpgradePresentsTheDeviceCredentialAndClientId() throws {
        let url = try #require(PiConnectionAddress.url(endpoint))
        let request = PiBridgeTrust.socketRequest(url: url, credential: credential, clientId: deviceId)
        #expect(request.url?.absoluteString == endpoint)
        #expect(request.value(forHTTPHeaderField: "Authorization") == "Bearer " + credential)
        #expect(request.value(forHTTPHeaderField: "X-PointGuard-Client-ID") == deviceId)
    }

    @Test func revokedCloseCodeOrPinMismatchRequiresRepair() {
        #expect(PiBridgeTrust.requiresRepair(closeCode: 4001, identityRejected: false))
        #expect(PiBridgeTrust.requiresRepair(closeCode: 1006, identityRejected: true))
        #expect(!PiBridgeTrust.requiresRepair(closeCode: 1006, identityRejected: false))
        #expect(!PiBridgeTrust.requiresRepair(closeCode: 1001, identityRejected: false))
    }

    @Test func enrollmentResponseBecomesTrustBoundToTheQR() throws {
        let code = PiEnrollmentCode.parse(codeJSON())!
        let body = json(["version": 1, "hostId": hostId, "deviceId": deviceId, "clientId": deviceId, "credential": credential])
        let saved = try PiBridgeTrust.trust(enrolled: body, status: 200, code: code)
        #expect(saved.valid)
        #expect(saved.endpoint == endpoint && saved.certificateSHA256 == pin && saved.credential == credential)

        #expect(throws: TrustFailure.self) { try PiBridgeTrust.trust(enrolled: body, status: 401, code: code) }
        let otherHost = json(["version": 1, "hostId": UUID().uuidString, "deviceId": deviceId, "clientId": deviceId, "credential": credential])
        #expect(throws: TrustFailure.self) { try PiBridgeTrust.trust(enrolled: otherHost, status: 200, code: code) }
        let badCredential = json(["version": 1, "hostId": hostId, "deviceId": deviceId, "clientId": deviceId, "credential": "short"])
        #expect(throws: TrustFailure.self) { try PiBridgeTrust.trust(enrolled: badCredential, status: 200, code: code) }
    }

    @Test func verifySeparatesRevocationFromOutages() throws {
        let saved = trusted()
        let ok = json(["version": 1, "hostId": hostId, "deviceId": deviceId, "clientId": deviceId])
        try PiBridgeTrust.verify(ok, status: 200, saved: saved)
        #expect(throws: TrustFailure.self) { try PiBridgeTrust.verify(Data(), status: 401, saved: saved) }
        #expect(throws: TrustFailure.self) { try PiBridgeTrust.verify(Data(), status: 403, saved: saved) }
        #expect(throws: MobileError.self) { try PiBridgeTrust.verify(Data(), status: 503, saved: saved) }
        let moved = json(["version": 1, "hostId": hostId, "deviceId": UUID().uuidString, "clientId": deviceId])
        #expect(throws: TrustFailure.self) { try PiBridgeTrust.verify(moved, status: 200, saved: saved) }
    }

    @Test func iPadStringsNameTheDevice() {
        #expect(PiBridgeTrust.deviceName == "iPad")
        #expect(PiChatModel.localDevice == "iPad")
        // The iPhone's shared Keychain messages name the iPad in this target only.
        #expect(PairingSecret.device == "iPad")
        do {
            try PiBridgeTrust.verify(Data(), status: 401, saved: trusted())
            Issue.record("A revoked device must not verify")
        } catch {
            #expect(error.localizedDescription == "This iPad’s trust was revoked or lost. Scan a new Mac QR code deliberately.")
        }
    }
}

// MARK: - Saved Macs

@Suite("Point Guard saved Macs")
struct PointGuardSavedMacsTests {
    @Test func rememberSelectsAndKeepsNamesAcrossRepair() {
        var macs = PiPairedMacs()
        let first = macs.remember(hostId: hostId, endpoint: endpoint)
        #expect(first.name == "100.100.1.2" && macs.selectedId == hostId)
        macs.rename(hostId, to: "  Studio\nMac  ")
        #expect(macs.selected?.name == "StudioMac")
        // The same Mac re-paired at a new address keeps its name and replaces its entry.
        let moved = macs.remember(hostId: hostId, endpoint: "wss://100.100.1.3:8787/v1")
        #expect(moved.name == "StudioMac" && macs.machines.count == 1)
        let other = UUID().uuidString
        _ = macs.remember(hostId: other, endpoint: endpoint)
        #expect(macs.machines.count == 2 && macs.selectedId == other && macs.valid)
    }

    @Test func forgetFallsBackToAnotherMac() {
        var macs = PiPairedMacs()
        _ = macs.remember(hostId: hostId, endpoint: endpoint)
        let other = UUID().uuidString
        _ = macs.remember(hostId: other, endpoint: "wss://100.100.1.3:8787/v1")
        macs.forget(other)
        #expect(macs.selectedId == hostId)
        macs.forget(hostId)
        #expect(macs.selectedId == nil && macs.machines.isEmpty && macs.valid)
    }

    @Test func storageRoundTripsAndRejectsTampering() throws {
        let defaults = UserDefaults(suiteName: "PointGuardSavedMacsTests." + UUID().uuidString)!
        #expect(PiPairedMacs.load(defaults) == PiPairedMacs())
        var macs = PiPairedMacs()
        _ = macs.remember(hostId: hostId, endpoint: endpoint)
        macs.save(defaults)
        #expect(PiPairedMacs.load(defaults) == macs)
        // Saved Macs hold no credential or pin; those live only in the Keychain record.
        let stored = String(data: try #require(defaults.data(forKey: PiPairedMacs.storageKey)), encoding: .utf8)!
        #expect(!stored.contains("credential") && !stored.contains("certificate"))
        defaults.set(Data("{\"version\":1,\"machines\":[{\"id\":\"x\",\"endpoint\":\"ws://evil/v1\",\"name\":\"x\"}]}".utf8), forKey: PiPairedMacs.storageKey)
        #expect(PiPairedMacs.load(defaults) == nil)
    }
}

// MARK: - Certificate pinning

/// A self-signed P-256 certificate for 100.100.1.2, valid until 2126.
private let certificateDER = Data(
    base64Encoded:
        "MIIBlDCCATqgAwIBAgIUZJaECjIn6+xvdcJeY3AuN13U0aEwCgYIKoZIzj0EAwIwFjEUMBIGA1UEAwwLMTAwLjEwMC4xLjIwIBcNMjYxMDIxMDYxMzQ0WhgPMjEyNjA5MjcwNjEzNDRaMBYxFDASBgNVBAMMCzEwMC4xMDAuMS4yMFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE5lZQcFZy8JpuSwADYzd3Wn8IcdNJJ/fBw1ZXW+N5UsKT3EU7M9csOcV1PE3YXKNP7/yjmV4ds3/CcOGVsYceRqNkMGIwHQYDVR0OBBYEFBEIzFEsObUeNgv6QCxtXEKUETjKMB8GA1UdIwQYMBaAFBEIzFEsObUeNgv6QCxtXEKUETjKMA8GA1UdEwEB/wQFMAMBAf8wDwYDVR0RBAgwBocEZGQBAjAKBggqhkjOPQQDAgNIADBFAiEAqF6L2dTvgQh1tWp4ArdXi7huzc37PI088xD38uqe0RYCIDrz1NbuaeK72u4iKezWE5WuwtxtNIxPPLRfyzpsIZP0"
)!
private let certificatePin = "14513a36518365314811164430276798612b7910e685c90803f086953976db22"

private final class TrustProtectionSpace: URLProtectionSpace, @unchecked Sendable {
    let trust: SecTrust
    init(host: String, trust: SecTrust) {
        self.trust = trust
        super.init(
            host: host, port: 8787, protocol: "https", realm: nil,
            authenticationMethod: NSURLAuthenticationMethodServerTrust)
    }
    required init?(coder: NSCoder) { nil }
    override var serverTrust: SecTrust? { trust }
}

private final class IgnoringSender: NSObject, URLAuthenticationChallengeSender {
    func use(_ credential: URLCredential, for challenge: URLAuthenticationChallenge) {}
    func continueWithoutCredential(for challenge: URLAuthenticationChallenge) {}
    func cancel(_ challenge: URLAuthenticationChallenge) {}
}

private func challenge(host: String) -> URLAuthenticationChallenge {
    let certificate = SecCertificateCreateWithData(nil, certificateDER as CFData)!
    var trust: SecTrust?
    SecTrustCreateWithCertificates(certificate, SecPolicyCreateSSL(true, host as CFString), &trust)
    return URLAuthenticationChallenge(
        protectionSpace: TrustProtectionSpace(host: host, trust: trust!), proposedCredential: nil,
        previousFailureCount: 0, failureResponse: nil, error: nil, sender: IgnoringSender())
}

private func disposition(_ session: PinnedHostSession, host: String) -> URLSession.AuthChallengeDisposition {
    var result: URLSession.AuthChallengeDisposition?
    session.urlSession(URLSession.shared, didReceive: challenge(host: host)) { disposition, _ in result = disposition }
    return result!
}

@Suite("Point Guard certificate pinning")
struct PointGuardPinningTests {
    @Test func pinnedCertificateIsTheOnlyTrustAnchor() {
        let session = PinnedHostSession(endpoint: endpoint, certificateSHA256: certificatePin)
        #expect(disposition(session, host: "100.100.1.2") == .useCredential)
        #expect(!session.identityRejected)
    }

    @Test func pinMismatchCancelsBeforeTheCredentialIsSent() {
        let session = PinnedHostSession(endpoint: endpoint, certificateSHA256: pin)
        #expect(disposition(session, host: "100.100.1.2") == .cancelAuthenticationChallenge)
        #expect(session.identityRejected)
    }

    @Test func challengeFromAnotherHostIsRefused() {
        let session = PinnedHostSession(endpoint: endpoint, certificateSHA256: certificatePin)
        #expect(disposition(session, host: "100.100.1.9") == .cancelAuthenticationChallenge)
    }
}

// MARK: - Model transitions

@MainActor
@Suite("Point Guard paired connection")
struct PointGuardPairedConnectionTests {
    @Test func pairingStoresTrustSavesTheMacAndVerifiesWithTheBearerCredential() async {
        let trust = MemoryTrust()
        let log = BridgeLog()
        let model = makeModel(trust: trust, log: log) { _ in throw MobileError("Mac trust verification is unavailable.") }
        model.pair(PiEnrollmentCode.parse(codeJSON())!)
        await waitUntil { !log.verified.isEmpty }
        #expect(log.enrolled.first?.enrollmentToken == token)
        #expect(trust[endpoint]?.credential == credential)
        #expect(model.pairedMacs.selected?.id == hostId)
        #expect(model.endpoint == endpoint)
        #expect(!model.pairing && model.pairingError == nil)
        // The stored credential and pin are what the connection presents to the paired Mac.
        #expect(log.verified.first?.credential == credential)
        #expect(log.verified.first?.certificateSHA256 == pin)
    }

    @Test func outageRetriesWithoutRequiringRepair() async {
        let trust = MemoryTrust()
        trust[endpoint] = trusted()
        let log = BridgeLog()
        let model = makeModel(trust: trust, log: log) { _ in throw MobileError("Mac trust verification is unavailable.") }
        model.pair(PiEnrollmentCode.parse(codeJSON())!)
        await waitUntil { model.connectionStatus.hasPrefix("Connection lost") }
        #expect(model.connectionStatus == "Connection lost · Point Guard continues on your Mac")
        #expect(!model.repairRequired)
        #expect(model.error?.hasSuffix("Check PurePoint on your Mac and Tailscale.") == true)
        model.disconnect()
    }

    @Test func revokedDeviceStopsReconnectingUntilRepaired() async {
        let trust = MemoryTrust()
        let log = BridgeLog()
        let model = makeModel(trust: trust, log: log) { _ in
            throw TrustFailure("This iPad’s trust was revoked or lost. Scan a new Mac QR code deliberately.")
        }
        model.pair(PiEnrollmentCode.parse(codeJSON())!)
        await waitUntil { model.repairRequired }
        #expect(model.connectionStatus == "Re-pair with your Mac")
        #expect(model.error == "This iPad’s trust was revoked or lost. Scan a new Mac QR code deliberately.")
        #expect(!model.connected)
        // No retry is scheduled: verification is not attempted again on its own.
        let attempts = log.verified.count
        try? await Task.sleep(nanoseconds: 2_500_000_000)
        #expect(log.verified.count == attempts)
    }

    @Test func missingTrustRequiresRepair() async {
        let trust = MemoryTrust()
        let log = BridgeLog()
        // Verification stays offline so no socket is opened to the fixture address.
        let model = makeModel(trust: trust, log: log) { _ in throw MobileError("offline") }
        model.pair(PiEnrollmentCode.parse(codeJSON())!)
        await waitUntil { !log.verified.isEmpty || model.repairRequired }
        // Losing the Keychain record behaves like a revocation.
        trust[endpoint] = nil
        model.disconnect(); model.connect()
        await waitUntil { model.repairRequired }
        #expect(model.repairRequired)
        #expect(model.error == "No saved device trust. Scan a new Mac QR code.")
    }

    @Test func freshPairingClearsRepairAndSelectingSwitchesMacs() async {
        let trust = MemoryTrust()
        let log = BridgeLog()
        let model = makeModel(trust: trust, log: log) { _ in throw TrustFailure("revoked") }
        model.pair(PiEnrollmentCode.parse(codeJSON())!)
        await waitUntil { model.repairRequired }
        model.bridge.verify = { host, _ in log.verify(host); throw MobileError("offline") }
        let other = UUID().uuidString
        let otherEndpoint = "wss://100.100.1.3:8787/v1"
        model.pair(PiEnrollmentCode.parse(codeJSON(endpoint: otherEndpoint, hostId: other))!)
        await waitUntil { model.pairedMacs.selectedId == other }
        #expect(!model.repairRequired)
        #expect(model.endpoint == otherEndpoint && model.pairedMacs.machines.count == 2)
        model.selectMac(hostId)
        #expect(model.endpoint == endpoint)
        model.renameMac(hostId, name: "Studio")
        #expect(model.pairedMacs.selected?.name == "Studio")
        model.forgetMac(hostId)
        #expect(trust[endpoint] == nil)
        #expect(model.pairedMacs.selectedId == other && model.endpoint == otherEndpoint)
        model.disconnect()
    }

    @Test func revokedSocketCloseRequiresRepairAndStopsRetrying() async {
        let trust = MemoryTrust()
        let log = BridgeLog()
        let model = makeModel(trust: trust, log: log) { _ in throw MobileError("Mac trust verification is unavailable.") }
        model.pair(PiEnrollmentCode.parse(codeJSON())!)
        await waitUntil { model.connectionStatus.hasPrefix("Connection lost") }
        // An ordinary drop keeps retrying.
        model.socketFailed(closeCode: 1006, reason: "The network connection was lost.")
        #expect(!model.repairRequired && model.connectionStatus.hasPrefix("Connection lost"))
        // The bridge closes a revoked or rotated device with 4001.
        model.socketFailed(closeCode: PiBridgeTrust.revokedCloseCode, reason: "closed")
        #expect(model.repairRequired && !model.connected)
        #expect(model.connectionStatus == "Re-pair with your Mac")
        #expect(model.error == "Mac trust changed or this iPad was revoked. Scan a new QR code deliberately.")
        let attempts = log.verified.count
        // Neither the retry scheduled by the earlier drop nor reopening Point Guard reconnects it.
        model.connect()
        try? await Task.sleep(nanoseconds: 2_500_000_000)
        #expect(log.verified.count == attempts && model.repairRequired)
    }

    @Test func expiredCodeIsNeverSent() {
        let log = BridgeLog()
        let model = makeModel(trust: MemoryTrust(), log: log) { _ in }
        let past = Date().addingTimeInterval(-60)
        model.pair(PiEnrollmentCode.parse(codeJSON(expiresIn: 1, now: past), now: past)!)
        #expect(model.pairingError == "This QR expired. Create a new code on your Mac.")
        #expect(!model.pairing && log.enrolled.isEmpty)
    }
}
#endif
