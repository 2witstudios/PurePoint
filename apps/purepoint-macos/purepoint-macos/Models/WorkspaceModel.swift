import Foundation

// ProjectState gates snapshot updates on equality. Compare every stored property so
// metadata changes (including nested agent renames) reach the sidebar.
nonisolated struct WorktreeModel: Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let path: String
    let branch: String
    let status: String
    var agents: [AgentModel]
    var baseBranch: String? = nil
}

// Synthesized equality includes the name even when the agent's status is unchanged.
nonisolated struct AgentModel: Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    let agentType: String
    let status: AgentStatus
    let prompt: String
    let startedAt: String
    let sessionId: String?
    let suspended: Bool
    let command: String?

    var displayName: String {
        name.isEmpty ? id : name
    }

    init(
        id: String, name: String, agentType: String, status: AgentStatus, prompt: String, startedAt: String,
        sessionId: String? = nil, suspended: Bool = false, command: String? = nil
    ) {
        self.id = id
        self.name = name
        self.agentType = agentType
        self.status = status
        self.prompt = prompt
        self.startedAt = startedAt
        self.sessionId = sessionId
        self.suspended = suspended
        self.command = command
    }

    init(from entry: AgentEntry) {
        self.init(
            id: entry.id,
            name: entry.name,
            agentType: entry.agentType,
            status: entry.status,
            prompt: entry.prompt ?? "",
            startedAt: entry.startedAt,
            sessionId: entry.sessionId,
            suspended: entry.suspended ?? false,
            command: entry.command
        )
    }
}
