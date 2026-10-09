import Foundation

nonisolated struct ChannelAuthor: Codable, Equatable, Sendable {
    let id: String
    let name: String
    let kind: String
    let agentType: String?
    let worktreeId: String?
    let branch: String?
    enum CodingKeys: String, CodingKey {
        case id, name, kind, branch
        case agentType = "agent_type", worktreeId = "worktree_id"
    }
}
nonisolated struct ChannelReference: Codable, Equatable, Sendable {
    let kind: String
    let value: String
    let label: String?
}
nonisolated struct ChannelReaction: Codable, Equatable, Sendable {
    let emoji: String
    let authorIds: [String]
    enum CodingKeys: String, CodingKey { case emoji; case authorIds = "author_ids" }
}
nonisolated struct ChannelMessage: Codable, Identifiable, Equatable, Sendable {
    let id: String
    let sequence: UInt64
    let parentId: String?
    let author: ChannelAuthor
    let text: String
    let createdAt: String
    let editedAt: String?
    let references: [ChannelReference]
    let reactions: [ChannelReaction]
    enum CodingKeys: String, CodingKey {
        case id, sequence, author, text, references, reactions
        case parentId = "parent_id", createdAt = "created_at", editedAt = "edited_at"
    }
}
nonisolated struct ChannelHistory: Decodable, Sendable {
    let messages: [ChannelMessage]
    let revision: UInt64
    let latestSequence: UInt64
    let hasMore: Bool
    let oldestSequence: UInt64?
    let unchanged: Bool
    let selfAuthorId: String
    let replyCounts: [String: UInt64]
    enum CodingKeys: String, CodingKey {
        case messages, revision, unchanged
        case latestSequence = "latest_sequence", hasMore = "has_more", oldestSequence = "oldest_sequence"
        case selfAuthorId = "self_author_id", replyCounts = "reply_counts"
    }
}
nonisolated struct ChannelMutation: Decodable {
    let message: ChannelMessage
    let revision: UInt64
}
