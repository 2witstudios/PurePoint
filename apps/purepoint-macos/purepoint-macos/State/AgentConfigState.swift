import Foundation
import Observation

@Observable
@MainActor
final class AgentConfigState {
    var agents: [AgentConfigPayload] = []
    var defaultAgent: String = "claude"
    var isLoading = false
    var error: String?
    var codexYolo = false
    var agentStatusTracking = true
    /// Machine-level ("all projects") launch args string per agent type.
    var machineLaunchArgs: [String: String] = [:]
    var globalSettingsLoaded = false
    var isSavingGlobalSettings = false
    var globalError: String?

    @ObservationIgnored private let ensureDaemon: () async throws -> Void
    @ObservationIgnored private let sendRequest: (DaemonRequest) async throws -> DaemonResponse

    #if os(macOS)
    init(
        ensureDaemon: @escaping () async throws -> Void = { try await DaemonLifecycle.ensureDaemon() },
        sendRequest: ((DaemonRequest) async throws -> DaemonResponse)? = nil
    ) {
        self.ensureDaemon = ensureDaemon
        let client = DaemonEndpoint.local.makeClient()
        self.sendRequest = sendRequest ?? { try await client.send($0) }
    }
    #else
    /// iOS has no local daemon to start. `AppState` builds one per machine on that machine's
    /// client (IPAD-012); these defaults report that no daemon is running.
    init(
        ensureDaemon: @escaping () async throws -> Void = { throw DaemonClientError.notRunning },
        sendRequest: ((DaemonRequest) async throws -> DaemonResponse)? = nil
    ) {
        self.ensureDaemon = ensureDaemon
        self.sendRequest = sendRequest ?? { _ in throw DaemonClientError.notRunning }
    }
    #endif

    func loadGlobalSettings() async {
        await sendGlobalSettings(.getGlobalAgentSettings)
    }

    func updateGlobalSettings(codexYolo: Bool, projectRoot: String?) async {
        guard !isSavingGlobalSettings else { return }
        isSavingGlobalSettings = true
        await sendGlobalSettings(.updateGlobalAgentSettings(codexYolo: codexYolo))
        if let projectRoot { await load(projectRoot: projectRoot) }
        isSavingGlobalSettings = false
    }

    /// Save the machine-level launch args for one agent type; a blank string removes them.
    func updateMachineLaunchArgs(agentName: String, launchArgs: String, projectRoot: String?) async {
        guard !isSavingGlobalSettings else { return }
        isSavingGlobalSettings = true
        await sendGlobalSettings(.updateGlobalAgentSettings(launchArgs: [agentName: launchArgs]))
        if let projectRoot { await load(projectRoot: projectRoot) }
        isSavingGlobalSettings = false
    }

    func updateAgentStatusTracking(_ enabled: Bool) async {
        guard !isSavingGlobalSettings else { return }
        isSavingGlobalSettings = true
        await sendGlobalSettings(.updateGlobalAgentSettings(agentStatusTracking: enabled))
        isSavingGlobalSettings = false
    }

    private func sendGlobalSettings(_ request: DaemonRequest) async {
        globalError = nil
        do {
            try await ensureDaemon()
            switch try await sendRequest(request) {
            case .globalAgentSettingsReport(let yolo, let tracking, let launchArgs):
                codexYolo = yolo
                agentStatusTracking = tracking
                globalSettingsLoaded = true
                if let launchArgs {
                    machineLaunchArgs = launchArgs
                } else if case .updateGlobalAgentSettings(_, _, .some) = request {
                    globalError = Self.oldDaemonMessage("save launch args for all projects")
                }
            case .error(_, let message):
                globalError = message
            default:
                globalError = "Unexpected response. Update the PurePoint daemon to use global settings."
            }
        } catch {
            globalError = error.localizedDescription
        }
    }

    func load(projectRoot: String) async {
        isLoading = true
        error = nil
        do {
            let response = try await sendRequest(.getConfig(projectRoot: projectRoot))
            switch response {
            case .configReport(let defaultAgent, let agents):
                applyConfigReport(defaultAgent: defaultAgent, agents: agents)
            case .error(_, let message):
                self.error = message
            default:
                self.error = "Unexpected response"
            }
        } catch {
            self.error = error.localizedDescription
        }
        isLoading = false
    }

    /// Save the project's launch args string; `nil` clears it so the machine default applies.
    func updateLaunchArgs(projectRoot: String, agentName: String, launchArgs: String?) async {
        error = nil
        do {
            let response = try await sendRequest(
                .updateAgentConfig(projectRoot: projectRoot, agentName: agentName, launchArgs: launchArgs))
            switch response {
            case .configReport(let defaultAgent, let agents):
                applyConfigReport(defaultAgent: defaultAgent, agents: agents)
            case .error(_, let message):
                self.error = message
            default:
                self.error = "Unexpected response"
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func applyConfigReport(defaultAgent: String, agents: [AgentConfigPayload]) {
        self.defaultAgent = defaultAgent
        self.agents = agents
        // Daemons before protocol 11 omit `resolved_command` and expect list-shaped launch args.
        if agents.contains(where: { $0.resolvedCommand == nil }) {
            self.error = Self.oldDaemonMessage("edit launch args")
        }
    }

    private static func oldDaemonMessage(_ action: String) -> String {
        "Update the PurePoint daemon to \(action). Restart it after updating."
    }

    func agentConfig(named name: String) -> AgentConfigPayload? {
        agents.first { $0.name == name }
    }
}
