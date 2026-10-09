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
        do { review.staged = try patches(["--cached"], at: path) }
        catch { review.stagedError = error.localizedDescription }
        do { review.unstaged = try patches([], at: path) }
        catch { review.unstagedError = error.localizedDescription }
        do {
            let names = try checked(["ls-files", "--others", "--exclude-standard", "-z"], at: path).split(separator: "\0").map(String.init)
            for name in names {
                review.untracked.append(try emptyBaselinePatch(name: name, status: "??", at: path))
            }
        } catch { review.untrackedError = error.localizedDescription }
        review.error = [review.stagedError, review.unstagedError, review.untrackedError].compactMap { $0 }.joined(separator: "\n").nilIfEmpty
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
        // Unmerged index stages cannot be expressed as a staged patch. In the
        // working-file group use stage 2 (ours) as the disclosed conflict baseline.
        let common = ["--no-color", "--no-ext-diff", "--no-textconv", "--find-renames"] + (revisions.isEmpty && !root ? ["--ours"] : [])
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
                let exists = FileManager.default.fileExists(atPath: fullPath)
                    || (try? FileManager.default.destinationOfSymbolicLink(atPath: fullPath)) != nil
                if !hasOurs && exists {
                    file = try emptyBaselinePatch(name: name, status: "U", at: path)
                    file.conflictBaseline = "empty (ours deleted)"
                }
            }
            files.append(file)
        }
        return files.sorted { $0.filename < $1.filename }
    }

    private func emptyBaselinePatch(name: String, status: String, at path: String) throws -> FileDiff {
        let result = runGit(["-c", "core.quotePath=true", "diff", "--no-index", "--no-color", "--no-ext-diff", "--no-textconv", "--", "/dev/null", name], cwd: path)
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
                && !FileManager.default.fileExists(
                    atPath: (worktreePath as NSString).appendingPathComponent(name))
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
            let preview = try await FileIOService.readFile(at: path, limit: 1_000_000)
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

    func fetchPRDiffChecked(cwd: String, prNumber: Int) throws -> DiffData {
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
        runProcess("/usr/bin/git", args: ["--literal-pathspecs"] + args, cwd: cwd, outputLimit: outputLimit)
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
        return runProcess(ghPath, args: args, cwd: cwd, timeoutSeconds: 25)
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

    private func runProcess(_ path: String, args: [String], cwd: String, outputLimit: Int? = nil, timeoutSeconds: TimeInterval? = nil) -> CommandResult {
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
        nonisolated(unsafe) var outputExceededLimit = false
        let group = DispatchGroup()

        group.enter()
        DispatchQueue.global().async {
            if let outputLimit {
                // Fixed-size reads bound capture before decoding. Keep draining
                // after termination so a full pipe cannot prevent process exit.
                while true {
                    let chunk = stdoutPipe.fileHandleForReading.readData(ofLength: 16_384)
                    if chunk.isEmpty { break }
                    if outputExceededLimit { continue }
                    if chunk.count > outputLimit - stdoutData.count {
                        outputExceededLimit = true
                        stdoutData.removeAll(keepingCapacity: false)
                        process.terminate()
                    } else {
                        stdoutData.append(chunk)
                    }
                }
            } else {
                stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
            }
            group.leave()
        }
        group.enter()
        DispatchQueue.global().async {
            stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()
            group.leave()
        }

        // Only remote/auth commands use a deadline. Local Git work may be slow.
        let timeout = timeoutSeconds.map { seconds in
            let work = DispatchWorkItem { if process.isRunning { process.terminate() } }
            DispatchQueue.global().asyncAfter(deadline: .now() + seconds, execute: work)
            return work
        }
        process.waitUntilExit()
        timeout?.cancel()
        group.wait()

        let stdout = String(data: stdoutData, encoding: .utf8) ?? ""
        let stderr = String(data: stderrData, encoding: .utf8) ?? ""
        return CommandResult(
            stdout: stdout, stderr: stderr, exitCode: process.terminationStatus,
            outputExceededLimit: outputExceededLimit)
    }
}

private extension String {
    nonisolated var nilIfEmpty: String? { isEmpty ? nil : self }
}
