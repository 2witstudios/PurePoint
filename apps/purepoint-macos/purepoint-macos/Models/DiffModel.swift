import Foundation

nonisolated struct DiffData: Sendable {
    let files: [FileDiff]

    static let empty = DiffData(files: [])
}

nonisolated struct FileDiff: Identifiable, Sendable {
    let filename: String
    let statusCode: String  // M, A, D, ??
    let added: Int
    let removed: Int
    let hunks: [Hunk]
    var oldFilename: String? = nil
    var isBinary: Bool = false

    var id: String { filename }
}

nonisolated struct Hunk: Sendable, Equatable {
    let header: String  // e.g. "@@ -10,6 +10,8 @@ func login()"
    let lines: [DiffLine]
}

nonisolated struct DiffLine: Sendable, Equatable {
    let type: LineType
    let content: String  // code without +/- prefix
    let oldLineNo: Int?
    let newLineNo: Int?
}

nonisolated enum LineType: Sendable, Equatable {
    case context, addition, deletion
}

nonisolated struct GitCommitInfo: Identifiable, Sendable, Equatable {
    let sha: String
    let subject: String
    let author: String
    let date: String
    var id: String { sha }
}

nonisolated struct GitReviewSummary: Sendable {
    let commitCount: Int
    let localFileCount: Int
    let error: String?
}

nonisolated struct GitBranchReview: Sendable {
    var comparisonBase = ""
    var availableBases: [String] = []
    var files: [FileDiff] = []
    var commits: [GitCommitInfo] = []
    var error: String?
}

nonisolated struct GitLocalReview: Sendable {
    var staged: [FileDiff] = []
    var unstaged: [FileDiff] = []
    var untracked: [FileDiff] = []
    var error: String?
    var uniquePathCount: Int { Set((staged + unstaged + untracked).map(\.filename)).count }
}

nonisolated struct GitReviewError: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}
