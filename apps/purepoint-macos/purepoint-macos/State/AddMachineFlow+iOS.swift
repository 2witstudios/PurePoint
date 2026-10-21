#if os(iOS)
import Foundation
import Observation

/// Add Machine on iPad (REQ-IPAD-029, REQ-IPAD-030): collect a name and `user@host[:port]`,
/// connect with the device identity, have the user confirm the host key on first use, probe
/// the daemon through the relay, and only then save the machine, which connects and lists its
/// projects. `AddMachineSheet` and the DEBUG `-PUDebugAddMachine` launch argument both drive
/// this one flow.
@Observable
@MainActor
final class AddMachineFlow {
    enum Step: Equatable {
        case editing
        case connecting
        /// First use: the user compares this key's fingerprint and confirms or cancels.
        case confirmHostKey(SSHPublicKey)
        case probing
        /// Test Connection succeeded; nothing was saved.
        case tested
        case saved
    }

    var name = ""
    var host = ""
    var port = "\(SSHDestination.defaultPort)"
    var user = ""
    private(set) var step: Step = .editing
    /// Why the last attempt stopped, shown in the sheet.
    private(set) var error: String?
    /// What the probe found, when it saved a machine whose daemon is not running.
    private(set) var notice: String?

    /// The pin is keyed by the id the machine will be saved under.
    let machineId = UUID()
    private let pins: HostKeyPinStore
    private let identities: SSHIdentityStore
    private let connectorFactory: (Machine) -> any SSHConnector
    private let saveMachine: (AppState, Machine) async throws -> Void
    private var session: SSHMachineSession?
    private var saved = false
    /// Whether the attempt in progress saves the machine (Add) or only tests it.
    private var saving = true
    /// The destination a key was pinned for in this sheet (by Test Connection, say). Editing
    /// the destination afterwards drops that pin: it belongs to the other host.
    private var pinnedFor: String?

    init(
        pins: HostKeyPinStore = .shared,
        identities: SSHIdentityStore = .shared,
        connectorFactory: ((Machine) -> any SSHConnector)? = nil,
        saveMachine: ((AppState, Machine) async throws -> Void)? = nil
    ) {
        self.pins = pins
        self.identities = identities
        self.connectorFactory = connectorFactory ?? { MachineConnection.connector(for: $0) }
        self.saveMachine =
            saveMachine ?? { appState, machine in
                try await appState.addMachine(name: machine.name, sshHost: machine.sshHost, id: machine.id)
            }
    }

    var isBusy: Bool { step == .connecting || step == .probing }

    /// The device public key to authorize on the machine.
    var publicKeyLine: String? { try? identities.identity().authorizedKeyLine }

    /// The machine as it would be saved, or why the fields cannot make one.
    func candidate() -> Result<Machine, Error> {
        let name = name.trimmingCharacters(in: .whitespaces)
        let host = host.trimmingCharacters(in: .whitespaces)
        let user = user.trimmingCharacters(in: .whitespaces)
        do {
            try MachineRegistryStore.validate(name: name)
            guard SSHDestination.isValid(user: user) else { throw AddMachineError.invalidUser }
            let bare = host.hasPrefix("[") && host.hasSuffix("]") ? String(host.dropFirst().dropLast()) : host
            guard SSHDestination.isValid(host: bare) else { throw MachineRegistryError.invalidHost(host) }
            let trimmedPort = port.trimmingCharacters(in: .whitespaces)
            guard let port = trimmedPort.isEmpty ? SSHDestination.defaultPort : Int(trimmedPort),
                (1...65_535).contains(port)
            else { throw AddMachineError.invalidPort }
            let destination = SSHDestination(user: user, host: bare, port: port)
            try MachineRegistryStore.validate(sshHost: destination.sshHost)
            return .success(Machine(id: machineId, name: name, sshHost: destination.sshHost))
        } catch {
            return .failure(error)
        }
    }

    /// Fill the fields from `user@host[:port]`, as the DEBUG launch argument does.
    func fill(sshHost: String, name: String?) -> Bool {
        guard let destination = SSHDestination(sshHost: sshHost) else { return false }
        user = destination.user
        host = destination.host
        port = "\(destination.port)"
        self.name = name ?? AddMachineFlow.suggestedName(destination.host)
        return true
    }

    /// A machine name from a host: its first label, cleaned to the allowed characters.
    static func suggestedName(_ host: String) -> String {
        // An IP address names no machine; a host name's first label does.
        let isAddress = host.contains(":") || host.allSatisfy { $0.isNumber || $0 == "." }
        let label = isAddress ? "machine" : (host.split(separator: ".").first.map(String.init) ?? host)
        let cleaned = String(label.unicodeScalars.filter {
            $0.isASCII && (CharacterSet.alphanumerics.contains($0) || "._-".unicodeScalars.contains($0))
        })
        return cleaned.isEmpty || !(cleaned.first?.isLetter == true || cleaned.first?.isNumber == true)
            ? "machine" : cleaned
    }

    /// Connect: stops at host-key confirmation on first use, otherwise probes, then saves the
    /// machine (`save`) or only reports the result (Test Connection).
    func submit(appState: AppState, save: Bool = true) async {
        guard !isBusy, !saved else { return }
        saving = save
        error = nil
        notice = nil
        let machine: Machine
        switch candidate() {
        case .success(let value): machine = value
        case .failure(let failure):
            error = failure.localizedDescription
            return
        }
        if appState.machines.contains(where: { $0.machine.name.caseInsensitiveCompare(machine.name) == .orderedSame }) {
            error = MachineRegistryError.duplicateName(machine.name).localizedDescription
            return
        }
        if let pinnedFor, pinnedFor != machine.sshHost {
            try? pins.reset(machineId: machineId)
            self.pinnedFor = nil
        }
        step = .connecting
        await closeSession()
        let session = SSHMachineSession(connector: connectorFactory(machine))
        self.session = session
        do {
            try await session.connect()
        } catch SSHTrustError.hostKeyUnconfirmed(let key) {
            step = .confirmHostKey(key)
            #if DEBUG
            if DebugLaunchArguments.current.trustHostKey { await confirmHostKey(appState: appState) }
            #endif
            return
        } catch {
            fail(error)
            return
        }
        await probeAndSave(machine, session: session, appState: appState)
    }

    /// Replace the device identity with an imported OpenSSH Ed25519 private key. Machines that
    /// are offline (say, because they refused the old key) reconnect with the new one.
    func importPrivateKey(_ text: String, appState: AppState) {
        do {
            try identities.importOpenSSHPrivateKey(text)
        } catch {
            self.error = error.localizedDescription
            return
        }
        error = nil
        notice = "Imported. This iPad now signs in with the imported key."
        for machine in appState.machines {
            if case .offline = machine.state { machine.reconnectNow() }
        }
    }

    /// The user confirmed the presented key: pin it, authenticate and probe.
    func confirmHostKey(appState: AppState) async {
        guard case .confirmHostKey(let key) = step, case .success(let machine) = candidate() else { return }
        do {
            try pins.pin(key, machineId: machineId)
            pinnedFor = machine.sshHost
        } catch {
            fail(error)
            return
        }
        step = .connecting
        await closeSession()
        let session = SSHMachineSession(connector: connectorFactory(machine))
        self.session = session
        do {
            try await session.connect()
        } catch {
            fail(error)
            return
        }
        await probeAndSave(machine, session: session, appState: appState)
    }

    /// Leave the sheet without saving: a pin made for an unsaved machine is dropped.
    func cancel() async {
        await closeSession()
        if !saved {
            try? pins.reset(machineId: machineId)
            pinnedFor = nil
            step = .editing
        }
    }

    // MARK: - Private

    private func probeAndSave(_ machine: Machine, session: SSHMachineSession, appState: AppState) async {
        step = .probing
        SSHMachineSessions.shared.register(session, machineId: machine.id, machineName: machine.name)
        let client = DaemonEndpoint.machine(machine.id).makeClient()
        var stoppedNotice: String?
        do {
            let response = try await Deadline.run(
                seconds: MachineConnection.probeTimeout + SSHExecTransport.connectTimeout, cancellable: true,
                onTimeout: DaemonClientError.requestTimeout
            ) {
                try await client.send(.health)
            } discard: { _ in }
            switch MachineHealth.classify(response) {
            case .running, .busy:
                break
            case .incompatible(let version):
                stoppedNotice = "PurePoint on \(machine.name) speaks protocol \(version); update it there to use its projects."
            case .failed:
                if case .unknown(type: "parse_error") = response {
                    throw AddMachineError.unexpectedOutput(machine.name)
                }
                throw AddMachineError.unexpectedResponse(machine.name)
            }
        } catch DaemonClientError.notRunning {
            // SSH and the relay work; only the daemon is down. Saved, with Start daemon offered.
            stoppedNotice = "The daemon on \(machine.name) is not running. Start it from the machine's row."
        } catch {
            SSHMachineSessions.shared.unregister(machineId: machine.id, session: session)
            fail(error)
            return
        }
        SSHMachineSessions.shared.unregister(machineId: machine.id, session: session)
        await closeSession()
        guard saving else {
            notice = stoppedNotice ?? "Connected. PurePoint is running on \(machine.name)."
            step = .tested
            return
        }
        do {
            try await saveMachine(appState, machine)
        } catch {
            fail(error)
            return
        }
        saved = true
        notice = stoppedNotice
        step = .saved
    }

    private func fail(_ failure: Error) {
        error = failure.localizedDescription
        step = .editing
        Task { await closeSession() }
        // A key confirmed for a machine that was not saved is not kept.
        if !saved {
            try? pins.reset(machineId: machineId)
            pinnedFor = nil
        }
    }

    private func closeSession() async {
        guard let session else { return }
        self.session = nil
        await session.disconnect()
    }
}

nonisolated enum AddMachineError: LocalizedError, Equatable {
    case invalidUser
    case invalidPort
    case unexpectedOutput(String)
    case unexpectedResponse(String)

    var errorDescription: String? {
        switch self {
        case .invalidUser: "Enter the user name to log in as, without spaces, '@' or ':'."
        case .invalidPort: "Use a port from 1 to 65535."
        case .unexpectedOutput(let machine):
            "Unexpected output from the login shell on \(machine). Make sure your shell startup files print nothing for non-interactive sessions."
        case .unexpectedResponse(let machine): "The daemon on \(machine) did not answer the health check."
        }
    }
}
#endif
