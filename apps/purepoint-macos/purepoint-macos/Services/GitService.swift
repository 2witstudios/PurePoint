import Foundation

nonisolated struct CommandResult: Sendable {
    let stdout: String
    let stderr: String
    let exitCode: Int32
    let outputExceededLimit: Bool

    init(stdout: String, stderr: String, exitCode: Int32, outputExceededLimit: Bool = false) {
        self.stdout = stdout
        self.stderr = stderr
        self.exitCode = exitCode
        self.outputExceededLimit = outputExceededLimit
    }
    var success: Bool { exitCode == 0 }
}

nonisolated enum WorkingTreeDiffError: LocalizedError {
    case git(String)

    var errorDescription: String? {
        switch self {
        case .git(let message): message.trimmingCharacters(in: .whitespacesAndNewlines)
        }
    }
}

actor GitService {
    static let shared = GitService()
    /// git processes and remote host calls block. Each service runs on its own queue rather
    /// than Swift's shared concurrency pool, so a slow machine cannot stall the whole app.
    private nonisolated let executorQueue = DispatchSerialQueue(label: "purepoint.git-service", qos: .userInitiated)
    nonisolated var unownedExecutor: UnownedSerialExecutor { executorQueue.asUnownedSerialExecutor() }
    private var cachedGhPath: String?
    private var branchSummaryCache: [String: GitBranchReview] = [:]
    private var untrackedCache: [String: (stamp: String, file: FileDiff, bytes: Int)] = [:]
    private var untrackedCacheOrder: [String] = []
    private var untrackedCacheBytes = 0

    /// Where this service's files live and its git/gh run: this Mac, or a machine's daemon.
    nonisolated let host: any ProjectHost

    init(ghPath: String? = nil, host: any ProjectHost = LocalProjectHost.shared) {
        cachedGhPath = ghPath
        self.host = host
    }

    /// `attributesOfItem`-style stamp, read through the host.
    private func stamp(_ fullPath: String, includeIdentity: Bool) -> String {
        Self.stamp((try? host.stat(path: fullPath)) ?? .missing, includeIdentity: includeIdentity)
    }

    private static func stamp(_ stat: HostFileStat, includeIdentity: Bool) -> String {
        guard stat.exists else { return "-1/0/" + (includeIdentity ? "0" : "missing") }
        let seconds = Double(stat.modifiedMs) / 1000
        return "\(seconds)/\(stat.size)/" + (includeIdentity ? "\(stat.modifiedMs)" : (stat.isDirectory ? "dir" : "file"))
    }

    /// Stats for many paths with one listing per parent directory instead of a daemon round
    /// trip per path (the remote review polls every few seconds). A directory that cannot be
    /// listed falls back to statting its paths one by one.
    private func batchedStats(_ fullPaths: [String]) -> [String: HostFileStat] {
        var byDirectory: [String: Set<String>] = [:]
        for fullPath in fullPaths {
            byDirectory[(fullPath as NSString).deletingLastPathComponent, default: []].insert(fullPath)
        }
        var stats: [String: HostFileStat] = [:]
        for (directory, wanted) in byDirectory {
            // A cancelled refresh stops making remote round trips.
            if Task.isCancelled { break }
            guard let entries = try? host.list(directory: directory) else {
                for fullPath in wanted { stats[fullPath] = (try? host.stat(path: fullPath)) ?? .missing }
                continue
            }
            for entry in entries {
                let fullPath = (directory as NSString).appendingPathComponent(entry.name)
                guard wanted.contains(fullPath) else { continue }
                stats[fullPath] = HostFileStat(
                    exists: true, isDirectory: entry.isDirectory, size: entry.size, modifiedMs: entry.modifiedMs)
            }
        }
        return stats
    }

    private func exists(_ fullPath: String) -> Bool {
        (try? host.stat(path: fullPath))?.exists ?? false
    }

    private func checked(_ args: [String], at path: String) throws -> String {
        // Multi-command sequences (reviews, patches) stop between commands once cancelled.
        try Task.checkCancellation()
        let result = runGit(args, cwd: path)
        guard result.success else {
            throw GitReviewError(message: result.stderr.trimmingCharacters(in: .whitespacesAndNewlines).nilIfEmpty ?? "Git command failed (exit \(result.exitCode)); the refs may have no common ancestor.")
        }
        return result.stdout
    }

    private func resolvedBase(at path: String, requested: String?) throws -> (String, [String]) {
        let refs = try checked(["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"], at: path)
            .split(separator: "\n").map(String.init).filter { !$0.hasSuffix("/HEAD") }
        if let requested { return (requested, refs) }
        let upstream = runGit(["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"], cwd: path)
        if upstream.success { return (upstream.stdout.trimmingCharacters(in: .whitespacesAndNewlines), refs) }
        let remoteHead = runGit(["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], cwd: path)
        if remoteHead.success { return (remoteHead.stdout.trimmingCharacters(in: .whitespacesAndNewlines), refs) }
        if let base = ["main", "master"].first(where: { refs.contains($0) }) { return (base, refs) }
        throw GitReviewError(message: "Choose a comparison base; no recorded base, upstream or default branch is available.")
    }

    func fetchBranchReview(at path: String, baseBranch: String?, summariesOnly: Bool = false) -> GitBranchReview {
        var review = GitBranchReview()
        do {
            let (base, refs) = try resolvedBase(at: path, requested: baseBranch)
            review.comparisonBase = base
            review.availableBases = refs
            let head = try checked(["rev-parse", "--verify", "HEAD^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            let baseSHA = try checked(["rev-parse", "--verify", "--end-of-options", base + "^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            let merge = try checked(["merge-base", baseSHA, head], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !merge.isEmpty else { throw GitReviewError(message: "The comparison base and HEAD have no common ancestor.") }
            let cacheKey = path + "\0" + base + "\0" + baseSHA + "\0" + head
            if summariesOnly, let cached = branchSummaryCache[cacheKey] {
                review.files = cached.files; review.commits = cached.commits
                return review
            }
            review.files = try patches([merge, head], at: path, summariesOnly: summariesOnly)
            let log = try checked(["log", "--format=%H%x00%s%x00%an%x00%aI%x00", baseSHA + ".." + head, "--"], at: path)
            let fields = log.components(separatedBy: "\0")
            var i = 0
            while i + 3 < fields.count {
                review.commits.append(GitCommitInfo(sha: fields[i].trimmingCharacters(in: .newlines), subject: fields[i+1], author: fields[i+2], date: fields[i+3]))
                i += 4
            }
            if summariesOnly {
                if branchSummaryCache.count >= 8 { branchSummaryCache.removeAll() }
                branchSummaryCache[cacheKey] = review
            }
        } catch {
            review.error = "Cannot compare with \(review.comparisonBase.isEmpty ? (baseBranch ?? "a base") : review.comparisonBase): \(error.localizedDescription)"
            // Still offer an explicit picker when resolution failed.
            if review.availableBases.isEmpty {
                review.availableBases = runGit(["for-each-ref", "--format=%(refname:short)", "refs/heads", "refs/remotes"], cwd: path).stdout.split(separator: "\n").map(String.init)
            }
        }
        return review
    }

    func fetchLocalReview(at path: String, summariesOnly: Bool = false) -> GitLocalReview {
        var review = GitLocalReview()
        do { review.staged = try patches(["--cached"], at: path, summariesOnly: summariesOnly) }
        catch { review.stagedError = error.localizedDescription }
        do { review.unstaged = try patches([], at: path, summariesOnly: summariesOnly) }
        catch { review.unstagedError = error.localizedDescription }
        do {
            let names = try checked(["ls-files", "--others", "--exclude-standard", "-z"], at: path).split(separator: "\0").map(String.init)
            var remaining = 32
            let remoteStats = host.isLocal ? [:] : batchedStats(names.map { (path as NSString).appendingPathComponent($0) })
            for name in names {
                let key = (path as NSString).appendingPathComponent(name)
                if summariesOnly {
                    var file = FileDiff(filename: name, statusCode: "??", added: 0, removed: 0, hunks: [])
                    // An untracked nested repository is listed as `name/`; it has no patch.
                    if !name.hasSuffix("/") {
                        file.patchArguments = ["diff", "--no-index", "--no-color", "--no-ext-diff", "--no-textconv", "--src-prefix=a/", "--dst-prefix=b/", "--", "/dev/null", name]
                    }
                    file.previewVersion = host.isLocal ? fileStamp(name, at: path) : Self.stamp(remoteStats[key] ?? .missing, includeIdentity: true)
                    review.untracked.append(file)
                    continue
                }
                let stamp = host.isLocal ? Self.localStamp(key) : Self.stamp(remoteStats[key] ?? .missing, includeIdentity: false)
                if let cached = untrackedCache[key], cached.stamp == stamp { review.untracked.append(cached.file) }
                else if remaining > 0, !Task.isCancelled {
                    remaining -= 1
                    let file = untrackedPreview(name: name, at: path, knownStat: remoteStats[key])
                    review.untracked.append(file)
                } else {
                    var file = FileDiff(filename: name, statusCode: "??", added: 0, removed: 0, hunks: [])
                    file.previewDeferred = true
                    review.untracked.append(file)
                }
            }
        } catch { review.untrackedError = error.localizedDescription }
        review.error = [review.stagedError, review.unstagedError, review.untrackedError].compactMap { $0 }.joined(separator: "\n").nilIfEmpty
        return review
    }

    /// `knownStat`: a remote stat the caller already has, saving a round trip.
    func untrackedPreview(name: String, at path: String, knownStat: HostFileStat? = nil) -> FileDiff {
        let key = (path as NSString).appendingPathComponent(name)
        let isDirectory: Bool
        let stamp: String
        if host.isLocal {
            let attributes = try? FileManager.default.attributesOfItem(atPath: key)
            isDirectory = attributes?[.type] as? FileAttributeType == .typeDirectory
            stamp = Self.localStamp(key)
        } else {
            let stat = knownStat ?? (try? host.stat(path: key)) ?? .missing
            isDirectory = stat.isDirectory
            stamp = Self.stamp(stat, includeIdentity: false)
        }
        if let cached = untrackedCache[key], cached.stamp == stamp { return cached.file }
        var file: FileDiff
        do {
            guard !isDirectory else { throw FilePreviewError.unsupported }
            file = try emptyBaselinePatch(name: name, status: "??", at: path, outputLimit: 1_000_000)
        } catch {
            file = FileDiff(filename: name, statusCode: "??", added: 0, removed: 0, hunks: [])
            file.previewError = error.localizedDescription
        }
        // Cache explicit/lazy previews too, with a fixed metadata-count and text budget.
        let bytes = file.hunks.reduce(0) { total, hunk in total + hunk.lines.reduce(0) { $0 + $1.content.utf8.count } }
        if let previous = untrackedCache.removeValue(forKey: key) { untrackedCacheBytes -= previous.bytes }
        untrackedCacheOrder.removeAll { $0 == key }
        while untrackedCache.count >= 128 || untrackedCacheBytes + bytes > 8_000_000 {
            guard !untrackedCacheOrder.isEmpty else { break }
            if let removed = untrackedCache.removeValue(forKey: untrackedCacheOrder.removeFirst()) { untrackedCacheBytes -= removed.bytes }
        }
        untrackedCache[key] = (stamp, file, bytes); untrackedCacheOrder.append(key); untrackedCacheBytes += bytes
        return file
    }

    func reviewSummary(at path: String, baseBranch: String?) -> GitReviewSummary {
        var commitCount = 0, localFileCount = 0
        var errors: [String] = []
        do {
            let (base, _) = try resolvedBase(at: path, requested: baseBranch)
            let head = try checked(["rev-parse", "--verify", "HEAD^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            let baseSHA = try checked(["rev-parse", "--verify", "--end-of-options", base + "^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            _ = try checked(["merge-base", baseSHA, head], at: path)
            commitCount = Int(try checked(["rev-list", "--count", baseSHA + ".." + head], at: path).trimmingCharacters(in: .whitespacesAndNewlines)) ?? 0
        } catch { errors.append(error.localizedDescription) }
        do {
            let fields = try checked(["status", "--porcelain=v1", "-z", "--untracked-files=all"], at: path).components(separatedBy: "\0")
            var paths = Set<String>(), i = 0
            while i < fields.count {
                let field = fields[i]; i += 1
                guard field.count >= 3 else { continue }
                paths.insert(String(field.dropFirst(3)))
                if field.prefix(2).contains("R") || field.prefix(2).contains("C") { i += 1 }
            }
            localFileCount = paths.count
        } catch { errors.append(error.localizedDescription) }
        return GitReviewSummary(commitCount: commitCount, localFileCount: localFileCount, error: errors.joined(separator: "\n").nilIfEmpty)
    }

    func fetchCommitDiff(at path: String, sha: String, summariesOnly: Bool = false) throws -> [FileDiff] {
        let commit = try checked(["rev-parse", "--verify", "--end-of-options", sha + "^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
        // First-parent patch for merges; --root also supports the initial commit.
        let parents = try checked(["rev-list", "--parents", "-n", "1", commit], at: path).split(whereSeparator: { $0.isWhitespace })
        if parents.count > 1 { return try patches([String(parents[1]), commit], at: path, summariesOnly: summariesOnly) }
        return try patches([commit], at: path, root: true, summariesOnly: summariesOnly)
    }

    /// Existing editor API, preserving its DiffData return shape.
    func fetchUnstagedDiff(worktreePath: String) -> DiffData {
        let local = fetchLocalReview(at: worktreePath)
        return DiffData(files: local.unstaged + local.untracked)
    }

    private func patches(_ revisions: [String], at path: String, root: Bool = false, summariesOnly: Bool = false) throws -> [FileDiff] {
        let command = root ? ["diff-tree", "--root", "--no-commit-id", "-r"] : ["diff"]
        // Unmerged index stages cannot be expressed as a staged patch. In the
        // working-file group use stage 2 (ours) as the disclosed conflict baseline.
        let common = ["--no-color", "--no-ext-diff", "--no-textconv", "--find-renames", "--src-prefix=a/", "--dst-prefix=b/"] + (revisions.isEmpty && !root ? ["--ours"] : [])
        let names = try checked(command + common + ["--name-status", "-z"] + revisions + ["--"], at: path).components(separatedBy: "\0")
        var entries: [(name: String, status: String, old: String?)] = [], i = 0
        while i + 1 < names.count && !names[i].isEmpty {
            let code = names[i]; i += 1
            let old = names[i]; i += 1
            let renamed = code.hasPrefix("R") || code.hasPrefix("C")
            let name: String
            if renamed { guard i < names.count else { break }; name = names[i]; i += 1 } else { name = old }
            entries.append((name, String(code.prefix(1)), renamed ? old : nil))
        }
        guard !entries.isEmpty else { return [] }
        if summariesOnly {
            let local = !root && (revisions.isEmpty || revisions == ["--cached"])
            let options = local ? ["--raw", "--no-abbrev", "--numstat", "-z"] : ["--numstat", "-z"]
            let stats = try checked(command + common + options + revisions + ["--"], at: path).components(separatedBy: "\0")
            var counts: [String: (Int, Int, Bool)] = [:], identities: [String: String] = [:], index = 0
            while index < stats.count {
                let record = stats[index]; index += 1
                if record.hasPrefix(":") {
                    let fields = record.split(separator: " ")
                    guard fields.count == 5, index < stats.count else { continue }
                    var name = stats[index]; index += 1
                    if fields[4].hasPrefix("R") || fields[4].hasPrefix("C") {
                        guard index < stats.count else { break }
                        name = stats[index]; index += 1
                    }
                    // Mode and per-path blob identities ignore opportunistic whole-index rewrites.
                    identities[name] = fields.joined(separator: "/")
                    continue
                }
                let parts = record.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
                guard parts.count == 3 else { continue }
                var name = String(parts[2])
                if name.isEmpty {
                    guard index + 1 < stats.count else { break }
                    name = stats[index + 1]; index += 2
                }
                counts[name] = (Int(parts[0]) ?? 0, Int(parts[1]) ?? 0, parts[0] == "-")
            }
            // A conflict may also appear as M; retain one path with the U identity.
            let conflicts = Set(entries.filter { $0.status == "U" }.map(\.name))
            var stagedBase = ""
            if revisions == ["--cached"] {
                let head = runGit(["rev-parse", "--verify", "HEAD"], cwd: path)
                stagedBase = head.success ? head.stdout.trimmingCharacters(in: .whitespacesAndNewlines) : "unborn"
            }
            var seen = Set<String>()
            return try entries.compactMap { entry -> FileDiff? in
                guard seen.insert(entry.name).inserted else { return nil }
                let count = counts[entry.name] ?? (0, 0, false)
                let status = conflicts.contains(entry.name) ? "U" : entry.status
                var file = FileDiff(filename: entry.name, statusCode: status, added: count.0, removed: count.1, hunks: [], oldFilename: entry.old, isBinary: count.2)
                let paths = entry.old.map { [$0, entry.name] } ?? [entry.name]
                file.patchArguments = command + common + ["--patch"] + revisions + ["--"] + paths
                file.previewVersion = path + revisions.joined(separator: "/") + "/origin:" + (entry.old ?? "")
                if revisions == ["--cached"] {
                    file.previewVersion += "/HEAD:" + stagedBase + "/index:" + (identities[entry.name] ?? "missing")
                } else if local {
                    file.previewVersion += "/index:" + (identities[entry.name] ?? "missing") + "/working:" + fileStamp(entry.name, at: path)
                }
                if status == "U", revisions.isEmpty, !root {
                    let stages = try checked(["ls-files", "--unmerged", "-z", "--", entry.name], at: path).split(separator: "\0")
                    file.previewVersion += "/stages:" + stages.map(String.init).joined(separator: "\0")
                    let hasOurs = stages.contains { $0.split(separator: "\t", maxSplits: 1).first?.split(separator: " ").last == "2" }
                    file.conflictBaseline = hasOurs ? "ours (index stage 2)" : "empty (ours deleted)"
                    if !hasOurs && exists((path as NSString).appendingPathComponent(entry.name)) {
                        file.patchArguments = ["diff", "--no-index", "--no-color", "--no-ext-diff", "--no-textconv", "--src-prefix=a/", "--dst-prefix=b/", "--", "/dev/null", entry.name]
                    }
                }
                return file
            }.sorted { $0.filename < $1.filename }
        }
        // One patch process per group, rather than one per file on every foreground poll.
        let output = try checked(["-c", "core.quotePath=true"] + command + common + ["--patch"] + revisions + ["--"], at: path)
        var sections: [String] = [], section = ""
        for line in output.components(separatedBy: "\n") {
            if line.hasPrefix("diff --git ") || line.hasPrefix("diff --cc ") || line.hasPrefix("diff --combined ") {
                if !section.isEmpty { sections.append(section) }
                section = line + "\n"
            } else if !section.isEmpty { section += line + "\n" }
        }
        if !section.isEmpty { sections.append(section) }
        let conflicts = Set(entries.filter { $0.status == "U" }.map(\.name))
        let ordinary = entries.filter { $0.status != "U" }
        guard ordinary.count == sections.count else {
            throw GitReviewError(message: "Git paths changed while patches loaded. Refresh to retry.")
        }
        var files = zip(ordinary, sections).map { entry, patch in
            var file = parsePatch(patch, filename: entry.name, status: conflicts.contains(entry.name) ? "U" : entry.status, oldFilename: entry.old)
            if conflicts.contains(entry.name) && revisions.isEmpty { file.conflictBaseline = "ours (index stage 2)" }
            return file
        }
        for name in conflicts where !files.contains(where: { $0.filename == name }) {
            var file = FileDiff(filename: name, statusCode: "U", added: 0, removed: 0, hunks: [])
            if revisions.isEmpty && !root {
                let stages = try checked(["ls-files", "--unmerged", "-z", "--", name], at: path).split(separator: "\0")
                let hasOurs = stages.contains { entry in
                    entry.split(separator: "\t", maxSplits: 1).first?.split(separator: " ").last == "2"
                }
                file.conflictBaseline = hasOurs ? "ours (index stage 2)" : "empty (ours deleted)"
                // Modify/delete conflicts can have no stage 2. --ours then emits no
                // patch, though the surviving working file has reviewable content.
                let fullPath = (path as NSString).appendingPathComponent(name)
                let exists = host.isLocal
                    ? FileManager.default.fileExists(atPath: fullPath)
                        || (try? FileManager.default.destinationOfSymbolicLink(atPath: fullPath)) != nil
                    : self.exists(fullPath)
                if !hasOurs && exists {
                    file = try emptyBaselinePatch(name: name, status: "U", at: path)
                    file.conflictBaseline = "empty (ours deleted)"
                }
            }
            files.append(file)
        }
        return files.sorted { $0.filename < $1.filename }
    }

    private func fileStamp(_ name: String, at path: String) -> String {
        let fullPath = (path as NSString).appendingPathComponent(name)
        guard host.isLocal else { return stamp(fullPath, includeIdentity: true) }
        let attributes = try? FileManager.default.attributesOfItem(atPath: fullPath)
        return "\((attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? -1)/\(attributes?[.size] ?? 0)/\(attributes?[.systemFileNumber] ?? 0)"
    }

    private static func localStamp(_ key: String) -> String {
        let attributes = try? FileManager.default.attributesOfItem(atPath: key)
        return "\((attributes?[.modificationDate] as? Date)?.timeIntervalSince1970 ?? -1)/\(attributes?[.size] ?? 0)/\(attributes?[.type] ?? "missing")"
    }

    /// Generate an on-disk snapshot once; subsequent pages never regenerate Git output.
    func fetchReviewPage(file: FileDiff, at path: String, after previous: ReviewPage? = nil, cursor: ReviewPageCursor? = nil) throws -> ReviewPage {
        try Task.checkCancellation()
        let resume = cursor ?? previous?.continuation
        let slice: PatchSlice
        if let resume { slice = resume.slice }
        else if let stored = file.patchSlice { slice = stored }
        else {
            guard let arguments = file.patchArguments else {
                throw GitReviewError(
                    message: file.filename.hasSuffix("/")
                        ? "This is an untracked nested repository; it has no diff here."
                        : "Refresh to load this file's diff.")
            }
            let snapshot = PatchSnapshot(url: FileManager.default.temporaryDirectory.appendingPathComponent("purepoint-review-\(UUID()).patch"))
            let result = host.run(HostCommand(
                program: "git", executablePath: "/usr/bin/git", args: ["--literal-pathspecs"] + arguments, cwd: path,
                timeoutSeconds: 15, outputFile: snapshot.url))
            try Task.checkCancellation()
            guard result.success || (arguments.contains("--no-index") && result.exitCode == 1) else {
                throw GitReviewError(message: result.stderr.nilIfEmpty ?? "Diff loading timed out or failed. Retry to load this file.")
            }
            let size = try FileManager.default.attributesOfItem(atPath: snapshot.url.path)[.size] as? NSNumber
            let indexed = try indexPatch(snapshot: snapshot, expected: file)
            if let matching = indexed.files.first(where: { $0.filename == file.filename && $0.oldFilename == file.oldFilename }), let selected = matching.patchSlice {
                slice = selected
            } else if size?.uint64Value == 0 || (file.statusCode == "U" && indexed.unmergedPaths.contains(file.filename) && indexed.files.isEmpty) {
                slice = PatchSlice(snapshot: snapshot, start: 0, end: 0)
            } else {
                throw GitReviewError(message: "The file changed while its diff loaded. Refresh to retry.")
            }
        }
        let reader = try PatchLineReader(slice: slice, offset: resume?.nextOffset ?? slice.start)
        var old = resume?.oldLine ?? 0, new = resume?.newLine ?? 0
        var header = resume?.header ?? "", hunks: [Hunk] = [], lines: [DiffLine] = []
        var count = 0, characters = 0, binary = resume?.isBinary ?? file.isBinary, longLines = false
        while let record = try reader.next() {
            try Task.checkCancellation()
            let line = record.text
            if line.hasPrefix("@@ ") {
                if !lines.isEmpty { hunks.append(Hunk(header: header, lines: lines)); lines = [] }
                header = line
                let parts = line.split(separator: " ")
                guard parts.count >= 3 else { continue }
                old = Int(parts[1].dropFirst().split(separator: ",")[0]) ?? 0
                new = Int(parts[2].dropFirst().split(separator: ",")[0]) ?? 0
            } else if line.hasPrefix("Binary files ") || line == "GIT binary patch" { binary = true }
            else if !header.isEmpty {
                let type: LineType
                if line.hasPrefix("+") { type = .addition }
                else if line.hasPrefix("-") { type = .deletion }
                else if line.hasPrefix(" ") { type = .context }
                else { continue }
                let content = String(line.dropFirst()) + (record.truncated ? " … [long line; open full patch]" : "")
                lines.append(DiffLine(type: type, content: content, oldLineNo: type == .addition ? nil : old, newLineNo: type == .deletion ? nil : new))
                if type != .addition { old += 1 }
                if type != .deletion { new += 1 }
                longLines = longLines || record.truncated
                count += 1; characters += content.utf8.count
                if count == 200 || characters >= 32_768 { break }
            }
        }
        if !lines.isEmpty { hunks.append(Hunk(header: header, lines: lines)) }
        return ReviewPage(slice: slice, hunks: hunks, nextOffset: reader.offset, oldLine: old, newLine: new, header: header, hasMore: reader.offset < slice.end, isBinary: binary, hasLongLines: longLines)
    }

    private func emptyBaselinePatch(name: String, status: String, at path: String, outputLimit: Int? = nil) throws -> FileDiff {
        let result = runGit(["-c", "core.quotePath=true", "diff", "--no-index", "--no-color", "--no-ext-diff", "--no-textconv", "--", "/dev/null", name], cwd: path, outputLimit: outputLimit)
        guard !result.outputExceededLimit else { throw FilePreviewError.tooLarge }
        guard result.exitCode == 0 || result.exitCode == 1 else { throw GitReviewError(message: result.stderr) }
        return parsePatch(result.stdout, filename: name, status: status)
    }

    private func parsePatch(_ patch: String, filename: String, status: String, oldFilename: String? = nil) -> FileDiff {
        var hunks: [Hunk] = [], header = "", lines: [DiffLine] = []
        var oldNo = 0, newNo = 0, added = 0, removed = 0
        for line in patch.components(separatedBy: "\n") {
            if line.hasPrefix("@@ ") {
                if !header.isEmpty { hunks.append(Hunk(header: header, lines: lines)) }
                header = line; lines = []
                let parts = line.split(separator: " ")
                oldNo = Int(parts[1].dropFirst().split(separator: ",")[0]) ?? 0
                newNo = Int(parts[2].dropFirst().split(separator: ",")[0]) ?? 0
            } else if !header.isEmpty {
                if line.hasPrefix("+") {
                    lines.append(DiffLine(type: .addition, content: String(line.dropFirst()), oldLineNo: nil, newLineNo: newNo)); newNo += 1; added += 1
                } else if line.hasPrefix("-") {
                    lines.append(DiffLine(type: .deletion, content: String(line.dropFirst()), oldLineNo: oldNo, newLineNo: nil)); oldNo += 1; removed += 1
                } else if line.hasPrefix(" ") {
                    lines.append(DiffLine(type: .context, content: String(line.dropFirst()), oldLineNo: oldNo, newLineNo: newNo)); oldNo += 1; newNo += 1
                }
            }
        }
        if !header.isEmpty { hunks.append(Hunk(header: header, lines: lines)) }
        return FileDiff(filename: filename, statusCode: status, added: added, removed: removed, hunks: hunks,
                        oldFilename: oldFilename, isBinary: patch.components(separatedBy: "\n").contains { $0.hasPrefix("Binary files ") || $0 == "GIT binary patch" })
    }

    // MARK: - Workspace review (HEAD to working tree, including staged and new files)

    func fetchWorkingTreeChanges(worktreePath: String) throws -> [FileDiff] {
        // NUL-delimited paths preserve spaces, tabs, quotes and newlines. Renames are
        // shown as deletion + addition so each row refers to one real path.
        let status = runGit(
            ["status", "--porcelain=v1", "-z", "--untracked-files=all", "--no-renames"], cwd: worktreePath)
        guard status.success else { throw WorkingTreeDiffError.git(status.stderr) }
        let hasHead = runGit(["rev-parse", "--verify", "HEAD"], cwd: worktreePath).success
        let stats = runGit(
            ["diff", "--no-ext-diff", "--no-textconv", "--numstat", "-z", "--no-renames"]
                + (hasHead ? ["HEAD"] : ["--cached"]) + ["--"], cwd: worktreePath)
        guard stats.success else { throw WorkingTreeDiffError.git(stats.stderr) }
        var counts: [String: (Int, Int)] = [:]
        for record in stats.stdout.split(separator: "\0") {
            let parts = record.split(separator: "\t", maxSplits: 2, omittingEmptySubsequences: false)
            if parts.count == 3 { counts[String(parts[2])] = (Int(parts[0]) ?? 0, Int(parts[1]) ?? 0) }
        }
        var codes: [String: String] = [:]
        for record in status.stdout.split(separator: "\0") {
            guard record.count >= 4 else { continue }
            let name = String(record.dropFirst(3))
            // Before the first commit there is no HEAD content to delete. A staged
            // path subsequently removed/renamed contributes no working-tree file.
            if !hasHead
                && !exists((worktreePath as NSString).appendingPathComponent(name))
            {
                continue
            }
            let x = record.first!
            let y = record.dropFirst().first!
            let code = x == "?" ? "??" : String(y == " " ? x : y)
            // rm --cached can report both a tracked deletion and an untracked
            // file at the same path. Keep the tracked change (and its HEAD diff);
            // the on-disk untracked copy remains accessible in Files mode.
            if codes[name] == nil || code != "??" { codes[name] = code }
        }
        return codes.map { name, code in
            let count = counts[name] ?? (0, 0)
            return FileDiff(filename: name, statusCode: code, added: count.0, removed: count.1, hunks: [])
        }.sorted { $0.filename.localizedStandardCompare($1.filename) == .orderedAscending }
    }

    func fetchWorkingTreeFileDiff(worktreePath: String, file: FileDiff) async throws -> FileDiff {
        let hasHead = runGit(["rev-parse", "--verify", "HEAD"], cwd: worktreePath).success
        if file.statusCode == "??" || !hasHead {
            let path = (worktreePath as NSString).appendingPathComponent(file.filename)
            let preview = try await FileIOService.readFile(at: path, limit: 1_000_000, host: host)
            guard !preview.isBinary else { return file }
            var lines = preview.content.components(separatedBy: "\n")
            if lines.last == "" { lines.removeLast() }
            let diffLines = lines.enumerated().map {
                DiffLine(type: .addition, content: $0.element, oldLineNo: nil, newLineNo: $0.offset + 1)
            }
            return FileDiff(
                filename: file.filename, statusCode: file.statusCode, added: lines.count, removed: 0,
                hunks: diffLines.isEmpty ? [] : [Hunk(header: "@@ -0,0 +1,\(lines.count) @@", lines: diffLines)])
        }
        let result = runGit(
            [
                "--literal-pathspecs", "diff", "--no-ext-diff", "--no-textconv", "--no-color", "--no-renames", "HEAD",
                "--", file.filename,
            ],
            cwd: worktreePath, outputLimit: 1_000_000)
        guard !result.outputExceededLimit else { throw FilePreviewError.tooLarge }
        guard result.success else { throw WorkingTreeDiffError.git(result.stderr) }
        // The path comes from status, not a quoted diff header. Parse only the hunks
        // of this one file so code containing "diff --git" cannot create fake rows.
        let hunks = parseHunks(result.stdout.components(separatedBy: "\n"))
        let lines = hunks.flatMap(\.lines)
        return FileDiff(
            filename: file.filename, statusCode: file.statusCode,
            added: lines.filter { $0.type == .addition }.count,
            removed: lines.filter { $0.type == .deletion }.count, hunks: hunks)
    }

    private func parseHunks(_ lines: [String]) -> [Hunk] {
        var hunks: [Hunk] = []
        var currentHunkHeader = ""
        var currentHunkLines: [DiffLine] = []
        var oldLineNo = 0
        var newLineNo = 0
        var inHunk = false

        for line in lines {

            if line.hasPrefix("@@") {
                if inHunk && !currentHunkLines.isEmpty {
                    hunks.append(Hunk(header: currentHunkHeader, lines: currentHunkLines))
                }

                currentHunkHeader = line
                currentHunkLines = []
                inHunk = true

                // Parse "@@ -old,count +new,count @@"
                let scanner = line.dropFirst(3)
                if let plusIdx = scanner.firstIndex(of: "+") {
                    let newPart = scanner[plusIdx...].dropFirst()
                    if let end = newPart.firstIndex(where: { $0 == "," || $0 == " " }) {
                        newLineNo = Int(newPart[newPart.startIndex..<end]) ?? 1
                    } else {
                        newLineNo = Int(newPart) ?? 1
                    }
                }
                if let minusIdx = scanner.firstIndex(of: "-") {
                    let oldPart = scanner[scanner.index(after: minusIdx)...]
                    if let end = oldPart.firstIndex(where: { $0 == "," || $0 == " " }) {
                        oldLineNo = Int(oldPart[oldPart.startIndex..<end]) ?? 1
                    } else {
                        oldLineNo = Int(oldPart) ?? 1
                    }
                }
                continue
            }

            if !inHunk { continue }

            if line.hasPrefix("+") {
                currentHunkLines.append(
                    DiffLine(
                        type: .addition, content: String(line.dropFirst()),
                        oldLineNo: nil, newLineNo: newLineNo
                    ))
                newLineNo += 1
            } else if line.hasPrefix("-") {
                currentHunkLines.append(
                    DiffLine(
                        type: .deletion, content: String(line.dropFirst()),
                        oldLineNo: oldLineNo, newLineNo: nil
                    ))
                oldLineNo += 1
            } else if line.hasPrefix(" ") {
                currentHunkLines.append(
                    DiffLine(
                        type: .context, content: String(line.dropFirst()),
                        oldLineNo: oldLineNo, newLineNo: newLineNo
                    ))
                oldLineNo += 1
                newLineNo += 1
            } else if line.hasPrefix("\\") {
                continue  // "\ No newline at end of file"
            }
        }

        if inHunk && !currentHunkLines.isEmpty {
            hunks.append(Hunk(header: currentHunkHeader, lines: currentHunkLines))
        }

        return hunks
    }

    /// Extract filename from a `diff --git a/path b/path` header line (without the `diff --git ` prefix).
    func fetchPRListChecked(cwd: String, branch: String?) throws -> [PullRequestInfo] {
        let fields = "number,title,url,state,headRefName,baseRefName,author,labels,reviewDecision,additions,deletions,changedFiles,isDraft,createdAt,updatedAt"
        let result = runGh(["pr", "list", "--json", fields, "--limit", "50"] + (branch.map { ["--head", $0] } ?? []), cwd: cwd)
        guard result.success else { throw GitReviewError(message: result.stderr.nilIfEmpty ?? "GitHub PR lookup failed") }
        return try JSONDecoder().decode([PullRequestInfo].self, from: Data(result.stdout.utf8))
    }

    func fetchPRDiffChecked(cwd: String, prNumber: Int, summariesOnly: Bool = false) throws -> DiffData {
        if summariesOnly { return try fetchPRSummary(cwd: cwd, prNumber: prNumber) }
        let result = runGh(["pr", "diff", String(prNumber)], cwd: cwd)
        guard result.success else { throw GitReviewError(message: result.stderr.nilIfEmpty ?? "GitHub PR diff failed") }
        guard result.stdout.isEmpty || result.stdout.hasPrefix("diff --git ") else {
            throw GitReviewError(message: "GitHub returned an unrecognized patch; refresh to retry.")
        }
        // Split only actual file headers, never matching text inside a hunk.
        let sections = result.stdout.components(separatedBy: "\ndiff --git ")
        let files = sections.compactMap { section -> FileDiff? in
            let patch = section.hasPrefix("diff --git ") ? section : "diff --git " + section
            let metadata = patch.components(separatedBy: "\n")
            let target = metadata.first { $0.hasPrefix("+++ ") && !$0.hasPrefix("+++ /dev/null") }
                ?? metadata.first { $0.hasPrefix("--- ") }
            let rename = metadata.first { $0.hasPrefix("rename to ") }
            let header = metadata.first ?? ""
            let raw = rename.map { String($0.dropFirst(10)) } ?? target.map { String($0.dropFirst(4)).components(separatedBy: "\t")[0] }
                ?? prHeaderPath(header)
            guard let raw else { return nil }
            let decoded = decodeGitPath(raw)
            let name = rename != nil ? decoded : (decoded.hasPrefix("a/") || decoded.hasPrefix("b/") ? String(decoded.dropFirst(2)) : decoded)
            let status = patch.contains("\nnew file mode") ? "A" : patch.contains("\ndeleted file mode") ? "D" : rename != nil ? "R" : "M"
            return parsePatch(patch, filename: name, status: status,
                              oldFilename: metadata.first { $0.hasPrefix("rename from ") }.map { decodeGitPath(String($0.dropFirst(12))) })
        }
        guard result.stdout.isEmpty || !files.isEmpty else {
            throw GitReviewError(message: "GitHub patch paths could not be read; refresh to retry.")
        }
        return DiffData(files: files)
    }

    /// Index a PR patch on disk without retaining all its lines or creating text views.
    private func fetchPRSummary(cwd: String, prNumber: Int) throws -> DiffData {
        let snapshot = PatchSnapshot(url: FileManager.default.temporaryDirectory.appendingPathComponent("purepoint-pr-\(UUID()).patch"))
        let result = runGh(["pr", "diff", String(prNumber)], cwd: cwd, outputFile: snapshot.url)
        try Task.checkCancellation()
        guard result.success else { throw GitReviewError(message: result.stderr.nilIfEmpty ?? "GitHub PR diff failed") }
        return DiffData(files: try indexPatch(snapshot: snapshot).files)
    }

    private func indexPatch(snapshot: PatchSnapshot, expected: FileDiff? = nil) throws -> IndexedPatch {
        let size = (try FileManager.default.attributesOfItem(atPath: snapshot.url.path)[.size] as? NSNumber)?.uint64Value ?? 0
        var unmergedPaths = Set<String>(), offset: UInt64 = 0
        if let expected, expected.statusCode == "U" {
            // Unmerged markers use raw filenames, unlike C-quoted patch headers.
            // Match the complete known marker before line parsing so quotes/newlines
            // remain filename bytes rather than being decoded or split as metadata.
            let marker = Data(("* Unmerged path " + expected.filename + "\n").utf8)
            let handle = try FileHandle(forReadingFrom: snapshot.url)
            defer { try? handle.close() }
            if try handle.read(upToCount: marker.count) == marker {
                unmergedPaths.insert(expected.filename); offset = UInt64(marker.count)
            }
        }
        let reader = try PatchLineReader(slice: PatchSlice(snapshot: snapshot, start: 0, end: size), offset: offset)
        var files: [FileDiff] = [], metadata: [String] = [], start: UInt64 = offset
        var added = 0, removed = 0, inHunk = false, binary = false
        func finish(end: UInt64) throws {
            guard !metadata.isEmpty else { return }
            let target = metadata.first { $0.hasPrefix("+++ ") && !$0.hasPrefix("+++ /dev/null") } ?? metadata.first { $0.hasPrefix("--- ") }
            let rename = metadata.first { $0.hasPrefix("rename to ") }
            let raw = rename.map { String($0.dropFirst(10)) } ?? target.map { String($0.dropFirst(4)).components(separatedBy: "\t")[0] } ?? (expected.flatMap { metadata[0] == "diff --git a/\($0.filename) b/\($0.filename)" ? "b/" + $0.filename : nil }) ?? prHeaderPath(metadata[0])
            guard let raw else { throw GitReviewError(message: "GitHub patch paths could not be read; refresh to retry.") }
            let decoded = decodeGitPath(raw)
            let name = rename != nil ? decoded : (decoded.hasPrefix("a/") || decoded.hasPrefix("b/") ? String(decoded.dropFirst(2)) : decoded)
            let status = metadata.contains { $0.hasPrefix("new file mode ") } ? "A" : metadata.contains { $0.hasPrefix("deleted file mode ") } ? "D" : rename != nil ? "R" : "M"
            var file = FileDiff(filename: name, statusCode: status, added: added, removed: removed, hunks: [], oldFilename: metadata.first { $0.hasPrefix("rename from ") }.map { decodeGitPath(String($0.dropFirst(12))) }, isBinary: binary)
            file.patchSlice = PatchSlice(snapshot: snapshot, start: start, end: end)
            file.previewVersion = snapshot.url.path
            files.append(file)
        }
        while let record = try reader.next() {
            try Task.checkCancellation()
            let line = record.text
            if line.hasPrefix("diff --git ") {
                try finish(end: record.start)
                metadata = [line]; start = record.start; added = 0; removed = 0; inHunk = false; binary = false
            } else if metadata.isEmpty {
                throw GitReviewError(message: "Git returned an unrecognized patch; refresh to retry.")
            }
            else if line.hasPrefix("@@ ") { inHunk = true }
            else if inHunk {
                if line.hasPrefix("+") { added += 1 }
                else if line.hasPrefix("-") { removed += 1 }
            } else {
                if metadata.count < 32 { metadata.append(line) }
                if line.hasPrefix("Binary files ") || line == "GIT binary patch" { binary = true }
            }
        }
        try finish(end: size)
        return IndexedPatch(files: files, unmergedPaths: unmergedPaths)
    }

    private func prHeaderPath(_ header: String) -> String? {
        let value = String(header.dropFirst("diff --git ".count))
        if value.hasPrefix("\"") {
            var escaped = false
            for index in value.indices.dropFirst() {
                let char = value[index]
                if char == "\"" && !escaped {
                    return String(value[value.index(after: index)...]).trimmingCharacters(in: .whitespaces)
                }
                if char == "\\" { escaped.toggle() } else { escaped = false }
            }
        }
        if let range = value.range(of: " b/", options: .backwards) { return String(value[value.index(after: range.lowerBound)...]) }
        return nil
    }

    private func decodeGitPath(_ value: String) -> String {
        guard value.hasPrefix("\"") && value.hasSuffix("\"") else { return value }
        let bytes = Array(value.dropFirst().dropLast().utf8)
        var output: [UInt8] = [], i = 0
        while i < bytes.count {
            if bytes[i] != 92 { output.append(bytes[i]); i += 1; continue }
            i += 1; guard i < bytes.count else { break }
            if (48...55).contains(bytes[i]) {
                var n = 0, count = 0
                while i < bytes.count && (48...55).contains(bytes[i]) && count < 3 { n = n * 8 + Int(bytes[i] - 48); i += 1; count += 1 }
                output.append(UInt8(truncatingIfNeeded: n))
            } else {
                output.append([UInt8(110): UInt8(10), 116: 9, 114: 13, 98: 8, 102: 12, 118: 11, 97: 7][bytes[i]] ?? bytes[i]); i += 1
            }
        }
        return String(decoding: output, as: UTF8.self)
    }

    func fetchPRList(cwd: String, branch: String?) -> [PullRequestInfo] { (try? fetchPRListChecked(cwd: cwd, branch: branch)) ?? [] }
    func fetchPRDiff(cwd: String, prNumber: Int) -> DiffData { (try? fetchPRDiffChecked(cwd: cwd, prNumber: prNumber)) ?? .empty }
    func isGhAvailable(cwd: String) -> Bool { runGh(["auth", "status"], cwd: cwd).success }
    // MARK: - Process Execution

    func runGit(_ args: [String], cwd: String, outputLimit: Int? = nil) -> CommandResult {
        host.run(HostCommand(
            program: "git", executablePath: "/usr/bin/git", args: ["--literal-pathspecs"] + args, cwd: cwd,
            outputLimit: outputLimit))
    }

    private func runGh(_ args: [String], cwd: String, outputFile: URL? = nil) -> CommandResult {
        // A remote machine's daemon runs its own gh with that user's auth.
        guard host.isLocal else {
            return host.run(HostCommand(
                program: "gh", executablePath: "gh", args: args, cwd: cwd, timeoutSeconds: 25, outputFile: outputFile))
        }
        let ghPath: String
        if let cached = cachedGhPath {
            ghPath = cached
        } else {
            guard let resolved = locateGh() else {
                return CommandResult(stdout: "", stderr: "gh not found", exitCode: 1)
            }
            cachedGhPath = resolved
            ghPath = resolved
        }
        return host.run(HostCommand(
            program: "gh", executablePath: ghPath, args: args, cwd: cwd, timeoutSeconds: 25, outputFile: outputFile))
    }

    private nonisolated func locateGh() -> String? {
        let candidates = [
            "/opt/homebrew/bin/gh",
            "/usr/local/bin/gh",
        ]
        for path in candidates {
            if FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }

        #if os(macOS)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/which")
        process.arguments = ["gh"]
        let pipe = Pipe()
        process.standardOutput = pipe
        process.standardError = FileHandle.nullDevice
        do {
            try process.run()
        } catch {
            return nil
        }
        process.waitUntilExit()
        if process.terminationStatus == 0 {
            let data = pipe.fileHandleForReading.readDataToEndOfFile()
            let path = String(decoding: data, as: UTF8.self)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !path.isEmpty && FileManager.default.isExecutableFile(atPath: path) {
                return path
            }
        }
        #endif
        return nil
    }
}

private extension String {
    nonisolated var nilIfEmpty: String? { isEmpty ? nil : self }
}

/// Stream a disk patch with bounded memory, even for minified/generated single lines.
private nonisolated final class PatchLineReader {
    struct Record { let text: String; let start: UInt64; let truncated: Bool }
    private let handle: FileHandle
    private let end: UInt64
    private var buffer = Data()
    private var index = 0
    private(set) var offset: UInt64
    init(slice: PatchSlice, offset: UInt64) throws {
        handle = try FileHandle(forReadingFrom: slice.snapshot.url)
        self.offset = offset; end = slice.end
        try handle.seek(toOffset: offset)
    }
    deinit { try? handle.close() }
    func next() throws -> Record? {
        guard offset < end else { return nil }
        let start = offset
        var bytes = Data(), truncated = false
        while offset < end {
            if index == buffer.count {
                buffer = try handle.read(upToCount: Int(min(65_536, end - offset))) ?? Data()
                index = 0
                if buffer.isEmpty { offset = end; break }
            }
            let newline = buffer[index...].firstIndex(of: 10)
            let stop = newline ?? buffer.endIndex
            let available = stop - index
            let retained = min(available, 2048 - bytes.count)
            if retained > 0 { bytes.append(buffer[index..<(index + retained)]) }
            if retained < available { truncated = true }
            offset += UInt64(available); index = stop
            if newline != nil { index += 1; offset += 1; break }
        }
        return Record(text: String(decoding: bytes, as: UTF8.self), start: start, truncated: truncated)
    }
}

private nonisolated struct IndexedPatch {
    let files: [FileDiff]
    let unmergedPaths: Set<String>
}
