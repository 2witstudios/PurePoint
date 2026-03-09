import Foundation

enum AgentSource: String, Sendable {
    case claude, codex, opencode
}

nonisolated struct Conversation: Identifiable, Hashable, Sendable {
    let sessionId: String
    let agentSource: AgentSource
    let title: String
    let previewSnippets: [String]
    let projectPath: String?
    let purePointProjectRoot: String?
    let gitBranch: String?
    let transcriptPath: String?
    let createdAt: Date?
    let modifiedAt: Date
    let messageCount: Int?

    var id: String { "\(agentSource.rawValue):\(sessionId)" }

    var projectName: String {
        let referencePath = purePointProjectRoot ?? projectPath ?? NSHomeDirectory()
        return URL(fileURLWithPath: referencePath).lastPathComponent
    }

    var workspaceName: String {
        guard let projectPath else { return "~" }
        return URL(fileURLWithPath: projectPath).lastPathComponent
    }

    func withSnippets(_ snippets: [String]) -> Conversation {
        Conversation(
            sessionId: sessionId,
            agentSource: agentSource,
            title: title,
            previewSnippets: snippets,
            projectPath: projectPath,
            purePointProjectRoot: purePointProjectRoot,
            gitBranch: gitBranch,
            transcriptPath: transcriptPath,
            createdAt: createdAt,
            modifiedAt: modifiedAt,
            messageCount: messageCount
        )
    }
}
