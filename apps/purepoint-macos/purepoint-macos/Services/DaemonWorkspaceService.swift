import Foundation

nonisolated struct DaemonWorkspaceService: WorkspaceService {
    func manifestPath(projectRoot: String) -> String {
        (projectRoot as NSString)
            .appendingPathComponent(".pu")
            .appending("/manifest.json")
    }

    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot {
        let client = DaemonClient()
        let response = try await client.send(.status(projectRoot: projectRoot))

        switch response {
        case .statusReport(let worktrees, let agents):
            let worktreeModels = Self.parseWorktrees(worktrees)
            let rootAgents = agents.map { report in
                AgentModel(
                    id: report.id,
                    name: report.name,
                    agentType: "claude",
                    status: AgentStatus(rawValue: report.status) ?? .lost,
                    prompt: "",
                    startedAt: "",
                    sessionId: nil
                )
            }
            return WorkspaceSnapshot(worktrees: worktreeModels, rootAgents: rootAgents)
        case .error(_, let message):
            throw DaemonWorkspaceError.daemonError(message)
        default:
            throw DaemonWorkspaceError.unexpectedResponse
        }
    }

    static func parseWorktrees(_ entries: [WorktreeEntry]) -> [WorktreeModel] {
        entries.map { entry in
            let agents = entry.agents.values
                .map { AgentModel(from: $0) }
                .sorted(by: { $0.startedAt < $1.startedAt })

            return WorktreeModel(
                id: entry.id,
                name: entry.name,
                path: entry.path,
                branch: entry.branch,
                status: entry.status,
                agents: agents
            )
        }
    }
}

enum DaemonWorkspaceError: Error, LocalizedError {
    case daemonError(String)
    case unexpectedResponse

    var errorDescription: String? {
        switch self {
        case .daemonError(let msg): msg
        case .unexpectedResponse: "Unexpected response from daemon"
        }
    }
}
