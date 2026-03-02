import Foundation

struct WorkspaceSnapshot: Sendable {
    let worktrees: [WorktreeModel]
    let rootAgents: [AgentModel]
}

/// Abstraction boundary for workspace data access.
/// DaemonWorkspaceService queries the pu-engine daemon via IPC.
nonisolated protocol WorkspaceService: Sendable {
    func loadWorkspace(projectRoot: String) async throws -> WorkspaceSnapshot
    func manifestPath(projectRoot: String) -> String
}
