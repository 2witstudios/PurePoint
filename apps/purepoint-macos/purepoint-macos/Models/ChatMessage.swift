import Foundation

enum ChatRole: Sendable {
    case user, assistant
}

struct ChatMessage: Identifiable, Sendable {
    let id: String
    let role: ChatRole
    let timestamp: Date
    var contentBlocks: [ContentBlock]
    var isStreaming: Bool

    init(id: String = UUID().uuidString, role: ChatRole, timestamp: Date = Date(), contentBlocks: [ContentBlock] = [], isStreaming: Bool = false) {
        self.id = id
        self.role = role
        self.timestamp = timestamp
        self.contentBlocks = contentBlocks
        self.isStreaming = isStreaming
    }
}

enum ToolUseStatus: Sendable {
    case running, completed, failed
}

struct PulseSummary: Sendable {
    let activeAgents: Int
    let recentEvents: [(agent: String, event: String)]
}

enum ContentBlock: Identifiable, Sendable {
    case text(id: String, text: String)
    case codeBlock(id: String, language: String?, code: String)
    case toolUse(id: String, name: String, input: String, status: ToolUseStatus)
    case toolResult(id: String, toolUseId: String, output: String, isError: Bool)
    case pulse(id: String, summary: PulseSummary)

    var id: String {
        switch self {
        case .text(let id, _): id
        case .codeBlock(let id, _, _): id
        case .toolUse(let id, _, _, _): id
        case .toolResult(let id, _, _, _): id
        case .pulse(let id, _): id
        }
    }
}
