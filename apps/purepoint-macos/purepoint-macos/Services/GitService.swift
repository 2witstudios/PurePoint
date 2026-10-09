import Foundation

nonisolated struct CommandResult: Sendable {
    let stdout: String
    let stderr: String
    let exitCode: Int32
    var success: Bool { exitCode == 0 }
}

actor GitService {
    static let shared = GitService()
    private var cachedGhPath: String?

    init(ghPath: String? = nil) { cachedGhPath = ghPath }

    private func checked(_ args: [String], at path: String) throws -> String {
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

    func fetchBranchReview(at path: String, baseBranch: String?) -> GitBranchReview {
        var review = GitBranchReview()
        do {
            let (base, refs) = try resolvedBase(at: path, requested: baseBranch)
            review.comparisonBase = base
            review.availableBases = refs
            let head = try checked(["rev-parse", "--verify", "HEAD^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            let baseSHA = try checked(["rev-parse", "--verify", "--end-of-options", base + "^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            let merge = try checked(["merge-base", baseSHA, head], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
            guard !merge.isEmpty else { throw GitReviewError(message: "The comparison base and HEAD have no common ancestor.") }
            review.files = try patches([merge, head], at: path)
            let log = try checked(["log", "--format=%H%x00%s%x00%an%x00%aI%x00", baseSHA + ".." + head, "--"], at: path)
            let fields = log.components(separatedBy: "\0")
            var i = 0
            while i + 3 < fields.count {
                review.commits.append(GitCommitInfo(sha: fields[i].trimmingCharacters(in: .newlines), subject: fields[i+1], author: fields[i+2], date: fields[i+3]))
                i += 4
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

    func fetchLocalReview(at path: String) -> GitLocalReview {
        var review = GitLocalReview()
        do {
            review.staged = try patches(["--cached"], at: path)
            review.unstaged = try patches([], at: path)
            let names = try checked(["ls-files", "--others", "--exclude-standard", "-z"], at: path).split(separator: "\0").map(String.init)
            for name in names {
                let result = runGit(["-c", "core.quotePath=true", "diff", "--no-index", "--no-color", "--no-ext-diff", "--no-textconv", "--", "/dev/null", name], cwd: path)
                guard result.exitCode == 0 || result.exitCode == 1 else { throw GitReviewError(message: result.stderr) }
                review.untracked.append(parsePatch(result.stdout, filename: name, status: "??"))
            }
        } catch { review.error = error.localizedDescription }
        return review
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

    func fetchCommitDiff(at path: String, sha: String) throws -> [FileDiff] {
        let commit = try checked(["rev-parse", "--verify", "--end-of-options", sha + "^{commit}"], at: path).trimmingCharacters(in: .whitespacesAndNewlines)
        // First-parent patch for merges; --root also supports the initial commit.
        let parents = try checked(["rev-list", "--parents", "-n", "1", commit], at: path).split(whereSeparator: { $0.isWhitespace })
        if parents.count > 1 { return try patches([String(parents[1]), commit], at: path) }
        return try patches([commit], at: path, root: true)
    }

    /// Existing editor API, preserving its DiffData return shape.
    func fetchUnstagedDiff(worktreePath: String) -> DiffData {
        let local = fetchLocalReview(at: worktreePath)
        return DiffData(files: local.unstaged + local.untracked)
    }

    private func patches(_ revisions: [String], at path: String, root: Bool = false) throws -> [FileDiff] {
        let command = root ? ["diff-tree", "--root", "--no-commit-id", "-r"] : ["diff"]
        let common = ["--no-color", "--no-ext-diff", "--no-textconv", "--find-renames"]
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
        guard entries.count == sections.count else {
            throw GitReviewError(message: "Git paths changed while patches loaded. Refresh to retry.")
        }
        return zip(entries, sections).map { entry, patch in
            parsePatch(patch, filename: entry.name, status: entry.status, oldFilename: entry.old)
        }
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

    func fetchPRListChecked(cwd: String, branch: String?) throws -> [PullRequestInfo] {
        let fields = "number,title,url,state,headRefName,baseRefName,author,labels,reviewDecision,additions,deletions,changedFiles,isDraft,createdAt,updatedAt"
        let result = runGh(["pr", "list", "--json", fields, "--limit", "50"] + (branch.map { ["--head", $0] } ?? []), cwd: cwd)
        guard result.success else { throw GitReviewError(message: result.stderr.nilIfEmpty ?? "GitHub PR lookup failed") }
        return try JSONDecoder().decode([PullRequestInfo].self, from: Data(result.stdout.utf8))
    }

    func fetchPRDiffChecked(cwd: String, prNumber: Int) throws -> DiffData {
        let result = runGh(["pr", "diff", String(prNumber)], cwd: cwd)
        guard result.success else { throw GitReviewError(message: result.stderr.nilIfEmpty ?? "GitHub PR diff failed") }
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
        return DiffData(files: files)
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

    private func runGit(_ args: [String], cwd: String) -> CommandResult {
        runProcess("/usr/bin/git", args: ["--literal-pathspecs"] + args, cwd: cwd)
    }

    private func runGh(_ args: [String], cwd: String) -> CommandResult {
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
        return runProcess(ghPath, args: args, cwd: cwd)
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
        return nil
    }

    private func runProcess(_ path: String, args: [String], cwd: String) -> CommandResult {
        let process = Process()
        process.executableURL = URL(fileURLWithPath: path)
        process.arguments = args
        process.currentDirectoryURL = URL(fileURLWithPath: cwd)

        let stdoutPipe = Pipe()
        let stderrPipe = Pipe()
        process.standardOutput = stdoutPipe
        process.standardError = stderrPipe

        do {
            try process.run()
        } catch {
            return CommandResult(stdout: "", stderr: error.localizedDescription, exitCode: -1)
        }

        // Read stdout and stderr concurrently to avoid pipe buffer deadlock.
        // If we wait for exit first, a process that fills the pipe buffer blocks
        // forever because nobody is draining it.
        nonisolated(unsafe) var stdoutData = Data()
        nonisolated(unsafe) var stderrData = Data()
        let group = DispatchGroup()

        group.enter()
        DispatchQueue.global().async {
            stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
            group.leave()
        }
        group.enter()
        DispatchQueue.global().async {
            stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
            group.leave()
        }

        // Bound remote/auth calls so a stalled gh cannot indefinitely block Git refreshes.
        let timeout = DispatchWorkItem { if process.isRunning { process.terminate() } }
        DispatchQueue.global().asyncAfter(deadline: .now() + 25, execute: timeout)
        process.waitUntilExit()
        timeout.cancel()
        group.wait()

        let stdout = String(data: stdoutData, encoding: .utf8) ?? ""
        let stderr = String(data: stderrData, encoding: .utf8) ?? ""
        return CommandResult(stdout: stdout, stderr: stderr, exitCode: process.terminationStatus)
    }
}

private extension String {
    var nilIfEmpty: String? { isEmpty ? nil : self }
}
