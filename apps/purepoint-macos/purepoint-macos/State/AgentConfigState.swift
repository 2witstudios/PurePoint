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
    var globalSettingsLoaded = false
    var isSavingGlobalSettings = false
    var globalError: String?

    @ObservationIgnored private let ensureDaemon: () async throws -> Void
    @ObservationIgnored private let sendRequest: (DaemonRequest) async throws -> DaemonResponse

    init(
        ensureDaemon: @escaping () async throws -> Void = { try await DaemonLifecycle.ensureDaemon() },
        sendRequest: ((DaemonRequest) async throws -> DaemonResponse)? = nil
    ) {
        self.ensureDaemon = ensureDaemon
        let client = DaemonClient()
        self.sendRequest = sendRequest ?? { try await client.send($0) }
    }

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

    private func sendGlobalSettings(_ request: DaemonRequest) async {
        globalError = nil
        do {
            try await ensureDaemon()
            switch try await sendRequest(request) {
            case .globalAgentSettingsReport(let enabled):
                codexYolo = enabled
                globalSettingsLoaded = true
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
                self.defaultAgent = defaultAgent
                self.agents = agents
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

    func updateLaunchArgs(projectRoot: String, agentName: String, launchArgs: [String]?) async {
        error = nil
        do {
            let response = try await sendRequest(
                .updateAgentConfig(projectRoot: projectRoot, agentName: agentName, launchArgs: launchArgs))
            switch response {
            case .configReport(let defaultAgent, let agents):
                self.defaultAgent = defaultAgent
                self.agents = agents
            case .error(_, let message):
                self.error = message
            default:
                self.error = "Unexpected response"
            }
        } catch {
            self.error = error.localizedDescription
        }
    }

    func agentConfig(named name: String) -> AgentConfigPayload? {
        agents.first { $0.name == name }
    }
}
