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
    var previewError: String? = nil
    var previewDeferred: Bool = false
    /// Conflict working-file patches compare against stage 2, or empty if ours deleted the path.
    var conflictBaseline: String? = nil

    /// Commands and immutable disk slices keep bodies out of overview metadata.
    var patchArguments: [String]? = nil
    var patchSlice: PatchSlice? = nil
    var previewVersion: String = ""
    var shouldCollapse: Bool { added + removed > 200 || statusCode == "??" }
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
    var stagedError: String?
    var unstagedError: String?
    var untrackedError: String?
    var uniquePathCount: Int { Set((staged + unstaged + untracked).map(\.filename)).count }
}

nonisolated struct GitReviewError: Error, LocalizedError, Sendable {
    let message: String
    var errorDescription: String? { message }
}

/// A patch snapshot lives only as long as a loaded review/page references it.
nonisolated final class PatchSnapshot: @unchecked Sendable {
    let url: URL
    init(url: URL) { self.url = url }
    deinit { try? FileManager.default.removeItem(at: url) }
}

nonisolated struct PatchSlice: Sendable {
    let snapshot: PatchSnapshot
    let start: UInt64
    let end: UInt64
}

nonisolated struct ReviewPage: Sendable {
    let slice: PatchSlice
    let hunks: [Hunk]
    let nextOffset: UInt64
    let oldLine: Int
    let newLine: Int
    let header: String
    let hasMore: Bool
    let isBinary: Bool
    let hasLongLines: Bool
}

nonisolated struct DiffFileGroup: Sendable {
    let title: String
    let files: [FileDiff]
}

nonisolated struct ReviewFileEntry: Identifiable, Sendable {
    let file: FileDiff
    let group: String
    var startsGroup: Bool = false
    var id: String { group + "\0" + file.filename }
}

extension FileDiff {
    /// Legacy full-patch consumers use the same bounded renderer.
    nonisolated func pageHunks(offset: Int, limit: Int = 200) -> [Hunk] {
        var skipped = 0, remaining = limit, result: [Hunk] = []
        for hunk in hunks {
            if skipped + hunk.lines.count <= offset { skipped += hunk.lines.count; continue }
            let start = max(0, offset - skipped)
            let lines = hunk.lines.dropFirst(start).prefix(remaining).map { line in
                DiffLine(type: line.type,
                         content: line.content.count > 2048 ? String(line.content.prefix(2048)) + " … [long line]" : line.content,
                         oldLineNo: line.oldLineNo, newLineNo: line.newLineNo)
            }
            if !lines.isEmpty { result.append(Hunk(header: hunk.header, lines: lines)); remaining -= lines.count }
            skipped += hunk.lines.count
            if remaining == 0 { break }
        }
        return result
    }
}
