#if os(iOS)
import Foundation
import Testing

@testable import PurePoint

/// The real Citadel connector against a local sshd, which the simulator reaches on the Mac's
/// loopback. Skipped unless the run sets, through `TEST_RUNNER_` variables:
/// - `PU_TEST_SSHD`: `user@127.0.0.1:port`
/// - `PU_TEST_SSHD_KEY`: an unencrypted OpenSSH Ed25519 private key the sshd authorizes
/// - `PU_TEST_SSHD_HOST_KEY`: the sshd's `.pub` host key
private enum LiveSSHD {
    static let environment = ProcessInfo.processInfo.environment
    static var isConfigured: Bool {
        ["PU_TEST_SSHD", "PU_TEST_SSHD_KEY", "PU_TEST_SSHD_HOST_KEY"].allSatisfy { environment[$0] != nil }
    }
}

@Suite(.enabled(if: LiveSSHD.isConfigured, "set PU_TEST_SSHD, PU_TEST_SSHD_KEY and PU_TEST_SSHD_HOST_KEY"))
struct CitadelSSHLiveTests {
    /// Connects through the keepalive socket (a refused socket option would fail the connect)
    /// and runs a command on an exec channel.
    @Test func connectsWithKeepaliveAndRunsACommand() async throws {
        let env = LiveSSHD.environment
        let destination = try #require(SSHDestination(sshHost: env["PU_TEST_SSHD"]!))
        let identities = SSHIdentityStore(store: InMemorySecretStore())
        try identities.importOpenSSHPrivateKey(String(contentsOfFile: env["PU_TEST_SSHD_KEY"]!, encoding: .utf8))
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        let machineId = UUID()
        let hostKeyLine = try String(contentsOfFile: env["PU_TEST_SSHD_HOST_KEY"]!, encoding: .utf8)
        try pins.pin(SSHPublicKey(openSSHLine: hostKeyLine.trimmingCharacters(in: .whitespacesAndNewlines)), machineId: machineId)
        let connector = CitadelSSHConnector(
            machineId: machineId, machineName: "live", destination: destination, identities: identities, pins: pins)

        let connection = try await connector.connect(onDisconnect: {})
        defer { connection.close() }
        let channel = try await connection.openExec("echo pu-live-ok")
        var output = Data()
        while true {
            switch await channel.read() {
            case .stdout(let data): output.append(data)
            case .end(let end):
                #expect(end == .exited(0, stderr: ""))
                #expect(String(decoding: output, as: UTF8.self) == "pu-live-ok\n")
                return
            }
        }
    }

    @Test func aKeyTheMachineRefusesReadsAsAuthenticationFailed() async throws {
        let env = LiveSSHD.environment
        let destination = try #require(SSHDestination(sshHost: env["PU_TEST_SSHD"]!))
        let pins = HostKeyPinStore(store: InMemorySecretStore())
        let machineId = UUID()
        let hostKeyLine = try String(contentsOfFile: env["PU_TEST_SSHD_HOST_KEY"]!, encoding: .utf8)
        try pins.pin(SSHPublicKey(openSSHLine: hostKeyLine.trimmingCharacters(in: .whitespacesAndNewlines)), machineId: machineId)
        // A freshly generated identity the sshd has never seen.
        let connector = CitadelSSHConnector(
            machineId: machineId, machineName: "live", destination: destination,
            identities: SSHIdentityStore(store: InMemorySecretStore()), pins: pins)
        await #expect(throws: SSHTransportError.authenticationFailed(machine: "live", user: destination.user)) {
            _ = try await connector.connect(onDisconnect: {})
        }
    }

    @Test func anUnpinnedHostKeyIsStillRefusedBeforeLogin() async throws {
        let env = LiveSSHD.environment
        let destination = try #require(SSHDestination(sshHost: env["PU_TEST_SSHD"]!))
        let connector = CitadelSSHConnector(
            machineId: UUID(), machineName: "live", destination: destination,
            identities: SSHIdentityStore(store: InMemorySecretStore()), pins: HostKeyPinStore(store: InMemorySecretStore()))
        await #expect {
            _ = try await connector.connect(onDisconnect: {})
        } throws: { error in
            if case SSHTrustError.hostKeyUnconfirmed = error { return true }
            return false
        }
    }
}
#endif
