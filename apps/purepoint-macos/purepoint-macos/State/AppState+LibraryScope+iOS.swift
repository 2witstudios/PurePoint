#if os(iOS)
import Foundation
import SwiftUI

/// The library screens' daemon on iPad (IPAD-012). On the Mac, Agents Hub, Schedule, Triggers
/// and agent settings talk to the local daemon. The iPad has none, so they talk to the daemon
/// of the machine that owns the current selection, or, with no machine project selected, to
/// the machine the user picked (the first running machine until they pick one). The state
/// types are the Mac's, one set per machine, built on that machine's endpoint.
extension AppState {
    /// The machine whose daemon the library screens use.
    var libraryMachine: MachineState? {
        selectedLibraryMachine
            ?? pickedLibraryMachineId.flatMap { id in machines.first { $0.machine.id == id } }
            ?? machines.first { $0.state.isDaemonRunning }
            ?? machines.first
    }

    /// Whether the selection names the library machine. When it does not, the screens offer a
    /// machine picker.
    var libraryMachineFollowsSelection: Bool { selectedLibraryMachine != nil }

    private var selectedLibraryMachine: MachineState? {
        guard let activeProjectRoot, let id = ProjectKey(id: activeProjectRoot).machineId else { return nil }
        return machines.first { $0.machine.id == id }
    }

    /// Identifies the library's daemon, so screens reload when it changes.
    var libraryScopeId: UUID? { libraryMachine?.machine.id }

    var agentsHubState: AgentsHubState { libraryStates.hub }
    var agentConfigState: AgentConfigState { libraryStates.agentConfig }
    var scheduleState: ScheduleState { libraryStates.schedule }
    var triggersState: TriggersState { libraryStates.triggers }

    private var libraryStates: LibraryStates {
        guard let machine = libraryMachine else { return noMachineLibraryStates }
        if let states = libraryStatesByMachine[machine.machine.id] { return states }
        let states = LibraryStates(endpoint: machine.endpoint)
        libraryStatesByMachine[machine.machine.id] = states
        return states
    }
}

/// One machine's library states, each talking to that machine's daemon.
@MainActor
final class LibraryStates {
    let hub: AgentsHubState
    let agentConfig: AgentConfigState
    let schedule: ScheduleState
    let triggers: TriggersState

    /// `endpoint` nil: no machine is registered, and every request reports no daemon.
    init(endpoint: DaemonEndpoint?) {
        let client = endpoint?.makeClient() ?? DaemonClient(transport: NoMachineTransport())
        hub = AgentsHubState(client: client)
        agentConfig = AgentConfigState(ensureDaemon: {}, sendRequest: { try await client.send($0) })
        schedule = ScheduleState(client: client)
        triggers = TriggersState(client: client)
    }
}

/// The library's transport while no machine is registered: requests report no daemon,
/// without the local-endpoint trap (there is no stray client to catch, just nothing to reach).
nonisolated struct NoMachineTransport: DaemonTransport {
    func openStream(cancellable: Bool) async throws -> any DaemonByteStream {
        throw DaemonClientError.notRunning
    }

    func openBlockingStream(timeout: TimeInterval) throws -> any DaemonBlockingByteStream {
        throw DaemonClientError.notRunning
    }
}

/// Names the library machine above Agents Hub, Schedule and agent settings, with a picker
/// when the selection names no machine (IPAD-012). The Mac's screens need neither: they only
/// ever show the local daemon.
struct LibraryMachineBar: View {
    @Environment(AppState.self) private var appState

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "desktopcomputer")
                .font(.system(size: 11))
                .foregroundStyle(.secondary)
            if appState.libraryMachineFollowsSelection || appState.machines.count < 2 {
                Text(appState.libraryMachine?.machine.name ?? "No machine")
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(appState.libraryMachine == nil ? .secondary : .primary)
            } else {
                Menu {
                    ForEach(appState.machines, id: \.machine.id) { machine in
                        Button {
                            appState.pickedLibraryMachineId = machine.machine.id
                        } label: {
                            if machine.machine.id == appState.libraryMachine?.machine.id {
                                Label(machine.machine.name, systemImage: "checkmark")
                            } else {
                                Text(machine.machine.name)
                            }
                        }
                    }
                } label: {
                    HStack(spacing: 3) {
                        Text(appState.libraryMachine?.machine.name ?? "Choose a machine")
                            .font(.system(size: 12, weight: .medium))
                        Image(systemName: "chevron.up.chevron.down")
                            .font(.system(size: 8, weight: .semibold))
                    }
                    .foregroundStyle(.primary)
                }
                .menuStyle(.button)
                .buttonStyle(.borderless)
                .fixedSize()
                .accessibilityLabel("Machine")
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, 16)
        .frame(height: 26)
    }
}
#endif
