import Foundation

@main enum SignalRoutingChecks {
    static func main() {
        let scope = AgentSignalDestination(projectID: "machine:remote:/repo", worktreeID: "wt", agentID: "same", launchID: "launch", generation: 2, nativeSessionID: "native", eventID: "event")
        let local = AgentSignalRouting.Target(projectID: "/repo", worktreeID: "wt", workspaceID: "local", agentID: "same", leafID: 0, surfaceID: 0)
        let remote = AgentSignalRouting.Target(projectID: "machine:remote:/repo", worktreeID: "wt", workspaceID: "remote", agentID: "same", leafID: 1, surfaceID: 9)
        precondition(AgentSignalRouting.target(for: scope, among: [local, remote]) == remote)
        precondition(AgentSignalRouting.target(for: scope, among: [local]) == nil)
        precondition(AgentSignalRouting.target(for: scope, among: [remote, remote]) == nil)
        let collision = AgentSignalRouting.Target(projectID: "/repo", worktreeID: "wt", workspaceID: "remote", agentID: "other", leafID: 0, surfaceID: 0)
        precondition(AgentSignalRouting.target(for: scope, among: [collision, remote]) == nil)
        precondition(AgentSignalPresence.isVisible(appActive: true, keyWindow: true, settingsVisible: false, selectedWorkspaceID: "remote", activeWorkspaceID: "remote", focusedAgentID: "same", activeProjectID: scope.projectID, destination: scope))
        precondition(!AgentSignalPresence.isVisible(appActive: true, keyWindow: true, settingsVisible: true, selectedWorkspaceID: "remote", activeWorkspaceID: "remote", focusedAgentID: "same", activeProjectID: scope.projectID, destination: scope))
        precondition(!AgentSignalPresence.isVisible(appActive: true, keyWindow: true, settingsVisible: false, selectedWorkspaceID: nil, activeWorkspaceID: "remote", focusedAgentID: "same", activeProjectID: scope.projectID, destination: scope))
        let sameProjectCollision = AgentSignalRouting.Target(projectID: scope.projectID, worktreeID: "other-worktree", workspaceID: "remote", agentID: "other", leafID: 0, surfaceID: 0, workspaceOrdinal: 1)
        precondition(AgentSignalRouting.target(for: scope, among: [sameProjectCollision, remote]) == nil)
        let now = Date(timeIntervalSince1970: 100)
        func snapshot(generation: UInt64 = 2, session: String = "native", worktree: String? = "wt") -> AgentSignalSnapshot {
            .init(schema_version: 1,
                identity: .init(agent_id: "same", worktree_id: worktree, launch_id: "launch", generation: generation, native_session_id: session),
                activity: "idle", pending_attention: [], outcome: .init(kind: "errored", event_id: "event", turn_id: nil, confidence: "authoritative"),
                observation: .init(state: "current", last_received_at_ms: 99_000, expires_at_ms: 101_000), evidence: nil)
        }
        precondition(scope.matches(snapshot(), now: now))
        precondition(!scope.matches(snapshot(generation: 3), now: now))
        precondition(!scope.matches(snapshot(session: "replacement"), now: now))
        precondition(!scope.matches(snapshot(worktree: nil), now: now))
        precondition(!scope.matches(snapshot(), now: Date(timeIntervalSince1970: 101)))
        let error = AgentSignalAlertPolicy.Agent(identity: "scoped", launch: "launch", presentation: .init(label: "Error", eventID: "event", needsAttention: true), destination: scope)
        let hidden = AgentSignalPresence.isVisible(appActive: true, keyWindow: true, settingsVisible: true, selectedWorkspaceID: "remote", activeWorkspaceID: "remote", focusedAgentID: "same", activeProjectID: scope.projectID, destination: scope)
        var policy = AgentSignalAlertPolicy()
        precondition(policy.project([error], acknowledgedErrors: hidden ? [error.eventKey!] : []).badge == 1)
        let replacement = AgentSignalAlertPolicy.Agent(identity: "scoped", launch: "launch", presentation: error.presentation,
            destination: .init(projectID: scope.projectID, worktreeID: scope.worktreeID, agentID: scope.agentID, launchID: scope.launchID, generation: 3, nativeSessionID: scope.nativeSessionID, eventID: scope.eventID))
        precondition(error.eventKey != replacement.eventKey)
        print("Scoped native routing/presence passed: host/project/worktree, duplicate IDs, ambiguous workspace, hidden Settings/library terminal")
    }
}
