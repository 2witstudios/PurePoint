import Foundation

nonisolated struct ClaudeConversation: Identifiable, Hashable, Sendable {
    let sessionId: String
    let title: String
    let previewSnippets: [String]
    let projectPath: String
    let purePointProjectRoot: String?
    let gitBranch: String?
    let transcriptPath: String
    let createdAt: Date?
    let modifiedAt: Date
    let messageCount: Int?

    var id: String { sessionId }

    var projectName: String {
        let referencePath = purePointProjectRoot ?? projectPath
        return URL(fileURLWithPath: referencePath).lastPathComponent
    }

    var workspaceName: String {
        URL(fileURLWithPath: projectPath).lastPathComponent
    }

    func withSnippets(_ snippets: [String]) -> ClaudeConversation {
        ClaudeConversation(
            sessionId: sessionId,
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
