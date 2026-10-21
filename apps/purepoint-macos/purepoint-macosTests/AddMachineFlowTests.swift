#if os(iOS)
import CryptoKit
import Foundation
import Testing

@testable import PurePoint

/// A server that presents `hostKey` and, like the Citadel connector, refuses before
/// authentication unless the pin store trusts it for the machine.
private final class PinCheckingConnector: SSHConnector, @unchecked Sendable {
    let inner: FakeConnector
    let pins: HostKeyPinStore
    let machineId: UUID
    let hostKey: SSHPublicKey

    init(inner: FakeConnector, pins: HostKeyPinStore, machineId: UUID, hostKey: SSHPublicKey) {
        self.inner = inner
        self.pins = pins
        self.machineId = machineId
        self.hostKey = hostKey
    }

    func connect(onDisconnect: @escaping @Sendable () -> Void) async throws -> any SSHExecConnection {
        if let refusal = SSHTrustError(pins.evaluate(hostKey, machineId: machineId)) { throw refusal }
        return try await inner.connect(onDisconnect: onDisconnect)
    }
}

@MainActor
private final class SavedMachines {
    var machines: [Machine] = []
}

@MainActor
struct AddMachineFlowTests {
    private let hostKey = SSHPublicKey.ed25519(Curve25519.Signing.PrivateKey().publicKey)

    private func makeFlow(
        relay: @escaping @Sendable (FakeExecChannel, Data) -> Void = healthRelay
    ) -> (AddMachineFlow, HostKeyPinStore, SavedMachines, FakeConnector) {
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        let saved = SavedMachines()
        let fake = FakeConnector(respond: relay)
        let hostKey = self.hostKey
        let flow = AddMachineFlow(
            pins: pins, identities: SSHIdentityStore(store: InMemorySecretStore()),
            connectorFactory: { machine in
                PinCheckingConnector(inner: fake, pins: pins, machineId: machine.id, hostKey: hostKey)
            },
            saveMachine: { _, machine in saved.machines.append(machine) })
        flow.name = "studio"
        flow.host = "127.0.0.1"
        flow.port = "22022"
        flow.user = "jono"
        return (flow, pins, saved, fake)
    }

    @Test func collectsLabelHostPortAndUserIntoTheRegistryForm() throws {
        let (flow, _, _, _) = makeFlow()
        let machine = try flow.candidate().get()
        #expect(machine.id == flow.machineId)
        #expect(machine.name == "studio")
        #expect(machine.sshHost == "jono@127.0.0.1:22022")
        flow.port = "22"
        #expect(try flow.candidate().get().sshHost == "jono@127.0.0.1")
        flow.host = "fe80::1"
        #expect(try flow.candidate().get().sshHost == "jono@[fe80::1]")
        flow.port = "0"
        #expect(throws: AddMachineError.invalidPort) { try flow.candidate().get() }
        flow.port = "22"
        flow.user = "a b"
        #expect(throws: AddMachineError.invalidUser) { try flow.candidate().get() }
        flow.user = "jono"
        flow.name = "bad name"
        #expect(throws: MachineRegistryError.invalidName("bad name")) { try flow.candidate().get() }
    }

    @Test func showsTheDevicePublicKey() {
        let (flow, _, _, _) = makeFlow()
        #expect(flow.publicKeyLine?.hasPrefix("ssh-ed25519 ") == true)
        #expect(flow.publicKeyLine?.hasSuffix(" purepoint-ipad") == true)
    }

    @Test func firstConnectAsksForHostKeyThenSavesAfterTheProbe() async throws {
        let (flow, pins, saved, _) = makeFlow()
        let appState = AppState()
        await flow.submit(appState: appState)
        #expect(flow.step == .confirmHostKey(hostKey))
        #expect(saved.machines.isEmpty)
        #expect(pins.pinnedKey(machineId: flow.machineId) == nil)

        await flow.confirmHostKey(appState: appState)
        #expect(flow.step == .saved)
        #expect(flow.error == nil)
        #expect(pins.pinnedKey(machineId: flow.machineId) == hostKey)
        #expect(saved.machines == [Machine(id: flow.machineId, name: "studio", sshHost: "jono@127.0.0.1:22022")])
    }

    @Test func cancellingTheHostKeySavesNothingAndPinsNothing() async throws {
        let (flow, pins, saved, _) = makeFlow()
        await flow.submit(appState: AppState())
        await flow.cancel()
        #expect(flow.step == .editing)
        #expect(saved.machines.isEmpty)
        #expect(pins.pinnedKey(machineId: flow.machineId) == nil)
    }

    @Test func aFailedProbeSavesNothingAndDropsThePin() async throws {
        let (flow, pins, saved, _) = makeFlow { channel, _ in channel.end(.exited(127, stderr: "sh: pu: not found")) }
        let appState = AppState()
        await flow.submit(appState: appState)
        await flow.confirmHostKey(appState: appState)
        #expect(flow.step == .editing)
        #expect(flow.error?.contains("not installed") == true)
        #expect(saved.machines.isEmpty)
        #expect(pins.pinnedKey(machineId: flow.machineId) == nil)
    }

    @Test func aRefusedKeyKeepsThePinSoTheRetryDoesNotAskAgain() async throws {
        let (flow, pins, saved, fake) = makeFlow()
        let appState = AppState()
        await flow.submit(appState: appState)
        fake.setFailure(SSHTransportError.authenticationFailed(machine: "studio", user: "jono"))
        await flow.confirmHostKey(appState: appState)
        #expect(flow.step == .editing)
        #expect(flow.error?.contains("refused this iPad's SSH key") == true)
        #expect(pins.pinnedKey(machineId: flow.machineId) == hostKey)

        // The user installs the key and tries again: no second trust prompt.
        fake.setFailure(nil)
        await flow.submit(appState: appState)
        #expect(flow.step == .saved)
        #expect(saved.machines.count == 1)
        #expect(pins.pinnedKey(machineId: flow.machineId) == hostKey)
    }

    @Test func cancellingAfterARefusedKeyDropsThePin() async throws {
        let (flow, pins, saved, fake) = makeFlow()
        let appState = AppState()
        await flow.submit(appState: appState)
        fake.setFailure(SSHTransportError.authenticationFailed(machine: "studio", user: "jono"))
        await flow.confirmHostKey(appState: appState)
        #expect(pins.pinnedKey(machineId: flow.machineId) == hostKey)
        await flow.cancel()
        #expect(saved.machines.isEmpty)
        #expect(pins.pinnedKey(machineId: flow.machineId) == nil)
    }

    @Test func aStoppedDaemonStillSavesTheMachine() async throws {
        let (flow, _, saved, _) = makeFlow { channel, _ in channel.end(.exited(69, stderr: "")) }
        let appState = AppState()
        await flow.submit(appState: appState)
        await flow.confirmHostKey(appState: appState)
        #expect(flow.step == .saved)
        #expect(flow.notice?.contains("not running") == true)
        #expect(saved.machines.count == 1)
    }

    @Test func testConnectionKeepsThePinAndAddDoesNotAskAgain() async throws {
        let (flow, pins, saved, _) = makeFlow()
        let appState = AppState()
        await flow.submit(appState: appState, save: false)
        await flow.confirmHostKey(appState: appState)
        #expect(flow.step == .tested)
        #expect(saved.machines.isEmpty)
        await flow.submit(appState: appState)
        #expect(flow.step == .saved)
        #expect(saved.machines.count == 1)
        #expect(pins.pinnedKey(machineId: flow.machineId) == hostKey)
    }

    @Test func aProtectedKeyWaitsForItsPassphraseThenImports() {
        let (flow, _, _, _) = makeFlow()
        let before = flow.publicKeyLine
        flow.importPrivateKey(OpenSSHKeyFormatTests.protectedKey, appState: AppState())
        #expect(flow.keyAwaitingPassphrase != nil)
        #expect(flow.error == nil)
        #expect(flow.publicKeyLine == before)

        flow.importAwaitingKey(passphrase: "not-it", appState: AppState())
        #expect(flow.error == SSHKeyImportError.wrongPassphrase.localizedDescription)
        #expect(flow.keyAwaitingPassphrase != nil)
        #expect(flow.publicKeyLine == before)

        flow.importAwaitingKey(passphrase: "test-pass", appState: AppState())
        #expect(flow.error == nil)
        #expect(flow.keyAwaitingPassphrase == nil)
        #expect(flow.publicKeyLine?.contains(OpenSSHKeyFormatTests.protectedKeyPublicBlob) == true)
    }

    @Test func cancellingThePassphraseDropsTheKey() {
        let (flow, _, _, _) = makeFlow()
        flow.importPrivateKey(OpenSSHKeyFormatTests.protectedKey, appState: AppState())
        flow.cancelPassphrase()
        #expect(flow.keyAwaitingPassphrase == nil)
    }

    @Test func importRefusesForeignKeysAndKeepsTheIdentity() {
        let (flow, _, _, _) = makeFlow()
        let before = flow.publicKeyLine
        flow.importPrivateKey("-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----", appState: AppState())
        #expect(flow.error == SSHKeyImportError.unsupportedKeyType("RSA").localizedDescription)
        #expect(flow.publicKeyLine == before)
        let key = Curve25519.Signing.PrivateKey()
        flow.importPrivateKey(OpenSSHPrivateKey.encodeEd25519(key, comment: "x"), appState: AppState())
        #expect(flow.error == nil)
        #expect(flow.publicKeyLine == SSHIdentity(privateKey: key).authorizedKeyLine)
    }

    @Test func debugLaunchArgumentFillsTheSameFields() {
        let (flow, _, _, _) = makeFlow()
        #expect(flow.fill(sshHost: "me@10.0.0.5:2222", name: nil))
        #expect(flow.user == "me")
        #expect(flow.host == "10.0.0.5")
        #expect(flow.port == "2222")
        #expect(flow.name == "machine")
        #expect(flow.fill(sshHost: "me@build.example.com", name: "builder"))
        #expect(flow.name == "builder")
        #expect(AddMachineFlow.suggestedName("studio.local") == "studio")
        #expect(!flow.fill(sshHost: "alias-only", name: nil))
    }

    #if DEBUG
    @Test func debugLaunchArgumentsParse() throws {
        let defaults = try #require(UserDefaults(suiteName: "pu-debug-args-\(UUID().uuidString)"))
        defaults.set("me@127.0.0.1:22022,lab", forKey: "PUDebugAddMachine")
        defaults.set(true, forKey: "PUDebugTrustHostKey")
        let arguments = DebugLaunchArguments(defaults: defaults)
        #expect(arguments.addMachine?.sshHost == "me@127.0.0.1:22022")
        #expect(arguments.addMachine?.name == "lab")
        #expect(arguments.trustHostKey)
    }

    @Test func debugAddMachineWithoutADestinationIsIgnored() throws {
        for value in [",", " , ", ",lab", " "] {
            let defaults = try #require(UserDefaults(suiteName: "pu-debug-args-\(UUID().uuidString)"))
            defaults.set(value, forKey: "PUDebugAddMachine")
            #expect(DebugLaunchArguments(defaults: defaults, arguments: []).addMachine == nil)
        }
        let defaults = try #require(UserDefaults(suiteName: "pu-debug-args-\(UUID().uuidString)"))
        defaults.set("me@host,", forKey: "PUDebugAddMachine")
        let arguments = DebugLaunchArguments(defaults: defaults, arguments: [])
        #expect(arguments.addMachine?.sshHost == "me@host")
        #expect(arguments.addMachine?.name == nil)
    }
    #endif
}
#endif
