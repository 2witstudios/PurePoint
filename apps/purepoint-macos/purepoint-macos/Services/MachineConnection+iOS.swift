#if os(iOS)
import Foundation

/// iOS twin of the Mac's `MachineConnection`, with the same API `MachineState` drives. Where the
/// Mac keeps a system `ssh -L` forward up, the iPad keeps the machine's SSH session up
/// (`SSHMachineSession`) and every daemon connection is an exec channel on it (IPAD-004). The
/// supervisor reconnects with the session's backoff, probes `health` through the relay, and
/// never starts a daemon (MACH-003). A host key that needs confirmation or changed stops the
/// supervisor until the user decides (IPAD-009).
actor MachineConnection {
    /// There is no forward process on iOS; quitting needs nothing ended synchronously.
    nonisolated struct Forward: Sendable {
        func terminate() {}
    }

    /// How long Start daemon may run, as on the Mac.
    static let startDaemonTimeout: TimeInterval = 60
    /// A probe's deadline.
    static let probeTimeout: TimeInterval = 8

    let machine: Machine
    let endpoint: DaemonEndpoint
    nonisolated let forward = Forward()
    nonisolated let session: SSHMachineSession
    private let onState: @Sendable (MachineConnectionState) -> Void
    private let onTrust: @Sendable (SSHTrustError?) -> Void
    private var supervisor: Task<Void, Never>?
    private var lastReported: MachineConnectionState = .disconnected
    private var generation = 0
    private var tracker = HealthProbeTracker()

    init(
        machine: Machine,
        connector: (any SSHConnector)? = nil,
        onTrust: @escaping @Sendable (SSHTrustError?) -> Void = { _ in },
        onState: @escaping @Sendable (MachineConnectionState) -> Void
    ) {
        self.machine = machine
        self.endpoint = .machine(machine.id)
        self.onState = onState
        self.onTrust = onTrust
        session = SSHMachineSession(connector: connector ?? Self.connector(for: machine))
    }

    /// Citadel with the device identity and this machine's pin. A destination the iPad cannot
    /// use (an ssh config alias) fails every connect with a message saying so.
    nonisolated static func connector(for machine: Machine) -> any SSHConnector {
        guard let destination = machine.destination else { return UnusableDestinationConnector(machine: machine) }
        return CitadelSSHConnector(
            machineId: machine.id, machineName: machine.name, destination: destination,
            identities: .shared, pins: .shared)
    }

    func connect() {
        guard supervisor == nil else { return }
        SSHMachineSessions.shared.register(session, machineId: machine.id, machineName: machine.name)
        generation += 1
        let current = generation
        supervisor = Task { await self.supervise(generation: current) }
    }

    /// Stop probing and reporting but leave the session up, so writes already queued can
    /// still go through it before `disconnect`.
    func stopSupervising() {
        generation += 1
        supervisor?.cancel()
        supervisor = nil
    }

    func disconnect() async {
        generation += 1
        supervisor?.cancel()
        supervisor = nil
        SSHMachineSessions.shared.unregister(machineId: machine.id, session: session)
        await session.disconnect()
        report(.disconnected)
    }

    nonisolated func terminateForQuit() {
        let session = self.session
        Task { await session.disconnect() }
    }

    /// Connect again now, skipping the backoff: after a host key was confirmed or reset, or a
    /// new identity was imported.
    func reconnectNow() async {
        stopSupervising()
        await session.disconnect()
        await session.resetBackoff()
        connect()
    }

    /// Probe now rather than at the next interval — after Start daemon, for instance.
    func refreshNow() async {
        guard supervisor != nil, await session.isConnected else { return }
        await probeAndReport()
    }

    /// Start the daemon on the machine (an explicit user action, MACH-003): the Mac's start
    /// command as an SSH exec, given 60 s, then probe again.
    func startDaemon() async -> String? {
        let result = await Self.runCommand(
            MachineSSHCommands.startDaemonCommand, on: session, machine: machine.name,
            timeout: Self.startDaemonTimeout)
        await refreshNow()
        return result
    }

    /// Run a one-shot command over the session and return nil on exit 0, or why it failed.
    static func runCommand(
        _ command: String, on session: SSHMachineSession, machine: String, timeout: TimeInterval
    ) async -> String? {
        do {
            let opened = try await Deadline.run(
                seconds: SSHExecTransport.connectTimeout, cancellable: true,
                onTimeout: DaemonClientError.connectTimeout
            ) {
                try await session.openChannel(command)
            } discard: { late in
                late.channel.close()
                late.release()
            }
            defer {
                opened.channel.close()
                opened.release()
            }
            let end = try await Deadline.run(
                seconds: timeout, cancellable: true,
                onTimeout: SSHTransportError.connectFailed(machine: machine, detail: "timed out after \(Int(timeout)) s")
            ) {
                while true {
                    if case .end(let end) = await opened.channel.read() { return end }
                }
            } discard: { _ in }
            switch end {
            case .exited(0, _): return nil
            case .exited(SSHRelayErrors.commandNotFoundStatus, _):
                return SSHTransportError.puNotInstalled(machine: machine).localizedDescription
            case .exited(let status, let stderr):
                let detail = SSHRelayErrors.lastLine(stderr)
                return detail.isEmpty ? "exited with status \(status)" : detail
            case .disconnected: return "SSH connection closed"
            case .closedLocally: return "cancelled"
            }
        } catch {
            return error.localizedDescription
        }
    }

    // MARK: - Private

    private func isCurrent(_ generation: Int) -> Bool {
        !Task.isCancelled && generation == self.generation
    }

    private func supervise(generation: Int) async {
        while isCurrent(generation) {
            if !(await session.isConnected) { report(.connecting) }
            do {
                try await session.connect()
            } catch let refusal as SSHTrustError {
                guard isCurrent(generation) else { return }
                // Never retried on its own: the user confirms, compares or resets.
                onTrust(refusal)
                report(.offline(refusal.localizedDescription))
                supervisor = nil
                return
            } catch {
                guard isCurrent(generation) else { return }
                report(.offline(error.localizedDescription))
                // The session paces the next attempt (backoff); a short pause keeps a cancelled
                // sleep from spinning.
                try? await Task.sleep(for: .milliseconds(100))
                continue
            }
            guard isCurrent(generation) else { return }
            onTrust(nil)
            tracker = HealthProbeTracker()
            while isCurrent(generation), await session.isConnected {
                let conclusive = await probeAndReport()
                guard isCurrent(generation) else { return }
                // An inconclusive probe (BUSY, or a failure short of the limit) retries soon.
                let seconds = conclusive ? 10 : 2
                for _ in 0..<seconds where isCurrent(generation) {
                    guard await session.isConnected else { break }
                    try? await Task.sleep(for: .seconds(1))
                }
            }
            guard isCurrent(generation) else { return }
            report(.offline("SSH connection closed"))
        }
    }

    /// One `health` probe through the relay. Returns whether it settled the state.
    @discardableResult
    private func probeAndReport() async -> Bool {
        switch await probe() {
        case .health(let probe):
            guard let availability = tracker.record(probe) else { return false }
            report(.connected(availability))
            return true
        case .noDaemon:
            // The relay ran and found no daemon (exit 69): stopped, never started (IPAD-006).
            tracker = HealthProbeTracker()
            report(.connected(.stopped))
            return true
        case .unusable(let message):
            report(.offline(message))
            return true
        }
    }

    private enum Probe {
        case health(HealthProbe)
        case noDaemon
        /// SSH works but the relay cannot serve: `pu` missing, a failing relay, or a login
        /// shell that writes to stdout.
        case unusable(String)
    }

    private func probe() async -> Probe {
        let client = endpoint.makeClient()
        let machineName = machine.name
        let outcome: Result<DaemonResponse, Error>
        do {
            outcome = .success(
                try await Deadline.run(
                    seconds: Self.probeTimeout, cancellable: true, onTimeout: DaemonClientError.requestTimeout
                ) {
                    try await client.send(.health)
                } discard: { _ in })
        } catch {
            outcome = .failure(error)
        }
        return Self.classify(outcome, machine: machineName)
    }

    private static func classify(_ outcome: Result<DaemonResponse, Error>, machine: String) -> Probe {
        switch outcome {
        case .success(.unknown(type: "parse_error")):
            return .unusable(
                "Unexpected output from the login shell on \(machine). Make sure your shell startup files print nothing for non-interactive sessions.")
        case .success(let response):
            return .health(MachineHealth.classify(response))
        case .failure(DaemonClientError.notRunning):
            return .noDaemon
        case .failure(let error as SSHTransportError):
            return .unusable(error.localizedDescription)
        case .failure:
            return .health(.failed)
        }
    }

    private func report(_ state: MachineConnectionState) {
        guard state != lastReported else { return }
        lastReported = state
        onState(state)
    }
}

/// `health` replies as machine probes see them, on both platforms.
nonisolated enum MachineHealth {
    static func classify(_ response: DaemonResponse?) -> HealthProbe {
        switch response {
        case .healthReport(_, _, let version, _)?:
            version == DaemonProtocol.version ? .running : .incompatible(version)
        case .error(let code, _)? where code == "BUSY":
            .busy
        default:
            .failed
        }
    }
}

/// A registry entry the iPad cannot reach (an ssh config alias).
nonisolated struct UnusableDestinationConnector: SSHConnector {
    let machine: Machine

    func connect(onDisconnect: @escaping @Sendable () -> Void) async throws -> any SSHExecConnection {
        throw SSHTransportError.unsupportedDestination(machine: machine.name)
    }
}
#endif
