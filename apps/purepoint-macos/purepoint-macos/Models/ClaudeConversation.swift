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
}
