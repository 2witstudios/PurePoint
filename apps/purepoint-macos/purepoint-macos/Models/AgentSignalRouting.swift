import Foundation

nonisolated struct AgentSignalDestination: Equatable, Sendable {
    /// ProjectKey.id includes the daemon machine identity for remote roots.
    let projectID: String
    let worktreeID: String?
    let agentID: String
    let launchID: String
    let generation: UInt64
    let nativeSessionID: String
    let eventID: String

    func matches(_ snapshot: AgentSignalSnapshot?, now: Date = Date()) -> Bool {
        guard let snapshot else { return false }
        return snapshot.identity.agent_id == agentID
            && snapshot.identity.worktree_id == worktreeID
            && snapshot.identity.launch_id == launchID
            && snapshot.identity.generation == generation
            && snapshot.identity.native_session_id == nativeSessionID
            && snapshot.presentation(agentID: agentID, sessionID: nativeSessionID, now: now).eventID == eventID
    }
}

nonisolated enum AgentSignalRouting {
    nonisolated struct Target: Equatable, Sendable {
        let projectID: String
        let worktreeID: String?
        let workspaceID: String
        let agentID: String?
        let leafID: Int
        let surfaceID: Int
        var workspaceOrdinal = 0
    }
    static func target(for destination: AgentSignalDestination, among targets: [Target]) -> Target? {
        let scoped = targets.filter {
            $0.projectID == destination.projectID && $0.worktreeID == destination.worktreeID
                && $0.agentID == destination.agentID
        }
        guard scoped.count == 1, let target = scoped.first else { return nil }
        // Existing workspace navigation is ID-based. Refuse a cross-project ID collision.
        let owners = Set(targets.filter { $0.workspaceID == target.workspaceID }.map {
            [$0.projectID, $0.worktreeID, String($0.workspaceOrdinal)]
        })
        guard owners.count == 1 else { return nil }
        return target
    }
}

nonisolated enum AgentSignalPresence {
    static func isVisible(
        appActive: Bool, keyWindow: Bool, settingsVisible: Bool,
        selectedWorkspaceID: String?, activeWorkspaceID: String?, focusedAgentID: String?,
        activeProjectID: String?, destination: AgentSignalDestination
    ) -> Bool {
        appActive && keyWindow && !settingsVisible && selectedWorkspaceID != nil
            && selectedWorkspaceID == activeWorkspaceID && focusedAgentID == destination.agentID
            && activeProjectID == destination.projectID
    }
}
