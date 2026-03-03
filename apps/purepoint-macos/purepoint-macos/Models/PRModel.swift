import Foundation

nonisolated struct PullRequestInfo: Codable, Identifiable, Sendable {
    let number: Int
    let title: String
    let url: String
    let state: String
    let headRefName: String
    let baseRefName: String
    let author: PRAuthor
    let labels: [PRLabel]
    let reviewDecision: String?
    let additions: Int
    let deletions: Int
    let changedFiles: Int
    let isDraft: Bool
    let createdAt: String
    let updatedAt: String

    var id: Int { number }
}

nonisolated struct PRAuthor: Codable, Sendable {
    let login: String
    let name: String?
}

nonisolated struct PRLabel: Codable, Identifiable, Sendable {
    let name: String
    let color: String

    var id: String { name }
}
