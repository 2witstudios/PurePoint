import Foundation

struct WorkspaceSnapshot: Sendable {
    let worktrees: [WorktreeModel]
    let rootAgents: [AgentModel]
    let sessionName: String
}

/// Abstraction boundary for workspace data access.
/// In the no-daemon phase, TmuxWorkspaceService reads manifest.json directly.
/// When the daemon is implemented, a DaemonWorkspaceService replaces this.
nonisolated protocol WorkspaceService: Sendable {
    func loadWorkspace(projectRoot: String) throws -> WorkspaceSnapshot
    func manifestPath(projectRoot: String) -> String
}
