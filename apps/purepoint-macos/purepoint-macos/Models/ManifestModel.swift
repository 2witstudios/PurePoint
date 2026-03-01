import Foundation

struct ManifestModel: Codable, Sendable {
    let version: Int
    let projectRoot: String
    let sessionName: String
    let worktrees: [String: WorktreeEntry]
    let agents: [String: AgentEntry]?
    let createdAt: String
    let updatedAt: String
}

struct WorktreeEntry: Codable, Sendable {
    let id: String
    let name: String
    let path: String
    let branch: String
    let baseBranch: String?
    let status: String
    let tmuxWindow: String
    let agents: [String: AgentEntry]
    let createdAt: String
    let mergedAt: String?
}

struct AgentEntry: Codable, Sendable {
    let id: String
    let name: String
    let agentType: String
    let status: String
    let tmuxTarget: String
    let prompt: String?
    let startedAt: String
    let completedAt: String?
    let exitCode: Int?
    let error: String?
    let sessionId: String?
}
