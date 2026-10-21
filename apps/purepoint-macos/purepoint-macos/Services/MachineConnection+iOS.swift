#if os(iOS)
import Foundation

/// iOS twin of the Mac's `MachineConnection`, with the same API `MachineState` drives. The Mac
/// keeps a machine reachable through a system `ssh -L` forward, which iOS cannot run. Until the
/// iPad connects machines over its own SSH transport (IPAD-5.3), connecting reports the machine
/// offline as unavailable on iPad, and nothing is started or probed.
actor MachineConnection {
    /// Stand-in for the Mac's forward process handle: there is no forward process on iOS.
    nonisolated struct Forward: Sendable {
        func terminate() {}
    }

    static let unavailableMessage = "Connecting to machines is not available on iPad yet."

    let machine: Machine
    let endpoint: DaemonEndpoint
    nonisolated let forward = Forward()
    private let onState: @Sendable (MachineConnectionState) -> Void
    private var lastReported: MachineConnectionState = .disconnected

    init(machine: Machine, onState: @escaping @Sendable (MachineConnectionState) -> Void) {
        self.machine = machine
        self.endpoint = .machine(machine.id)
        self.onState = onState
    }

    func connect() {
        report(.offline(Self.unavailableMessage))
    }

    func stopSupervising() {}

    func disconnect() {
        report(.disconnected)
    }

    nonisolated func terminateForQuit() {}

    func refreshNow() async {}

    /// Starting a daemon needs an SSH exec channel; report why it cannot run yet.
    func startDaemon() async -> String? {
        Self.unavailableMessage
    }

    private func report(_ state: MachineConnectionState) {
        guard state != lastReported else { return }
        lastReported = state
        onState(state)
    }
}
#endif
