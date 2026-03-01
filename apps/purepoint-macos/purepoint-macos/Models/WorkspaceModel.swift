import Foundation

struct WorktreeModel: Identifiable, Equatable {
    let id: String
    let name: String
    let path: String
    let branch: String
    let status: String
    let tmuxWindow: String
    var agents: [AgentModel]

    static func == (lhs: WorktreeModel, rhs: WorktreeModel) -> Bool {
        lhs.id == rhs.id && lhs.status == rhs.status && lhs.branch == rhs.branch && lhs.agents == rhs.agents
    }
}

struct AgentModel: Identifiable, Equatable {
    let id: String
    let name: String
    let agentType: String
    let status: AgentStatus
    let tmuxTarget: String
    let prompt: String
    let startedAt: String
    let sessionId: String?

    var displayName: String {
        name.isEmpty ? id : name
    }

    init(id: String, name: String, agentType: String, status: AgentStatus, tmuxTarget: String, prompt: String, startedAt: String, sessionId: String? = nil) {
        self.id = id
        self.name = name
        self.agentType = agentType
        self.status = status
        self.tmuxTarget = tmuxTarget
        self.prompt = prompt
        self.startedAt = startedAt
        self.sessionId = sessionId
    }

    init(from entry: AgentEntry) {
        self.init(
            id: entry.id,
            name: entry.name,
            agentType: entry.agentType,
            status: AgentStatus(rawValue: entry.status) ?? .lost,
            tmuxTarget: entry.tmuxTarget,
            prompt: entry.prompt ?? "",
            startedAt: entry.startedAt,
            sessionId: entry.sessionId
        )
    }

    static func == (lhs: AgentModel, rhs: AgentModel) -> Bool {
        lhs.id == rhs.id && lhs.status == rhs.status
    }
}
