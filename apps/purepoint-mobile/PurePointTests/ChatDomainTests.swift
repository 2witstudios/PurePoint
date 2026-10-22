import XCTest
import Security
@testable import PurePoint
final class ChatDomainTests: XCTestCase {
    func testGivenSavedTrustShouldSurviveKeychainReadAndAtomicCredentialUpdate() throws {
        let endpoint = "wss://fixture-" + UUID().uuidString.lowercased() + ".ts.net/v1"
        // Synthetic unique account only; never touch owner endpoints or legacy trust.
        defer { SecItemDelete([kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "PiMobile.TrustedHost.v1", kSecAttrAccount as String: endpoint] as CFDictionary) }
        let host = UUID().uuidString; let device = UUID().uuidString
        let first = TrustedHost(version: 1, endpoint: endpoint, hostId: host, certificateSHA256: String(repeating: "a", count: 64), deviceId: device, clientId: device, credential: String(repeating: "b", count: 43))
        try PairingSecret.saveTrust(first)
        XCTAssertEqual(try PairingSecret.readTrust(endpoint: endpoint)?.credential, first.credential)
        let updated = TrustedHost(version: 1, endpoint: endpoint, hostId: host, certificateSHA256: first.certificateSHA256, deviceId: device, clientId: device, credential: String(repeating: "c", count: 43))
        try PairingSecret.saveTrust(updated)
        XCTAssertEqual(try PairingSecret.readTrust(endpoint: endpoint)?.credential, updated.credential)
        let invalid = TrustedHost(version: 99, endpoint: endpoint, hostId: host, certificateSHA256: first.certificateSHA256, deviceId: device, clientId: device, credential: String(repeating: "d", count: 43))
        XCTAssertThrowsError(try PairingSecret.saveTrust(invalid))
        XCTAssertEqual(try PairingSecret.readTrust(endpoint: endpoint)?.credential, updated.credential)
    }
    @MainActor func testGivenInterruptedTrustReplacementShouldQuarantineUntilRollbackOrAcceptance() async throws {
        let endpoint = "wss://fixture-" + UUID().uuidString.lowercased() + ".ts.net/v1"
        defer { try? PairingSecret.removeTrust(endpoint: endpoint); try? PairingSecret.finishTrustChange(endpoint: endpoint) }
        let host = UUID().uuidString; let device = UUID().uuidString
        let first = TrustedHost(version: 1, endpoint: endpoint, hostId: host, certificateSHA256: String(repeating: "a", count: 64), deviceId: device, clientId: device, credential: String(repeating: "b", count: 43))
        let updated = TrustedHost(version: 1, endpoint: endpoint, hostId: host, certificateSHA256: first.certificateSHA256, deviceId: device, clientId: device, credential: String(repeating: "c", count: 43))
        try PairingSecret.saveTrust(first)
        let writer = PairingTrustWriter(save: { try PairingSecret.saveTrust($0) }, load: { try PairingSecret.readTrust(endpoint: $0) }, remove: { try PairingSecret.removeTrust(endpoint: $0) }, begin: { try PairingSecret.beginTrustChange(endpoint: $0) }, finish: { try PairingSecret.finishTrustChange(endpoint: $0) }, backup: { try PairingSecret.backupTrust(endpoint: $0) })
        writer.advance(to: 1); try await writer.commit(updated, generation: 1)
        XCTAssertThrowsError(try PairingSecret.readTrust(endpoint: endpoint), "Unaccepted replacement must not release a credential")
        writer.advance(to: 2); await writer.drain()
        XCTAssertEqual(try PairingSecret.readTrust(endpoint: endpoint)?.credential, first.credential, "Cancellation restores original trust")
        try PairingSecret.beginTrustChange(endpoint: endpoint); try PairingSecret.saveTrust(updated)
        XCTAssertThrowsError(try PairingSecret.readTrust(endpoint: endpoint), "An interrupted process leaves a durable quarantine")
        try await writer.commit(first, generation: 2); writer.advance(to: 3); await writer.drain()
        XCTAssertThrowsError(try PairingSecret.readTrust(endpoint: endpoint), "Canceled re-pair must preserve preexisting quarantine")
        try await writer.commit(first, generation: 3); XCTAssertTrue(writer.accept(generation: 3)); await writer.drain()
        XCTAssertEqual(try PairingSecret.readTrust(endpoint: endpoint)?.credential, first.credential, "Deliberate accepted re-pair can finalize quarantined trust")
        let corrupt = Data("unknown or corrupt previous trust".utf8)
        let identity: [String: Any] = [kSecClass as String: kSecClassGenericPassword, kSecAttrService as String: "PiMobile.TrustedHost.v1", kSecAttrAccount as String: endpoint]
        XCTAssertEqual(SecItemUpdate(identity as CFDictionary, [kSecValueData as String: corrupt] as CFDictionary), errSecSuccess)
        writer.advance(to: 4); try await writer.commit(updated, generation: 4); writer.advance(to: 5); await writer.drain()
        var query = identity; query[kSecReturnData as String] = true; var restored: CFTypeRef?
        XCTAssertEqual(SecItemCopyMatching(query as CFDictionary, &restored), errSecSuccess)
        XCTAssertEqual(restored as? Data, corrupt, "Canceled repair preserves exact opaque previous bytes")
        XCTAssertThrowsError(try PairingSecret.readTrust(endpoint: endpoint), "Corrupt old bytes never authorize a connection")
        try await writer.commit(updated, generation: 5); XCTAssertTrue(writer.accept(generation: 5)); await writer.drain()
        XCTAssertEqual(try PairingSecret.readTrust(endpoint: endpoint)?.credential, updated.credential, "Deliberate repair must not require decoding old bytes")
    }

    func testGivenRepeatedFailuresShouldBoundReconnectAndResetOnNetworkChange() {
        var budget = ReconnectBudget()
        XCTAssertEqual((0..<5).compactMap { _ in budget.nextDelay() }, [1,2,4,8,16])
        XCTAssertNil(budget.nextDelay()); budget.reset(); XCTAssertEqual(budget.nextDelay(), 1)
    }

    @MainActor func testGivenExplicitDisconnectShouldStayDisconnectedOnForegroundWithSavedEndpoint() {
        let suite = "connection-check-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set("wss://100.100.1.2:8787/v1", forKey: "pi.endpoint")
        let model = ChatModel(defaults: defaults, trustLoader: { _ in nil })
        model.disconnect()
        model.setForeground(false)
        model.setForeground(true)
        XCTAssertFalse(model.connected)
        XCTAssertFalse(model.connectionStatus.contains("Connecting"))
    }

    @MainActor func testGivenConnectionIntentShouldReconnectOnForeground() {
        let suite = "connection-check-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        defaults.set("wss://100.100.1.2:8787/v1", forKey: "pi.endpoint")
        let model = ChatModel(defaults: defaults, trustLoader: { _ in nil })
        model.connect()
        model.setForeground(false)
        model.setForeground(true)
        XCTAssertEqual(model.connectionStatus, "Connecting to your computer…")
        model.disconnect()
    }

    func testGivenAttachedFilesShouldComposeNativeImagesAndReadableTextWithinBudgets() throws {
        let file = ComposerAttachment(id: "file", name: "notes.txt", mimeType: "text/plain", data: Data("Project notes".utf8))
        let photo = ComposerAttachment(id: "photo", name: "photo.jpg", mimeType: "image/jpeg", data: Data([255,216,255,1]))
        let content = try ComposerPayload.make(text: "Read these", attachments: [file,photo])
        XCTAssertTrue(content.message.contains("Project notes"))
        XCTAssertTrue(content.message.contains("notes.txt"))
        XCTAssertEqual(content.images.count, 1)
        XCTAssertEqual(content.images[0]["data"], photo.data.base64EncodedString())
        XCTAssertThrowsError(try ComposerPayload.make(text: "", attachments: Array(repeating: photo, count: 5)))
        XCTAssertThrowsError(try ComposerPayload.make(text: String(repeating: "x", count: 65537), attachments: []))
        let saved = Submission(id: "one", text: "Draft", sessionId: "session", status: "Delivery uncertain", recoverable: true, attachments: [photo])
        let recovered = try JSONDecoder().decode(Submission.self, from: JSONEncoder().encode(saved))
        XCTAssertEqual(recovered.attachments?.first?.data, photo.data)
    }
    func testGivenNativeAndLiveToolsShouldGroupOnceWithoutBlankAssistantRows() {
        let messages = [
            ChatMessage(id: "toolResult-1-call-a", role: "toolResult", text: "Read output", activity: "read"),
            ChatMessage(id: "assistant-2", role: "assistant", text: " "),
            ChatMessage(id: "toolResult-3-call-b", role: "toolResult", text: "Shell output", activity: "bash")
        ]
        let live = [ToolActivity(id: "call-a", name: "read", state: "finished", text: "Read output"), ToolActivity(id: "call-b", name: "bash", state: "finished", text: "Shell output"), ToolActivity(id: "call-c", name: "read", state: "running", text: "Working")]
        let rows = TranscriptRows.make(messages: messages, tools: live)
        XCTAssertEqual(rows.count, 1)
        guard let first = rows.first, case .activity(let tools) = first else { return XCTFail("Expected one activity group") }
        XCTAssertEqual(tools.map(\.id), ["call-a", "call-b", "call-c"])
        XCTAssertEqual(tools.last?.state, "running")
    }
    func testGivenProseBetweenToolsShouldPreserveConversationOrderAndErrors() {
        let rows = TranscriptRows.make(messages: [
            ChatMessage(id: "first", role: "toolResult", text: "Output", activity: "read"),
            ChatMessage(id: "explanation", role: "assistant", text: "What I found"),
            ChatMessage(id: "second", role: "toolResult", text: "", activity: "bash", error: "Permission denied"),
            ChatMessage(id: "error", role: "assistant", text: "", error: "Provider failed")
        ], tools: [])
        XCTAssertEqual(rows.count, 4)
        guard case .activity(let failed) = rows[2] else { return XCTFail("Expected tool group after prose") }
        XCTAssertEqual(failed[0].state, "failed")
        XCTAssertTrue(failed[0].text.contains("Permission denied"))
        XCTAssertEqual(rows[3].id, "message:error")
    }
    func testGivenPairingQRShouldAcceptNativePayloadAndRejectUnsafeOrUnsupportedCodes() {
        let payload = "{\"type\":\"pi-mobile-pairing\",\"version\":1,\"endpoint\":\"wss://100.94.14.74:8787/v1\",\"hostId\":\"550e8400-e29b-41d4-a716-446655440000\",\"certificateSHA256\":\"" + String(repeating: "a", count: 64) + "\",\"enrollmentToken\":\"" + String(repeating: "b", count: 43) + "\",\"expiresAt\":" + String(Int64(Date().timeIntervalSince1970 * 1000) + 120000) + "}"
        XCTAssertEqual(PairingCode.parse(payload)?.endpoint, "wss://100.94.14.74:8787/v1")
        XCTAssertNil(PairingCode.parse(payload.replacingOccurrences(of: "100.94.14.74", with: "example.com")))
        XCTAssertNil(PairingCode.parse(payload.replacingOccurrences(of: "\"version\":1", with: "\"version\":2")))
        XCTAssertNil(PairingCode.parse(payload.replacingOccurrences(of: String(repeating: "b", count: 43), with: "short")))
        XCTAssertNil(PairingCode.parse(String(repeating: "x", count: 8193)))
        XCTAssertNil(PairingCode.parse("https://example.com"))
    }
    func testGivenLaterTypingShouldPreserveItWhenRestoringSubmission() {
        var draft = DraftState(text: "First instruction")
        let submitted = draft.takeSubmission()
        draft.text = "A later thought"
        draft.restore(submitted)
        XCTAssertEqual(draft.text, "A later thought\n\nFirst instruction")
    }
    func testGivenRepeatedSnapshotShouldRejectOlderRevision() {
        var cursor = SnapshotCursor()
        XCTAssertTrue(cursor.accept(epoch: "one", revision: 5))
        XCTAssertFalse(cursor.accept(epoch: "one", revision: 4))
        XCTAssertFalse(cursor.accept(epoch: "one", revision: 5))
        XCTAssertTrue(cursor.accept(epoch: "two", revision: 1))
    }
    func testGivenFencedCodeShouldKeepCodeSeparateAndCopyable() {
        let blocks = MarkdownBlocks.split("Hello\n```swift\nlet x = 1\n```\nGoodbye")
        XCTAssertEqual(blocks.count, 3)
        XCTAssertEqual(blocks[1], .code(language: "swift", text: "let x = 1"))
        XCTAssertEqual(MarkdownBlocks.split("```\npartial").last, .code(language: "", text: "partial"))
    }
    func testGivenNumericLookingPublicHostnameShouldRejectManualAndQRPairing() {
        for host in ["100.64.1.2.attacker.example", "100.attacker.64.1.2", "100..64.1.2", "100.064.1.2", "100.63.1.2", "100.128.1.2", "100.64.1.256"] {
            XCTAssertNil(ConnectionAddress.url("ws://" + host + "/v1"))
            let qr = "{\"type\":\"pi-mobile-pairing\",\"version\":1,\"endpoint\":\"ws://" + host + "/v1\",\"secret\":\"fixture-pairing-secret-0123456789abcdef\"}"
            XCTAssertNil(PairingCode.parse(qr))
        }
        XCTAssertNotNil(ConnectionAddress.url("ws://100.64.0.1/v1"))
        XCTAssertNotNil(ConnectionAddress.url("ws://100.127.255.254/v1"))
    }
    func testGivenUnsafeEndpointShouldRejectPublicAndWildcardHosts() {
        XCTAssertNotNil(ConnectionAddress.url("ws://100.100.1.2:8787/v1"))
        XCTAssertNotNil(ConnectionAddress.url("wss://mac.example.ts.net/v1"))
        XCTAssertNil(ConnectionAddress.url("ws://0.0.0.0:8787/v1"))
        XCTAssertNil(ConnectionAddress.url("ws://example.com/v1"))
    }
    func testGivenChatAccessShouldFailClosedOnUnknownValues() throws {
        XCTAssertEqual(ChatGrant.from("read"), .read)
        XCTAssertEqual(ChatGrant.from("admin"), .off)
        XCTAssertEqual(ChatGrant.from(nil), .send, "Bridges without chat gating allowed full chat")
        XCTAssertFalse(ChatGrant.read.canSend)
        XCTAssertFalse(ChatGrant.off.canRead)
        let envelope = try JSONDecoder().decode(WireEnvelope.self, from: Data(#"{"type":"permissions","chat":"off"}"#.utf8))
        XCTAssertEqual(ChatGrant.from(envelope.chat), .off)
    }

    private func enrollment(endpoint: String = "wss://100.94.14.74:8787/v1", pin: String = String(repeating: "a", count: 64), expiresIn: Int64 = 120000) -> String {
        "{\"type\":\"pi-mobile-pairing\",\"version\":1,\"endpoint\":\"" + endpoint + "\",\"hostId\":\"550e8400-e29b-41d4-a716-446655440000\",\"certificateSHA256\":\"" + pin + "\",\"enrollmentToken\":\"" + String(repeating: "b", count: 43) + "\",\"expiresAt\":" + String(Int64(Date().timeIntervalSince1970 * 1000) + expiresIn) + "}"
    }
    private func failure(_ result: Result<PairingCode, PairingInputFailure>) -> PairingInputFailure? {
        if case .failure(let failure) = result { return failure }
        return nil
    }
    func testGivenPastedEnrollmentShouldRequireExactPrefixAndApplyScannedValidation() throws {
        let json = enrollment()
        let scanned = try PairingCode.read(json, from: .scanned).get()
        let pasted = try PairingCode.read("purepoint-pair:" + json, from: .pasted).get()
        XCTAssertEqual(pasted.endpoint, scanned.endpoint)
        XCTAssertEqual(pasted.hostId, scanned.hostId)
        XCTAssertEqual(pasted.certificateSHA256, scanned.certificateSHA256)
        XCTAssertEqual(pasted.enrollmentToken, scanned.enrollmentToken)
        XCTAssertEqual(try PairingCode.read("\n  purepoint-pair:" + json + " \r\n", from: .pasted).get().hostId, scanned.hostId, "Surrounding whitespace is trimmed")

        XCTAssertEqual(failure(PairingCode.read(json, from: .pasted)), .missingPrefix, "Raw QR JSON is not a paste code")
        XCTAssertEqual(failure(PairingCode.read("PurePoint-Pair:" + json, from: .pasted)), .missingPrefix)
        XCTAssertEqual(failure(PairingCode.read("pair:" + json, from: .pasted)), .missingPrefix)
        XCTAssertEqual(failure(PairingCode.read("x purepoint-pair:" + json, from: .pasted)), .missingPrefix)
        XCTAssertEqual(failure(PairingCode.read("purepoint-pair: " + json, from: .pasted)), .invalid, "Only surrounding whitespace is trimmed")
        XCTAssertEqual(failure(PairingCode.read("purepoint-pair:", from: .pasted)), .invalid)
        XCTAssertEqual(failure(PairingCode.read("purepoint-pair:" + json + String(repeating: " ", count: 4096), from: .pasted)), .oversized, "Size is checked before trimming or parsing")
        XCTAssertEqual(failure(PairingCode.read("purepoint-pair:" + json, from: .scanned)), .invalid, "QR payloads are unchanged and carry no prefix")

        // Tampered pastes fail the same pin, expiry and endpoint allow-list checks as a scan.
        for tampered in [enrollment(endpoint: "wss://example.com/v1"), enrollment(endpoint: "ws://100.94.14.74:8787/v1"), enrollment(pin: String(repeating: "a", count: 63)), enrollment(pin: String(repeating: "A", count: 64)), enrollment(expiresIn: -1000), enrollment(expiresIn: 600000), json.replacingOccurrences(of: "\"version\":1", with: "\"version\":2")] {
            XCTAssertEqual(failure(PairingCode.read("purepoint-pair:" + tampered, from: .pasted)), .invalid)
            XCTAssertEqual(failure(PairingCode.read(tampered, from: .scanned)), .invalid)
            XCTAssertNil(PairingCode.parse(tampered))
        }
    }
    func testGivenVerifyResponseShouldReadHostDescriptionStrictlyAndIgnoreUnknownValues() {
        func verify(_ extra: String) -> HostIdentity {
            HostIdentity.fromVerify(Data(("{\"version\":1,\"hostId\":\"550e8400-e29b-41d4-a716-446655440000\",\"deviceId\":\"d\",\"clientId\":\"d\"" + extra + "}").utf8))
        }
        XCTAssertEqual(verify(""), HostIdentity(), "Older bridges send neither field")
        XCTAssertEqual(verify(#","hostKind":"linux","hostLabel":"build-box""#), HostIdentity(kind: .linux, label: "build-box"))
        XCTAssertEqual(verify(#","hostKind":"mac","hostLabel":"Jonos-MacBook-Pro""#), HostIdentity(kind: .mac, label: "Jonos-MacBook-Pro"))
        XCTAssertEqual(verify(#","hostKind":"windows","hostLabel":"box""#), HostIdentity(kind: nil, label: "box"), "Unknown kinds are absent, not an error")
        XCTAssertNil(verify(#","hostKind":"Linux""#).kind, "Kinds are exact")
        XCTAssertNil(verify(#","hostKind":1,"hostLabel":true"#).kind)
        XCTAssertNil(verify(#","hostKind":1,"hostLabel":true"#).label)
        XCTAssertEqual(verify(#","hostLabel":""# + String(repeating: "a", count: 63) + "\"").label?.count, 63)
        for label in ["", "Linux host", "<script>", "build.box", "büro", "a\nb", String(repeating: "a", count: 64)] {
            let encoded = String(data: try! JSONEncoder().encode(label), encoding: .utf8)!
            XCTAssertNil(verify(",\"hostKind\":\"linux\",\"hostLabel\":" + encoded).label, "Rejects label " + label.debugDescription)
        }
        XCTAssertEqual(HostIdentity.fromVerify(Data("not json".utf8)), HostIdentity())
        XCTAssertEqual(HostIdentity.fromVerify(Data("[\"linux\"]".utf8)), HostIdentity())
    }
    func testGivenHostDescriptionsShouldNameByLabelKeepHostsDistinctAndSurviveRelaunch() throws {
        let mac = UUID().uuidString, linux = UUID().uuidString
        var registry = MachineRegistry()
        _ = registry.remember(hostId: mac, endpoint: "wss://100.100.1.2:8787/v1")
        _ = registry.remember(hostId: linux, endpoint: "wss://100.100.1.3:8787/v1")
        XCTAssertEqual(registry.machines.first { $0.id == linux }?.name, "100.100.1.3")
        XCTAssertFalse(registry.describe(hostId: linux, as: HostIdentity()), "Older bridges change nothing")
        XCTAssertFalse(registry.describe(hostId: UUID().uuidString, as: HostIdentity(kind: .linux, label: "build-box")), "Only a saved hostId is described")
        XCTAssertTrue(registry.describe(hostId: mac, as: HostIdentity(kind: .mac, label: "build-box")))
        XCTAssertTrue(registry.describe(hostId: linux, as: HostIdentity(kind: .linux, label: "build-box")))
        let byId = Dictionary(uniqueKeysWithValues: registry.machines.map { ($0.id, $0) })
        XCTAssertEqual(registry.machines.count, 2, "Matching labels never merge hosts")
        XCTAssertEqual(byId[mac]?.name, "build-box"); XCTAssertEqual(byId[linux]?.name, "build-box")
        XCTAssertEqual(byId[mac]?.host, HostIdentity(kind: .mac, label: "build-box"))
        XCTAssertEqual(byId[linux]?.host, HostIdentity(kind: .linux, label: "build-box"))
        XCTAssertEqual(HostCopy.symbol(byId[linux]?.host.kind), "server.rack")
        XCTAssertEqual(HostCopy.symbol(byId[mac]?.host.kind), "desktopcomputer")
        XCTAssertEqual(HostCopy.symbol(nil), "desktopcomputer")
        XCTAssertTrue(registry.valid)

        let index = try XCTUnwrap(registry.machines.firstIndex { $0.id == linux })
        registry.machines[index].name = "Home server"
        XCTAssertTrue(registry.describe(hostId: linux, as: HostIdentity(kind: .linux, label: "renamed-box")))
        XCTAssertEqual(registry.machines[index].name, "Home server", "A user-chosen name wins over the host label")
        XCTAssertEqual(registry.machines[index].hostLabel, "renamed-box")
        _ = registry.remember(hostId: linux, endpoint: "wss://100.100.1.9:8787/v1")
        XCTAssertEqual(registry.selected?.host, HostIdentity(kind: .linux, label: "renamed-box"), "Re-pair and address changes keep the description")

        let relaunched = try JSONDecoder().decode(MachineRegistry.self, from: JSONEncoder().encode(registry))
        XCTAssertTrue(relaunched.valid)
        XCTAssertEqual(relaunched.machines, registry.machines)
        let legacy = try JSONDecoder().decode(MachineRegistry.self, from: Data(("{\"version\":1,\"selectedId\":\"" + mac + "\",\"machines\":[{\"id\":\"" + mac + "\",\"endpoint\":\"wss://100.100.1.2:8787/v1\",\"name\":\"Desk\",\"stateKey\":\"" + mac + "\"}]}").utf8))
        XCTAssertTrue(legacy.valid); XCTAssertEqual(legacy.selected?.host, HostIdentity())
        let future = try JSONDecoder().decode(MachineRegistry.self, from: Data(("{\"version\":1,\"selectedId\":\"" + mac + "\",\"machines\":[{\"id\":\"" + mac + "\",\"endpoint\":\"wss://100.100.1.2:8787/v1\",\"name\":\"Desk\",\"stateKey\":\"" + mac + "\",\"hostKind\":\"windows\",\"hostLabel\":\"bad label\"}]}").utf8))
        XCTAssertTrue(future.valid, "Unknown saved descriptions never invalidate the list")
        XCTAssertEqual(future.selected?.host, HostIdentity())
    }
    func testGivenHostKindShouldWordRepairHintForThatHost() {
        let linux = HostCopy.repairHint(kind: .linux, label: "build-box")
        XCTAssertTrue(linux.contains("`pu phone pair` on build-box"))
        XCTAssertFalse(linux.contains("Mac"))
        XCTAssertTrue(HostCopy.repairHint(kind: .linux, label: nil).contains("`pu phone pair` on that computer"))
        let mac = HostCopy.repairHint(kind: .mac, label: "Desk")
        XCTAssertTrue(mac.contains("Connect phone") && mac.contains("Mac"))
        XCTAssertFalse(mac.contains("pu phone pair"))
        let unknown = HostCopy.repairHint(kind: nil, label: nil)
        XCTAssertTrue(unknown.contains("Connect phone") && unknown.contains("pu phone pair"), "Without a verified kind, name both ways to create a code")
    }
    @MainActor func testGivenLinuxHostTrustLostShouldAskToRunPuPhonePairOnThatHost() async throws {
        let suite = "connection-check-" + UUID().uuidString
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let host = UUID().uuidString
        var registry = MachineRegistry()
        _ = registry.remember(hostId: host, endpoint: "wss://100.100.1.2:8787/v1")
        _ = registry.describe(hostId: host, as: HostIdentity(kind: .linux, label: "build-box"))
        defaults.set(try JSONEncoder().encode(registry), forKey: "pi.machines.v1")
        let model = ChatModel(defaults: defaults, trustLoader: { _ in nil })
        XCTAssertEqual(model.selectedMachine?.name, "build-box")
        model.connect()
        for _ in 0..<300 where model.connectionStatus != "Re-pair needed" { try await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertEqual(model.connectionStatus, "Re-pair needed")
        XCTAssertEqual(model.error, "No saved device trust. Run `pu phone pair` on build-box, then scan or paste the new code.")
        model.disconnect()
    }
}
