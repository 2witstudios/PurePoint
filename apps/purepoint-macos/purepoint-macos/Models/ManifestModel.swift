import Foundation

nonisolated struct ManifestModel: Codable, Sendable {
    let version: Int
    let projectRoot: String
    let worktrees: [String: WorktreeEntry]
    let agents: [String: AgentEntry]
    let createdAt: String
    let updatedAt: String

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        version = try container.decode(Int.self, forKey: .version)
        projectRoot = try container.decode(String.self, forKey: .projectRoot)
        worktrees = try container.decodeIfPresent([String: WorktreeEntry].self, forKey: .worktrees) ?? [:]
        agents = try container.decodeIfPresent([String: AgentEntry].self, forKey: .agents) ?? [:]
        createdAt = try container.decode(String.self, forKey: .createdAt)
        updatedAt = try container.decode(String.self, forKey: .updatedAt)
    }
}

nonisolated struct WorktreeEntry: Codable, Sendable {
    let id: String
    let name: String
    let path: String
    let branch: String
    let baseBranch: String?
    let status: String
    let agents: [String: AgentEntry]
    let createdAt: String
    let mergedAt: String?

    // Explicit CodingKeys document the camelCase wire format
    // matching Rust's #[serde(rename_all = "camelCase")] on types::WorktreeEntry.
    private enum CodingKeys: String, CodingKey {
        case id, name, path, branch, status, agents
        case baseBranch, createdAt, mergedAt
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        name = try container.decode(String.self, forKey: .name)
        path = try container.decode(String.self, forKey: .path)
        branch = try container.decode(String.self, forKey: .branch)
        baseBranch = try container.decodeIfPresent(String.self, forKey: .baseBranch)
        status = try container.decode(String.self, forKey: .status)
        agents = try container.decodeIfPresent([String: AgentEntry].self, forKey: .agents) ?? [:]
        createdAt = try container.decode(String.self, forKey: .createdAt)
        mergedAt = try container.decodeIfPresent(String.self, forKey: .mergedAt)
    }
}

nonisolated struct AgentEntry: Codable, Sendable {
    let id: String
    let name: String
    let agentType: String
    let status: String
    let prompt: String?
    let startedAt: String
    let completedAt: String?
    let exitCode: Int?
    let error: String?
    let pid: Int?
    let sessionId: String?
    let suspended: Bool?

    // Explicit CodingKeys document the camelCase wire format
    // matching Rust's #[serde(rename_all = "camelCase")] on types::AgentEntry.
    private enum CodingKeys: String, CodingKey {
        case id, name, status, prompt, error, pid, suspended
        case agentType, startedAt, completedAt, exitCode, sessionId
    }
}
