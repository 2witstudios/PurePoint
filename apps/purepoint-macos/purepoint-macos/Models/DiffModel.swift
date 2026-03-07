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
