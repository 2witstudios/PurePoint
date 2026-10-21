#if os(iOS)
import CryptoKit
import Foundation
import Testing

@testable import PurePoint

struct SSHDestinationTests {
    @Test func parsesAndFormatsUserHostPort() {
        #expect(SSHDestination(sshHost: "jono@studio.local") == SSHDestination(user: "jono", host: "studio.local"))
        #expect(SSHDestination(sshHost: "jono@studio.local:2222") == SSHDestination(user: "jono", host: "studio.local", port: 2222))
        #expect(SSHDestination(user: "jono", host: "studio.local").sshHost == "jono@studio.local")
        #expect(SSHDestination(user: "jono", host: "127.0.0.1", port: 22022).sshHost == "jono@127.0.0.1:22022")
    }

    @Test func bracketsIPv6Literals() {
        let v6 = SSHDestination(user: "me", host: "fe80::1", port: 2200)
        #expect(v6.sshHost == "me@[fe80::1]:2200")
        #expect(SSHDestination(sshHost: v6.sshHost) == v6)
        #expect(SSHDestination(sshHost: "me@[::1]") == SSHDestination(user: "me", host: "::1"))
    }

    @Test func refusesAliasesAndBadPorts() {
        #expect(SSHDestination(sshHost: "studio-mac") == nil)
        #expect(SSHDestination(sshHost: "@host") == nil)
        #expect(SSHDestination(sshHost: "me@host:0") == nil)
        #expect(SSHDestination(sshHost: "me@host:70000") == nil)
        #expect(SSHDestination(sshHost: "me@host:ssh") == nil)
        #expect(SSHDestination(sshHost: "me@") == nil)
        #expect(Machine(name: "a", sshHost: "studio-mac").destination == nil)
    }
}

/// The container registry: same schema, atomic writes, no lock file.
@Suite(.serialized) struct IPadMachineRegistryTests {
    private func temporaryPath() -> String {
        NSTemporaryDirectory() + "pu-registry-\(UUID().uuidString)/machines.json"
    }

    @Test func registryLivesInTheAppContainersApplicationSupport() {
        #expect(MachinePaths.registryPath.hasSuffix("/Library/Application Support/PurePoint/machines.json"))
        #expect(MachinePaths.registryPath.hasPrefix(NSHomeDirectory()))
    }

    @Test func writesTheSharedSchemaWithoutALockFile() throws {
        let path = temporaryPath()
        let id = UUID()
        try MachineRegistryStore.update(path: path) { machines in
            _ = try MachineRegistryStore.add(name: "studio", sshHost: "jono@studio.local:2222", id: id, to: &machines)
        }
        let directory = (path as NSString).deletingLastPathComponent
        #expect(try FileManager.default.contentsOfDirectory(atPath: directory) == ["machines.json"])
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: URL(fileURLWithPath: path))) as? [String: Any]
        #expect(json?["version"] as? Int == 1)
        let entry = (json?["machines"] as? [[String: Any]])?.first
        #expect(entry?["id"] as? String == id.uuidString.lowercased())
        #expect(entry?["name"] as? String == "studio")
        #expect(entry?["ssh_host"] as? String == "jono@studio.local:2222")
        #expect(try MachineRegistryStore.load(path: path) == [Machine(id: id, name: "studio", sshHost: "jono@studio.local:2222")])
        let attributes = try FileManager.default.attributesOfItem(atPath: path)
        #expect(attributes[.posixPermissions] as? Int == 0o600)
    }
}

// MARK: - Machine connection over SSH

/// Collects what a connection reports.
private final class Reports: @unchecked Sendable {
    private let lock = NSLock()
    private var states: [MachineConnectionState] = []
    private var refusals: [SSHTrustError?] = []
    func state(_ value: MachineConnectionState) { lock.withLock { states.append(value) } }
    func trust(_ value: SSHTrustError?) { lock.withLock { refusals.append(value) } }
    var last: MachineConnectionState? { lock.withLock { states.last } }
    var all: [MachineConnectionState] { lock.withLock { states } }
    var lastRefusal: SSHTrustError?? { lock.withLock { refusals.last } }
}

@Suite(.serialized) struct IPadMachineConnectionTests {
    private func connection(_ connector: FakeConnector, name: String = "studio") -> (MachineConnection, Reports) {
        let reports = Reports()
        let machine = Machine(name: name, sshHost: "jono@127.0.0.1:22022")
        let connection = MachineConnection(
            machine: machine, connector: connector, onTrust: { reports.trust($0) }, onState: { reports.state($0) })
        return (connection, reports)
    }

    @Test func healthyDaemonReportsRunning() async throws {
        let (connection, reports) = connection(FakeConnector(respond: healthRelay))
        await connection.connect()
        await eventually { reports.last == .connected(.running) }
        #expect(reports.last == .connected(.running))
        await connection.disconnect()
        #expect(reports.last == .disconnected)
    }

    @Test func noDaemonReportsStoppedAndNeverStartsOne() async throws {
        let connector = FakeConnector { channel, _ in channel.end(.exited(69, stderr: "")) }
        let (connection, reports) = connection(connector)
        await connection.connect()
        await eventually { reports.last == .connected(.stopped) }
        #expect(reports.last == .connected(.stopped))
        await connection.disconnect()
    }

    @Test func startDaemonRunsTheStartCommandOverSSH() async throws {
        let commands = CommandLog()
        let connector = FakeConnector(
            respond: { channel, _ in channel.end(.exited(69, stderr: "")) },
            onOpen: { channel in
                commands.append(channel.command)
                if channel.command == MachineSSHCommands.startDaemonCommand { channel.end(.exited(0, stderr: "")) }
            })
        let (connection, reports) = connection(connector)
        await connection.connect()
        await eventually { reports.last == .connected(.stopped) }
        #expect(await connection.startDaemon() == nil)
        #expect(commands.all.contains(MachineSSHCommands.startDaemonCommand))
        #expect(commands.all.contains(SSHExecTransport.relayCommand))
        await connection.disconnect()
    }

    @Test func missingPuIsReportedNamingTheMachine() async throws {
        let connector = FakeConnector { channel, _ in channel.end(.exited(127, stderr: "sh: pu: not found")) }
        let (connection, reports) = connection(connector, name: "build-box")
        await connection.connect()
        await eventually { if case .offline = reports.last { true } else { false } }
        guard case .offline(let message) = reports.last else {
            Issue.record("expected offline, got \(String(describing: reports.last))")
            return
        }
        #expect(message.contains("build-box"))
        #expect(message.contains("not installed"))
        await connection.disconnect()
    }

    @Test func hostKeyRefusalStopsAndAsksTheUser() async throws {
        let key = SSHPublicKey.ed25519(Curve25519.Signing.PrivateKey().publicKey)
        let connector = FakeConnector()
        connector.setFailure(SSHTrustError.hostKeyUnconfirmed(key))
        let (connection, reports) = connection(connector)
        await connection.connect()
        await eventually { reports.lastRefusal != nil }
        #expect(reports.lastRefusal == .some(.hostKeyUnconfirmed(key)))
        // No automatic retry while the user has not decided.
        try await Task.sleep(for: .milliseconds(300))
        #expect(connector.attempts == 1)
        await connection.disconnect()
    }

    private func silentConnection(_ relay: SwitchableRelay) -> (MachineConnection, Reports, FakeConnector) {
        let reports = Reports()
        let connector = FakeConnector(respond: { channel, data in relay.respond(channel, data) })
        let connection = MachineConnection(
            machine: Machine(name: "studio", sshHost: "jono@127.0.0.1:22022"), connector: connector,
            probeTimeout: 0.2, onTrust: { reports.trust($0) }, onState: { reports.state($0) })
        return (connection, reports, connector)
    }

    /// A connection that died without closing: the session still reads connected, but no
    /// probe is answered. It is replaced, through the backoff, rather than read as stopped.
    @Test func probesThatTimeOutReplaceTheConnection() async throws {
        let relay = SwitchableRelay()
        let (connection, reports, connector) = silentConnection(relay)
        relay.silent = true
        await connection.connect()
        await waitFor(seconds: 10) { connector.attempts >= 2 }
        #expect(connector.attempts == 2)
        #expect(connector.all[0].isClosed)
        #expect(!reports.all.contains(.connected(.stopped)))
        #expect(reports.all.contains(.offline("SSH connection closed")))
        // The new connection answers: the machine is back.
        relay.silent = false
        await waitFor(seconds: 10) { reports.last == .connected(.running) }
        #expect(reports.last == .connected(.running))
        await connection.disconnect()
    }

    /// A relay that never answers on a fresh connection either is not the network.
    @Test func aReplacementThatTimesOutTooReportsTheRelay() async throws {
        let relay = SwitchableRelay()
        let (connection, reports, connector) = silentConnection(relay)
        relay.silent = true
        await connection.connect()
        await waitFor(seconds: 15) { reports.last == .offline("No answer from the daemon relay on studio.") }
        #expect(reports.last == .offline("No answer from the daemon relay on studio."))
        #expect(connector.attempts == 2)
        #expect(!reports.all.contains(.connected(.stopped)))
        await connection.disconnect()
    }

    @Test func returningToTheForegroundReplacesASilentConnection() async throws {
        let relay = SwitchableRelay()
        let (connection, reports, connector) = silentConnection(relay)
        await connection.connect()
        await waitFor(seconds: 5) { reports.last == .connected(.running) }
        relay.silent = true
        await connection.checkAfterForeground()
        #expect(connector.all[0].isClosed)
        relay.silent = false
        await waitFor(seconds: 10) { connector.attempts == 2 && reports.last == .connected(.running) }
        #expect(connector.attempts == 2)
        #expect(reports.last == .connected(.running))
        await connection.disconnect()
    }

    @Test func aDroppedConnectionIsReportedAndReconnects() async throws {
        let connector = FakeConnector(respond: healthRelay)
        let (connection, reports) = connection(connector)
        await connection.connect()
        await eventually { reports.last == .connected(.running) }
        connector.all[0].drop()
        await waitFor(seconds: 5) { connector.attempts == 2 && reports.last == .connected(.running) }
        #expect(reports.all.contains(.offline("SSH connection closed")))
        #expect(reports.last == .connected(.running))
        await connection.disconnect()
    }
}

/// A relay answering health checks, or silent like a connection that died without closing.
private final class SwitchableRelay: @unchecked Sendable {
    private let lock = NSLock()
    private var isSilent = false
    var silent: Bool {
        get { lock.withLock { isSilent } }
        set { lock.withLock { isSilent = newValue } }
    }

    func respond(_ channel: FakeExecChannel, _ data: Data) {
        guard !silent else { return }
        healthRelay(channel, data)
    }
}

private func waitFor(seconds: Double, _ condition: () async -> Bool) async {
    let deadline = ContinuousClock.now + .seconds(seconds)
    while ContinuousClock.now < deadline, !(await condition()) {
        try? await Task.sleep(for: .milliseconds(20))
    }
}

private final class CommandLog: @unchecked Sendable {
    private let lock = NSLock()
    private var commands: [String] = []
    func append(_ command: String) { lock.withLock { commands.append(command) } }
    var all: [String] { lock.withLock { commands } }
}
#endif
