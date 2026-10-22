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
    var signal: AgentSignalSnapshot? = nil
    /// The tab this agent was spawned for, if any.
    var placement: SpawnPlacement? = nil

    var displayName: String {
        name.isEmpty ? id : name
    }

    func signalPresentation(available: Bool = true, now: Date = Date()) -> AgentSignalPresentation {
        guard available else { return .unknown }
        if suspended { return .init(label: "Suspended", eventID: nil, needsAttention: false) }
        return signal?.presentation(agentID: id, sessionID: sessionId, available: status.isAlive, now: now) ?? .unknown
    }

    init(
        id: String, name: String, agentType: String, status: AgentStatus, prompt: String, startedAt: String,
        sessionId: String? = nil, suspended: Bool = false, command: String? = nil, signal: AgentSignalSnapshot? = nil,
        placement: SpawnPlacement? = nil
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
        self.signal = signal
        self.placement = placement
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
            command: entry.command,
            placement: entry.placement
        )
        signal = entry.signal
    }
}
