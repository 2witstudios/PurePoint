import Foundation
import Observation

@Observable
@MainActor
final class AgentConfigState {
    var agents: [AgentConfigPayload] = []
    var defaultAgent: String = "claude"
    var isLoading = false
    var error: String?

    @ObservationIgnored private let client = DaemonClient()

    func load(projectRoot: String) async {
        isLoading = true
        error = nil
        do {
            let response = try await client.send(.getConfig(projectRoot: projectRoot))
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
            let response = try await client.send(
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
