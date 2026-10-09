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

    // MARK: - Git Commands

    func fetchUnstagedDiff(worktreePath: String) -> DiffData {
        let statusResult = runGit(["status", "--porcelain"], cwd: worktreePath)
        let numstatResult = runGit(["diff", "--numstat"], cwd: worktreePath)
        let diffResult = runGit(["diff"], cwd: worktreePath)

        // Parse numstat: filename -> (added, removed)
        var numstatMap: [String: (Int, Int)] = [:]
        for line in numstatResult.stdout.components(separatedBy: "\n") where !line.isEmpty {
            let parts = line.split(separator: "\t", maxSplits: 2)
            if parts.count >= 3 {
                numstatMap[String(parts[2])] = (Int(parts[0]) ?? 0, Int(parts[1]) ?? 0)
            }
        }

        // Parse status --porcelain: filename -> statusCode
        var statusMap: [String: String] = [:]
        for line in statusResult.stdout.components(separatedBy: "\n") where !line.isEmpty {
            guard line.count >= 3 else { continue }
            let index = line.index(line.startIndex, offsetBy: 3)
            let file = String(line[index...])
            let x = String(line[line.startIndex...line.startIndex])
            let y = String(line[line.index(line.startIndex, offsetBy: 1)...line.index(line.startIndex, offsetBy: 1)])
            let code = (y == " " || y == "?") ? (x == "?" ? "??" : x) : y
            statusMap[file] = code
        }

        return parseDiffOutput(diffResult.stdout, statusMap: statusMap, numstatMap: numstatMap)
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

    // MARK: - PR Commands

    func fetchPRList(cwd: String, branch: String?) -> [PullRequestInfo] {
        let fields =
            "number,title,url,state,headRefName,baseRefName,author,labels,reviewDecision,additions,deletions,changedFiles,isDraft,createdAt,updatedAt"
        var args = ["pr", "list", "--json", fields, "--limit", "50"]
        if let branch {
            args += ["--head", branch]
        }
        let result = runGh(args, cwd: cwd)
        guard result.success, !result.stdout.isEmpty else { return [] }

        do {
            return try JSONDecoder().decode([PullRequestInfo].self, from: Data(result.stdout.utf8))
        } catch {
            return []
        }
    }

    func fetchPRDiff(cwd: String, prNumber: Int) -> DiffData {
        let result = runGh(["pr", "diff", String(prNumber)], cwd: cwd)
        guard result.success else { return .empty }

        // For PR diffs, infer status and numstat from the diff itself
        var statusMap: [String: String] = [:]
        var numstatMap: [String: (Int, Int)] = [:]

        let sections = result.stdout.components(separatedBy: "diff --git ")
        for section in sections {
            guard !section.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
            let lines = section.components(separatedBy: "\n")
            guard !lines.isEmpty else { continue }

            let filename = extractFilename(from: lines[0])
            guard !filename.isEmpty else { continue }

            // Infer status from diff metadata
            var hasNewFile = false
            var hasDeletedFile = false
            var addCount = 0
            var delCount = 0

            for line in lines {
                if line.hasPrefix("new file mode") { hasNewFile = true }
                if line.hasPrefix("deleted file mode") { hasDeletedFile = true }
                if line.hasPrefix("+") && !line.hasPrefix("+++") { addCount += 1 }
                if line.hasPrefix("-") && !line.hasPrefix("---") { delCount += 1 }
            }

            if hasNewFile {
                statusMap[filename] = "A"
            } else if hasDeletedFile {
                statusMap[filename] = "D"
            } else {
                statusMap[filename] = "M"
            }

            numstatMap[filename] = (addCount, delCount)
        }

        return parseDiffOutput(result.stdout, statusMap: statusMap, numstatMap: numstatMap)
    }

    func isGhAvailable(cwd: String) -> Bool {
        let result = runGh(["auth", "status"], cwd: cwd)
        return result.success
    }

    // MARK: - Diff Parsing

    private func parseDiffOutput(_ diffOutput: String, statusMap: [String: String], numstatMap: [String: (Int, Int)])
        -> DiffData
    {
        var files: [FileDiff] = []
        let fileSections = diffOutput.components(separatedBy: "diff --git ")

        for section in fileSections {
            guard !section.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }

            let sectionLines = section.components(separatedBy: "\n")
            guard !sectionLines.isEmpty else { continue }

            // First line: "a/path b/path"
            let filename = extractFilename(from: sectionLines[0])

            let hunks = parseHunks(Array(sectionLines.dropFirst()))

            let stats = numstatMap[filename] ?? (0, 0)
            let statusCode = statusMap[filename] ?? "M"

            files.append(
                FileDiff(
                    filename: filename,
                    statusCode: statusCode,
                    added: stats.0,
                    removed: stats.1,
                    hunks: hunks
                ))
        }

        // Include files from status with no diff (e.g. untracked)
        for (file, code) in statusMap {
            if !files.contains(where: { $0.filename == file }) {
                let stats = numstatMap[file] ?? (0, 0)
                files.append(
                    FileDiff(
                        filename: file,
                        statusCode: code,
                        added: stats.0,
                        removed: stats.1,
                        hunks: []
                    ))
            }
        }

        return DiffData(files: files)
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
    private func extractFilename(from header: String) -> String {
        let parts = header.split(separator: " ", maxSplits: 1)
        if parts.count >= 2 {
            let bPath = String(parts[1])
            return bPath.hasPrefix("b/") ? String(bPath.dropFirst(2)) : bPath
        } else if parts.count == 1 {
            let aPath = String(parts[0])
            return aPath.hasPrefix("a/") ? String(aPath.dropFirst(2)) : aPath
        }
        return ""
    }

    // MARK: - Process Execution

    func runGit(_ args: [String], cwd: String, outputLimit: Int? = nil) -> CommandResult {
        runProcess("/usr/bin/git", args: args, cwd: cwd, outputLimit: outputLimit)
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

    private func runProcess(_ path: String, args: [String], cwd: String, outputLimit: Int? = nil) -> CommandResult {
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

        process.waitUntilExit()
        group.wait()

        let stdout = String(data: stdoutData, encoding: .utf8) ?? ""
        let stderr = String(data: stderrData, encoding: .utf8) ?? ""
        return CommandResult(
            stdout: stdout, stderr: stderr, exitCode: process.terminationStatus,
            outputExceededLimit: outputExceededLimit)
    }
}
