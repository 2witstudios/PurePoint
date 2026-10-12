import Foundation
#if !GIT_REVIEW_HARNESS
import Testing
@testable import PurePoint
#endif

/// Can be run with swiftc -D GIT_REVIEW_HARNESS and the Git/manifest/workspace models,
/// GitService, WorktreeWatcher and DiffState sources, without building/installing the app.
private enum GitReviewChecks {
    struct Failure: Error { let message: String }
    static func require(_ condition: Bool, _ message: String) throws {
        if !condition { throw Failure(message: message) }
    }
    static func git(_ args: [String], at path: String) throws -> String {
        let process = Process(); process.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        process.arguments = args; process.currentDirectoryURL = URL(fileURLWithPath: path)
        let pipe = Pipe(); process.standardOutput = pipe; process.standardError = pipe
        try process.run(); let data = pipe.fileHandleForReading.readDataToEndOfFile(); process.waitUntilExit()
        let result = String(decoding: data, as: UTF8.self)
        try require(process.terminationStatus == 0, "git \(args): \(result)")
        return result.trimmingCharacters(in: .whitespacesAndNewlines)
    }
    static func repo() throws -> String {
        let path = FileManager.default.temporaryDirectory.appendingPathComponent("purepoint-git-\(UUID())").path
        try FileManager.default.createDirectory(atPath: path, withIntermediateDirectories: true)
        _ = try git(["init", "-b", "main"], at: path)
        _ = try git(["config", "user.email", "test@example.test"], at: path)
        _ = try git(["config", "user.name", "Test"], at: path)
        return path
    }
    static func write(_ value: String, _ name: String, at path: String) throws {
        try value.write(toFile: path + "/" + name, atomically: true, encoding: .utf8)
    }
    static func commit(_ message: String, at path: String) throws {
        _ = try git(["add", "--all"], at: path); _ = try git(["commit", "-m", message], at: path)
    }
    static func repositoryChecks() async throws {
        let path = try repo(); defer { try? FileManager.default.removeItem(atPath: path) }
        let service = GitService()
        try write("original\n", "same.txt", at: path)
        try write("remove\n", "deleted.txt", at: path)
        try write("rename\n", "old name.txt", at: path)
        try commit("initial", at: path)
        let root = try git(["rev-parse", "HEAD"], at: path)
        let initialPatch = try await service.fetchCommitDiff(at: path, sha: root)
        try require(initialPatch.count == 3, "initial commit patches")
        _ = try git(["checkout", "-b", "feature"], at: path)
        try write("branch\n", "same.txt", at: path); try commit("branch one", at: path)
        let first = try git(["rev-parse", "HEAD"], at: path)
        try write("branch two\n", "other.txt", at: path); try commit("branch two", at: path)
        _ = try git(["checkout", "main"], at: path)
        try write("base advanced\n", "base-only.txt", at: path); try commit("base advances", at: path)
        _ = try git(["checkout", "feature"], at: path)
        let branch = await service.fetchBranchReview(at: path, baseBranch: "main")
        try require(branch.error == nil && branch.commits.count == 2, "base-exclusive commits")
        try require(Set(branch.files.map(\.filename)) == ["same.txt", "other.txt"], "cumulative merge-base patch excludes base-only changes")
        let selected = try await service.fetchCommitDiff(at: path, sha: first)
        try require(selected.map(\.filename) == ["same.txt"], "selected commit patch")
        try write("staged\n", "same.txt", at: path); _ = try git(["add", "same.txt"], at: path)
        try write("unstaged\n", "same.txt", at: path)
        let quoted = "quoted \"name\"\tline\nü.txt"
        try write("untracked content\n", quoted, at: path)
        try Data([0, 1, 255, 0]).write(to: URL(fileURLWithPath: path + "/binary.bin"))
        _ = try git(["mv", "old name.txt", "new name.txt"], at: path)
        try FileManager.default.removeItem(atPath: path + "/deleted.txt")
        let local = await service.fetchLocalReview(at: path)
        try require(local.error == nil, "local patch success")
        let staged = local.staged.first { $0.filename == "same.txt" }
        let unstaged = local.unstaged.first { $0.filename == "same.txt" }
        try require(staged?.hunks.first?.lines.contains { $0.type == .addition && $0.content == "staged" } == true, "actual staged content")
        try require(unstaged?.hunks.first?.lines.contains { $0.type == .addition && $0.content == "unstaged" } == true, "actual unstaged content")
        try require(local.untracked.first { $0.filename == quoted }?.added == 1, "exact quoted UTF8/newline path with patch")
        try require(local.untracked.first { $0.filename == "binary.bin" }?.isBinary == true, "binary evidence")
        try require(local.staged.first { $0.filename == "new name.txt" }?.oldFilename == "old name.txt", "rename evidence")
        try require(local.unstaged.first { $0.filename == "deleted.txt" }?.statusCode == "D", "deleted path patch")
        try require(local.uniquePathCount == 5, "staged+unstaged path counted once")
        let lazyLocal = await service.fetchLocalReview(at: path, summariesOnly: true)
        try require(lazyLocal.error == nil && lazyLocal.staged.first { $0.filename == "new name.txt" }?.oldFilename == "old name.txt", "lazy rename metadata")
        let lazyStaged = try await service.fetchReviewPage(file: lazyLocal.staged.first { $0.filename == "same.txt" }!, at: path)
        let lazyUnstaged = try await service.fetchReviewPage(file: lazyLocal.unstaged.first { $0.filename == "same.txt" }!, at: path)
        try require(lazyStaged.hunks.first?.lines.contains { $0.content == "staged" && $0.type == .addition } == true, "lazy staged comparison remains distinct")
        try require(lazyUnstaged.hunks.first?.lines.contains { $0.content == "unstaged" && $0.type == .addition } == true, "lazy unstaged comparison remains distinct")
        let quotedPage = try await service.fetchReviewPage(file: lazyLocal.untracked.first { $0.filename == quoted }!, at: path)
        try require(quotedPage.hunks.first?.lines.last?.content == "untracked content", "lazy quoted/newline path")
        let summary = await service.reviewSummary(at: path, baseBranch: "main")
        try require(summary.error == nil && summary.commitCount == 2 && summary.localFileCount == 5, "compact summary")
        let invalid = await service.fetchBranchReview(at: path, baseBranch: "missing-base")
        try require(invalid.error != nil && invalid.comparisonBase == "missing-base", "recorded invalid base never falls back")
        _ = try git(["reset", "--hard"], at: path)
        _ = try git(["checkout", "--orphan", "unrelated"], at: path)
        _ = try git(["rm", "-rf", "."], at: path)
        try write("orphan\n", "orphan.txt", at: path); try commit("unrelated root", at: path)
        let unrelated = await service.fetchBranchReview(at: path, baseBranch: "main")
        try require(unrelated.error != nil && unrelated.files.isEmpty, "unrelated base error")
        let unborn = try repo(); defer { try? FileManager.default.removeItem(atPath: unborn) }
        try write("staged root\n", "root.txt", at: unborn); _ = try git(["add", "root.txt"], at: unborn)
        try write("local root\n", "root.txt", at: unborn); try write("new\n", "new.txt", at: unborn)
        let unbornLocal = await service.fetchLocalReview(at: unborn)
        let unbornBranch = await service.fetchBranchReview(at: unborn, baseBranch: "main")
        try require(unbornLocal.error == nil && unbornLocal.staged.count == 1 && unbornLocal.unstaged.count == 1 && unbornLocal.untracked.count == 1, "unborn local patches")
        try require(unbornBranch.error != nil, "unborn branch error disclosed")
        let linked = path + "-linked"; defer { try? FileManager.default.removeItem(atPath: linked) }
        _ = try git(["worktree", "add", "-b", "linked", linked, "main"], at: path)
        let metadata = WorktreeWatcher.metadataPaths(worktreePath: linked)
        try require(metadata.contains { $0.contains("/worktrees/") && $0.hasSuffix("/HEAD") }, "resolved linked gitdir")
        try require(metadata.contains(path + "/.git/refs/heads"), "resolved common refs")
        // Track the difficult paths as well, then verify branch/commit metadata is exact.
        _ = try git(["checkout", "feature"], at: path)
        try write("quoted committed\n", quoted, at: path)
        try Data([0, 255, 0, 1]).write(to: URL(fileURLWithPath: path + "/tracked-binary.bin"))
        _ = try git(["mv", "old name.txt", "committed rename.txt"], at: path)
        try commit("quoted binary rename", at: path)
        let difficult = await service.fetchBranchReview(at: path, baseBranch: "main")
        try require(difficult.files.first { $0.filename == quoted }?.added == 1, "quoted committed branch patch")
        try require(difficult.files.first { $0.filename == "tracked-binary.bin" }?.isBinary == true, "binary committed branch patch")
        try require(difficult.files.first { $0.filename == "committed rename.txt" }?.oldFilename == "old name.txt", "cumulative committed rename")
        // Prefer the configured upstream only when there is no recorded base metadata.
        _ = try git(["branch", "--set-upstream-to=main"], at: path)
        let fallback = await service.fetchBranchReview(at: path, baseBranch: nil)
        try require(fallback.comparisonBase == "main" && fallback.error == nil, "upstream fallback")
        let conflict = try repo(); defer { try? FileManager.default.removeItem(atPath: conflict) }
        try write("base\n", "conflict.txt", at: conflict); try write("unchanged\n", "ordinary.txt", at: conflict)
        try commit("base", at: conflict); _ = try git(["checkout", "-b", "side"], at: conflict)
        try write("side\n", "conflict.txt", at: conflict); try commit("side", at: conflict)
        _ = try git(["checkout", "main"], at: conflict)
        try write("ours\n", "conflict.txt", at: conflict); try commit("ours", at: conflict)
        // The merge must fail and leave real index stages 1/2/3.
        do { _ = try git(["merge", "side"], at: conflict); throw Failure(message: "merge unexpectedly succeeded") }
        catch let failure as Failure { try require(failure.message.contains("CONFLICT"), "real unresolved merge") }
        try write("staged ordinary\n", "ordinary.txt", at: conflict); _ = try git(["add", "ordinary.txt"], at: conflict)
        try write("unstaged ordinary\n", "ordinary.txt", at: conflict); try write("new file\n", "untracked.txt", at: conflict)
        let conflicted = await service.fetchLocalReview(at: conflict)
        try require(conflicted.error == nil && conflicted.staged.count == 2 && conflicted.unstaged.count == 2 && conflicted.untracked.count == 1, "unresolved merge retains all local groups")
        try require(conflicted.staged.first { $0.filename == "conflict.txt" }?.statusCode == "U", "explicit staged conflict")
        try require(conflicted.unstaged.first { $0.filename == "conflict.txt" }?.hunks.first?.lines.contains { $0.content == "<<<<<<< HEAD" } == true, "working conflict patch against ours")
        try require(conflicted.uniquePathCount == 3, "conflict path counted once")
        let lazyConflict = await service.fetchLocalReview(at: conflict, summariesOnly: true)
        try require(lazyConflict.error == nil && lazyConflict.staged.count == 2 && lazyConflict.unstaged.count == 2, "lazy conflicts retain unique paths in both groups")
        let conflictPage = try await service.fetchReviewPage(file: lazyConflict.unstaged.first { $0.filename == "conflict.txt" }!, at: conflict)
        try require(conflictPage.hunks.first?.lines.contains { $0.content == "<<<<<<< HEAD" } == true, "lazy conflict baseline retains actual working content")
        _ = try git(["reset", "--hard", "HEAD"], at: conflict)
        _ = try git(["checkout", "-b", "ours-deleted"], at: conflict)
        _ = try git(["rm", "conflict.txt"], at: conflict); try commit("ours deletes", at: conflict)
        do { _ = try git(["merge", "side"], at: conflict); throw Failure(message: "merge unexpectedly succeeded") }
        catch let failure as Failure { try require(failure.message.contains("CONFLICT"), "real ours-deleted merge") }
        let noOurs = await service.fetchLocalReview(at: conflict)
        let surviving = noOurs.unstaged.first { $0.filename == "conflict.txt" }
        try require(noOurs.error == nil && surviving?.statusCode == "U" && surviving?.added == 1, "ours-deleted surviving file patch")
        try require(surviving?.conflictBaseline == "empty (ours deleted)" && surviving?.hunks.first?.lines.last?.content == "side", "explicit empty baseline and actual surviving content")
        try Data([0, 1, 255]).write(to: URL(fileURLWithPath: conflict + "/conflict.txt"))
        let noOursBinary = await service.fetchLocalReview(at: conflict)
        try require(noOursBinary.unstaged.first { $0.filename == "conflict.txt" }?.isBinary == true, "ours-deleted binary evidence")
        _ = try git(["reset", "--hard", "HEAD"], at: conflict)
        _ = try git(["checkout", "side"], at: conflict)
        do { _ = try git(["merge", "ours-deleted"], at: conflict); throw Failure(message: "merge unexpectedly succeeded") }
        catch let failure as Failure { try require(failure.message.contains("CONFLICT"), "real theirs-deleted merge") }
        try FileManager.default.removeItem(atPath: conflict + "/conflict.txt")
        let oursOnly = await service.fetchLocalReview(at: conflict)
        let removed = oursOnly.unstaged.first { $0.filename == "conflict.txt" }
        try require(oursOnly.error == nil && removed?.removed == 1 && removed?.conflictBaseline == "ours (index stage 2)", "theirs-deleted working deletion uses existing ours baseline")
        // Merge reconciliation preserves workspace HEAD-to-working-file APIs and capture caps.
        let workspace = try repo(); defer { try? FileManager.default.removeItem(atPath: workspace) }
        try write("root\n", "file.txt", at: workspace); try commit("root", at: workspace)
        try write("staged\n", "file.txt", at: workspace); _ = try git(["add", "file.txt"], at: workspace)
        try write("working\n", "file.txt", at: workspace)
        try write("new\n", "quoted \"new\".txt", at: workspace)
        let workspaceFiles = try await service.fetchWorkingTreeChanges(worktreePath: workspace)
        try require(workspaceFiles.count == 2, "workspace staged/unstaged/untracked overview preserved")
        let tracked = workspaceFiles.first { $0.filename == "file.txt" }!
        let workspacePatch = try await service.fetchWorkingTreeFileDiff(worktreePath: workspace, file: tracked)
        try require(workspacePatch.hunks.first?.lines.contains { $0.content == "working" && $0.type == .addition } == true, "workspace diff keeps final working contents")
        let limited = await service.runGit(["diff", "HEAD", "--", "file.txt"], cwd: workspace, outputLimit: 8)
        try require(limited.outputExceededLimit && limited.stdout.isEmpty, "workspace output capture cap preserved")
        if ProcessInfo.processInfo.environment["GIT_REVIEW_SLOW_LOCAL_CHECK"] == "1" {
            let slow = await service.runGit(["-c", "alias.slow=!sleep 26; printf complete", "slow"], cwd: workspace)
            try require(slow.success && slow.stdout == "complete", "local Git must outlive remote-only 25-second deadline")
            print("Slow local Git deadline regression passed")
        }
        let previews = try repo(); defer { try? FileManager.default.removeItem(atPath: previews) }
        for index in 0..<40 { try write("file \(index)\n", String(format: "new%02d.txt", index), at: previews) }
        try write(String(repeating: "x", count: 1_100_000), "aa-large.txt", at: previews)
        try FileManager.default.createDirectory(atPath: previews + "/aa-nested", withIntermediateDirectories: true)
        _ = try git(["init"], at: previews + "/aa-nested")
        try write("nested\n", "file.txt", at: previews + "/aa-nested")
        let previewReview = await service.fetchLocalReview(at: previews)
        try require(previewReview.untrackedError == nil && previewReview.untracked.count == 42, "one bad untracked path must not discard group")
        try require(previewReview.untracked.first { $0.filename == "aa-large.txt" }?.previewError != nil, "large untracked capture bounded")
        try require(previewReview.untracked.first { $0.filename.hasPrefix("aa-nested") }?.previewError != nil, "nested repository placeholder")
        let deferred = previewReview.untracked.first { $0.previewDeferred }!
        let explicitPreview = await service.untrackedPreview(name: deferred.filename, at: previews)
        try require(explicitPreview.hunks.first?.lines.count == 1, "deferred preview explicitly loadable")
        let reloaded = await service.fetchLocalReview(at: previews)
        try require(reloaded.untracked.first { $0.filename == deferred.filename }?.previewDeferred == false, "explicit preview survives polling")
        try write("changed\n", deferred.filename, at: previews)
        let changed = await service.untrackedPreview(name: deferred.filename, at: previews)
        try require(changed.hunks.first?.lines.first?.content == "changed", "metadata cache invalidated after edit")
        print("Git repository checks passed")
    }

    @MainActor static func stateChecks() async throws {
        let path = try repo(); defer { try? FileManager.default.removeItem(atPath: path) }
        try write("root\n", "root.txt", at: path); try commit("root", at: path)
        _ = try git(["checkout", "-b", "feature"], at: path)
        try write("first\n", "first.txt", at: path); try commit("first", at: path)
        try write("second\n", "second.txt", at: path); try commit("second", at: path)
        let fake = path + "/fake-gh"
        // A controllable executable exercises remote refresh without auth/network dependencies.
        try write("""
        #!/bin/sh
        if [ -f fail ]; then echo remote-unavailable >&2; exit 1; fi
        if [ "$2" = list ]; then cat prs.json; else cat pr.patch; fi
        """, "fake-gh", at: path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: fake)
        try write("""
        [{"number":1,"title":"One","url":"https://example.test/1","state":"OPEN","headRefName":"feature","baseRefName":"main","author":{"login":"test"},"labels":[],"additions":1,"deletions":0,"changedFiles":1,"isDraft":false,"createdAt":"today","updatedAt":"today"}]
        """, "prs.json", at: path)
        let patch = "diff --git a/remote.txt b/remote.txt\nnew file mode 100644\n--- /dev/null\n+++ b/remote.txt\n@@ -0,0 +1 @@\n+remote first\n"
        try write(patch, "pr.patch", at: path)
        let state = DiffState(gitService: GitService(ghPath: fake), observeChanges: false, summariesOnly: false)
        let worktree = WorktreeModel(id: "test", name: "test", path: path, branch: "feature", status: "active", agents: [], baseBranch: "main")
        state.loadForWorktree(worktree)
        try await settle(state)
        try require(state.activeTab == .branch && state.commits.count == 2 && state.comparisonBase == "main", "default branch and recorded base")
        try require(state.prDiff?.files.first?.added == 1 && state.prError == nil, "remote initial patch")
        let lazyState = DiffState(gitService: GitService(ghPath: fake), observeChanges: false)
        lazyState.loadForWorktree(worktree); try await settle(lazyState)
        try require(lazyState.selectedPR == nil && lazyState.prDiff == nil && !lazyState.pullRequests.isEmpty, "opening a worktree does not download a background PR patch")
        lazyState.activeTab = .prDiffs; try await settle(lazyState)
        try require(lazyState.prDiff?.files.first?.hunks.isEmpty == true && lazyState.prDiff?.files.first?.added == 1, "PR tab loads metadata with deferred bodies")
        let lazyPage = try await lazyState.loadPage(lazyState.prDiff!.files[0])
        try require(lazyPage.hunks.first?.lines.last?.content == "remote first", "PR body loads through overview page routing")
        lazyState.activeTab = .commits
        let blob = try git(["rev-parse", "HEAD:second.txt"], at: path)
        let object = path + "/.git/objects/" + blob.prefix(2) + "/" + blob.dropFirst(2)
        let held = path + "/held-object"
        try FileManager.default.moveItem(atPath: object, toPath: held)
        lazyState.selectCommit(lazyState.commits[0]); try await settle(lazyState)
        try require(lazyState.commitError != nil && lazyState.commitDiff.isEmpty, "failed selected commit retains an actionable error")
        try FileManager.default.moveItem(atPath: held, toPath: object)
        lazyState.refresh(); try await settle(lazyState)
        try require(lazyState.commitError == nil, "local refresh retries a selected commit that has not loaded successfully")
        try require(lazyState.commitDiff.first?.filename == "second.txt" && lazyState.commitDiff.first?.hunks.isEmpty == true, "selected commit loads metadata only")
        lazyState.stopWatching()
        state.selectCommit(state.commits[1]); state.selectCommit(state.commits[0])
        try await settle(state)
        try require(state.commitDiff.first?.filename == "second.txt", "latest commit selection wins")
        try write(patch.replacingOccurrences(of: "remote first", with: "remote refreshed"), "pr.patch", at: path)
        state.refresh(); try await settle(state)
        try require(state.prDiff?.files.first?.hunks.first?.lines.last?.content == "remote refreshed", "explicit refresh updates selected PR")
        try write("fail", "fail", at: path); state.refresh(); try await settle(state)
        try require(state.prError?.contains("remote-unavailable") == true && state.prDiff != nil, "PR error retains evidence")
        try FileManager.default.removeItem(atPath: path + "/fail")
        // PR patches have Git C-quoted paths, including binary files with no +++ header.
        try write("quoted\n", "quoted \"name\"\tü.txt", at: path)
        try Data([0, 1, 0]).write(to: URL(fileURLWithPath: path + "/binary \"name\".bin"))
        _ = try git(["add", "quoted \"name\"\tü.txt", "binary \"name\".bin"], at: path)
        try write(try git(["diff", "--cached"], at: path) + "\n", "pr.patch", at: path)
        state.refresh(); try await settle(state)
        try require(state.prDiff?.files.contains { $0.filename == "quoted \"name\"\tü.txt" && $0.added == 1 } == true, "quoted PR filename")
        try require(state.prDiff?.files.contains { $0.filename == "binary \"name\".bin" && $0.isBinary } == true, "quoted binary PR filename")
        let lazyRemote = GitService(ghPath: fake)
        let remoteSummary = try await lazyRemote.fetchPRDiffChecked(cwd: path, prNumber: 1, summariesOnly: true)
        try require(remoteSummary.files.count == 2 && remoteSummary.files.allSatisfy { $0.hunks.isEmpty }, "PR metadata excludes all patch bodies")
        let remoteFile = remoteSummary.files.first { $0.filename == "quoted \"name\"\tü.txt" }!
        let remotePage = try await lazyRemote.fetchReviewPage(file: remoteFile, at: path)
        try require(remotePage.hunks.first?.lines.last?.content == "quoted", "disk-backed quoted PR path loads exact patch")
        try require(remoteSummary.files.first { $0.filename == "binary \"name\".bin" }?.isBinary == true, "disk-backed PR binary metadata")
        try write("unexpected output", "pr.patch", at: path)
        state.refresh(); try await settle(state)
        try require(state.prError != nil && state.prDiff?.files.count == 2, "malformed PR patch retains evidence")
        state.selectedPR = nil; state.prDiff = nil
        state.refresh(); try await settle(state)
        try require(state.selectedPR == nil && state.prDiff == nil && !state.pullRequests.isEmpty, "refresh must preserve explicit PR list navigation")
        // Base changes invalidate pending PR diffs even when the subsequent lookup fails.
        try write("fail", "fail", at: path)
        state.selectPR(state.pullRequests[0])
        state.setComparisonBase("missing-base"); try await settle(state)
        try require(state.branchError != nil && state.comparisonBase == "missing-base" && state.branchDiff.isEmpty, "explicit base failure")
        try FileManager.default.removeItem(atPath: path + "/fail")
        state.setComparisonBase("main"); try await settle(state)
        try require(state.branchError == nil && state.branchDiff.count == 2, "base error recovers")
        let other = try repo(); defer { try? FileManager.default.removeItem(atPath: other) }
        try write("other\n", "other.txt", at: other); try commit("other", at: other)
        state.loadForWorktree(worktree)
        state.loadForProject(root: other)
        try await settle(state)
        try require(state.commits.isEmpty && state.branchDiff.isEmpty && state.comparisonBase == "main", "stale workspace responses rejected")
        state.loadForWorktree(worktree); state.stopWatching()
        try await Task.sleep(for: .milliseconds(200))
        try require(!state.isLoadingBranch && !state.isLoadingPRs && state.commits.isEmpty, "stop invalidates in-flight loads")
        print("Git state and remote checks passed")
    }
    @MainActor static func emptyRefreshChecks() async throws {
        let path = try repo(); defer { try? FileManager.default.removeItem(atPath: path) }
        try write("root\n", "root.txt", at: path); try commit("root", at: path)
        let state = DiffState(gitService: GitService(ghPath: "/usr/bin/false"), observeChanges: false, summariesOnly: false)
        defer { state.stopWatching() }
        state.loadForProject(root: path)
        try require(state.isInitiallyLoadingBranch && state.isInitiallyLoadingUnstaged, "new project shows initial loading")
        try await settle(state)
        for _ in 0..<3 {
            state.refresh()
            try require(state.isLoadingBranch && state.isLoadingUnstaged, "background refresh still fetches Git evidence")
            try require(!state.isInitiallyLoadingBranch && !state.isInitiallyLoadingUnstaged, "empty branch, commits and clean working tree stay visible during refresh")
            try await settle(state)
            try require(state.branchDiff.isEmpty && state.commits.isEmpty && state.localFileCount == 0, "clean project remains empty")
        }
        try write("edited\n", "root.txt", at: path)
        state.refresh(); try await settle(state)
        try require(state.unstagedDiff.count == 1, "quiet refresh still publishes file edits")
        _ = try git(["checkout", "-b", "feature"], at: path)
        _ = try git(["commit", "--allow-empty", "-m", "empty"], at: path)
        let worktree = WorktreeModel(id: "empty", name: "empty", path: path, branch: "feature", status: "active", agents: [], baseBranch: "main")
        state.loadForWorktree(worktree)
        try require(state.isInitiallyLoadingBranch && state.isInitiallyLoadingUnstaged, "worktree selection resets initial loading")
        try await settle(state)
        state.selectCommit(state.commits[0])
        try require(state.isInitiallyLoadingCommit, "first commit selection loads its patch")
        try await settle(state)
        state.refresh(); try await settle(state)
        state.selectCommit(state.commits[0])
        try require(state.isLoadingCommit && !state.isInitiallyLoadingCommit && state.commitDiff.isEmpty, "empty selected commit remains visible during reload")
        try await settle(state)
        state.setComparisonBase("feature")
        try require(state.isInitiallyLoadingBranch && !state.isInitiallyLoadingUnstaged, "new base reloads branch evidence without hiding local evidence")
        try await settle(state)
        try require(state.commits.isEmpty, "new comparison replaces commit list")
        let unavailable = GitCommitInfo(sha: String(repeating: "0", count: 40), subject: "Unavailable", author: "Test", date: "today")
        for _ in 0..<2 {
            // Refresh clears branchError before retrying a selected commit.
            state.commitError = nil
            state.selectCommit(unavailable)
            try require(state.isInitiallyLoadingCommit && state.commitDiff.isEmpty, "failed commit fetch retries show loading rather than false empty evidence")
            try await settle(state)
            try require(state.commitError != nil && state.commitDiff.isEmpty, "unavailable commit exposes fetch failure")
        }
        print("Empty review refresh checks passed")
    }

    @MainActor static func settle(_ state: DiffState) async throws {
        for _ in 0..<200 {
            if !state.isLoadingBranch && !state.isLoadingUnstaged && !state.isLoadingPRs && !state.isLoadingPRDiff && !state.isLoadingCommit { return }
            try await Task.sleep(for: .milliseconds(25))
        }
        throw Failure(message: "state refresh timed out")
    }

    @MainActor static func lazyReviewChecks() async throws {
        let path = try repo(); defer { try? FileManager.default.removeItem(atPath: path) }
        try write("original\n", "large.txt", at: path); try commit("root", at: path)
        _ = try git(["checkout", "-b", "feature"], at: path)
        try write((0..<1000).map { "line \($0)" }.joined(separator: "\n") + "\n", "large.txt", at: path)
        try commit("large change", at: path)
        let service = GitService()
        let review = await service.fetchBranchReview(at: path, baseBranch: "main", summariesOnly: true)
        try require(review.error == nil && review.files.count == 1, "large changes remain navigable")
        let file = review.files[0]
        try require(file.hunks.isEmpty && file.added == 1000 && file.removed == 1, "overview loads counts without patch bodies")
        try require(file.shouldCollapse, "large files start collapsed")
        var page = try await service.fetchReviewPage(file: file, at: path)
        try require(page.hunks.flatMap(\.lines).count == 200, "expanded files have bounded text layout")
        var lines = page.hunks.flatMap(\.lines)
        while page.hasMore {
            page = try await service.fetchReviewPage(file: file, at: path, after: page)
            lines += page.hunks.flatMap(\.lines)
        }
        try require(lines.count == 1001 && lines.last?.content == "line 999" && lines.last?.newLineNo == 1000, "all lower lines remain reachable without gaps")
        try write("local\n", "large.txt", at: path)
        try write("new\n", "new.txt", at: path)
        let local = await service.fetchLocalReview(at: path, summariesOnly: true)
        try require(local.unstaged.first?.hunks.isEmpty == true && local.untracked.first?.hunks.isEmpty == true, "local overview defers all bodies")
        let newPage = try await service.fetchReviewPage(file: local.untracked[0], at: path)
        try require(newPage.hunks.first?.lines.first?.content == "new", "untracked content remains reachable")
        let selectedSHA = try git(["rev-parse", "HEAD"], at: path)
        let commitFiles = try await service.fetchCommitDiff(at: path, sha: selectedSHA, summariesOnly: true)
        try require(commitFiles[0].hunks.isEmpty && commitFiles[0].added == 1000, "commit summaries defer bodies")
        let initialSHA = try git(["rev-parse", "main"], at: path)
        let initialFiles = try await service.fetchCommitDiff(at: path, sha: initialSHA, summariesOnly: true)
        let initialPage = try await service.fetchReviewPage(file: initialFiles[0], at: path)
        try require(initialPage.hunks.first?.lines.first?.content == "original", "initial commit uses empty baseline")
        try write(String(repeating: "x", count: 1_100_000) + "\n", "minified.txt", at: path)
        let longSummary = await service.fetchLocalReview(at: path, summariesOnly: true)
        let longPage = try await service.fetchReviewPage(file: longSummary.untracked.first { $0.filename == "minified.txt" }!, at: path)
        try require(longPage.hasLongLines && longPage.hunks[0].lines[0].content.utf8.count < 2200, "minified lines cannot trigger unbounded layout")
        let raw = try String(contentsOf: longPage.slice.snapshot.url, encoding: .utf8)
        try require(raw.contains(String(repeating: "x", count: 1_100_000)), "full patch retains shortened line content")
        for index in 0..<1000 { try write("new file\n", String(format: "file%04d.txt", index), at: path) }
        try commit("many files", at: path)
        let started = Date()
        let many = await service.fetchBranchReview(at: path, baseBranch: "main", summariesOnly: true)
        try require(many.error == nil && many.files.count == 1003 && many.files.allSatisfy { $0.hunks.isEmpty }, "complete navigation survives thousands of changed files")
        let last = many.files.first { $0.filename == "file0999.txt" }!
        let lastPage = try await service.fetchReviewPage(file: last, at: path)
        try require(lastPage.hunks.first?.lines.last?.content == "new file", "direct access to lower files bypasses other patch bodies")
        print("1,003-file metadata and direct-jump check: \(Int(Date().timeIntervalSince(started) * 1000)) ms")
        let state = DiffState(gitService: GitService(ghPath: "/usr/bin/false"), observeChanges: false)
        state.loadForProject(root: path)
        try await settle(state)
        try require(state.branchDiff.allSatisfy { $0.hunks.isEmpty }, "overview defaults to metadata-only loading")
        state.stopWatching()
        print("Lazy review and page checks passed")
    }

    @MainActor static func reviewRegressionChecks() async throws {
        let path = try repo(); defer { try? FileManager.default.removeItem(atPath: path) }
        let service = GitService()
        try write("first\n", "baseline.txt", at: path); try commit("first", at: path)
        let first = try git(["rev-parse", "HEAD"], at: path)
        try write("second\n", "baseline.txt", at: path); try commit("second", at: path)
        try write("third\n", "baseline.txt", at: path); _ = try git(["add", "baseline.txt"], at: path)
        let before = await service.fetchLocalReview(at: path, summariesOnly: true)
        let oldPage = try await service.fetchReviewPage(file: before.staged[0], at: path)
        _ = try git(["reset", "--soft", first], at: path)
        let after = await service.fetchLocalReview(at: path, summariesOnly: true)
        try require(before.staged[0].previewVersion != after.staged[0].previewVersion, "soft reset invalidates staged preview independently of index/work file")
        let newPage = try await service.fetchReviewPage(file: after.staged[0], at: path)
        try require(oldPage.hunks[0].lines.contains { $0.content == "second" && $0.type == .deletion }, "old staged snapshot baseline")
        try require(newPage.hunks[0].lines.contains { $0.content == "first" && $0.type == .deletion }, "fresh staged snapshot follows moved HEAD")

        try write((1...20_000).map { "history line \($0)" }.joined(separator: "\n") + "\n", "many.txt", at: path)
        let summary = await service.fetchLocalReview(at: path, summariesOnly: true)
        let file = summary.untracked.first { $0.filename == "many.txt" }!
        var history = ReviewPageHistory(), index = 0
        while true {
            let page = try await service.fetchReviewPage(file: file, at: path, cursor: history.cursor(at: index))
            history.store(page, at: index)
            try require(history.retainedLineCount <= 200, "visited pages retain only one bounded body")
            if !page.hasMore { break }
            index += 1
        }
        try require(index >= 99 && history.page(at: index)?.hunks.last?.lines.last?.newLineNo == 20_000, "all pages remain reachable with bounded retention")
        history.releaseBody()
        try require(history.retainedLineCount == 0, "collapsing/offscreen cards release page bodies")
        try FileManager.default.removeItem(atPath: path + "/many.txt")
        let revisited = try await service.fetchReviewPage(file: file, at: path, cursor: history.cursor(at: 0))
        try require(revisited.hunks[0].lines.first?.content == "history line 1", "previous pages reread the existing snapshot without regenerating Git")

        let fake = path + "/fake-gh"
        try write("""
        #!/bin/sh
        if [ "$2" = list ]; then cat prs.json
        else
          if [ -f fail-diff ]; then echo temporary-diff-error >&2; exit 1; fi
          if [ -f delay ]; then touch started; sleep 1; fi
          cat pr.patch
        fi
        """, "fake-gh", at: path)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: fake)
        let list = """
        [{"number":1,"title":"One","url":"https://example.test/1","state":"OPEN","headRefName":"main","baseRefName":"main","author":{"login":"test"},"labels":[],"additions":1,"deletions":0,"changedFiles":1,"isDraft":false,"createdAt":"today","updatedAt":"v1"}]
        """
        let patch = "diff --git a/remote.txt b/remote.txt\n--- a/remote.txt\n+++ b/remote.txt\n@@ -0,0 +1 @@\n+first remote\n"
        try write(list, "prs.json", at: path); try write(patch, "pr.patch", at: path)
        let state = DiffState(gitService: GitService(ghPath: fake), observeChanges: false)
        state.loadForProject(root: path); try await settle(state)
        state.activeTab = .prDiffs; try await settle(state)
        try require(state.prDiff != nil && !state.isPRDiffStale, "initial PR snapshot is current")
        try write(list.replacingOccurrences(of: "v1", with: "v2"), "prs.json", at: path)
        try write(patch.replacingOccurrences(of: "first remote", with: "updated remote"), "pr.patch", at: path)
        try write("delay", "delay", at: path)
        state.refresh()
        for _ in 0..<120 {
            if FileManager.default.fileExists(atPath: path + "/started") { break }
            try await Task.sleep(for: .milliseconds(25))
        }
        try require(state.isLoadingPRDiff && state.selectedPR?.updatedAt == "v2", "updated PR download is in flight")
        state.activeTab = .branch
        try require(state.prDiff != nil && state.isPRDiffStale, "interrupted PR refresh retains visibly stale evidence")
        try FileManager.default.removeItem(atPath: path + "/delay")
        state.activeTab = .prDiffs
        try require(state.isLoadingPRDiff, "returning to stale PR resumes its interrupted refresh")
        try await settle(state)
        try require(!state.isPRDiffStale, "successful replacement marks the loaded PR revision current")
        let refreshed = try await state.loadPage(state.prDiff!.files[0])
        try require(refreshed.hunks[0].lines.last?.content == "updated remote", "reentry replaces stale PR patch content")
        state.selectedPR = nil; state.prDiff = nil
        try write("fail", "fail-diff", at: path)
        state.selectPR(state.pullRequests[0]); try await settle(state)
        try require(state.prError != nil && state.prDiff == nil, "failed selected PR has an actionable load error")
        try FileManager.default.removeItem(atPath: path + "/fail-diff")
        state.refreshPRs(); try await settle(state)
        try require(state.prError == nil && state.prDiff != nil, "ordinary PR polling retries an unloaded selected PR even when updatedAt is unchanged")
        state.stopWatching()

        let rename = try repo(); defer { try? FileManager.default.removeItem(atPath: rename) }
        try write("original\n", "old.txt", at: rename); try commit("root", at: rename)
        _ = try git(["checkout", "-b", "renamed"], at: rename)
        _ = try git(["mv", "old.txt", "new.txt"], at: rename)
        try FileManager.default.createDirectory(atPath: rename + "/old.txt", withIntermediateDirectories: true)
        try write("replacement\n", "old.txt/child.txt", at: rename)
        _ = try git(["add", "--all"], at: rename)
        let staged = await service.fetchLocalReview(at: rename, summariesOnly: true)
        let stagedRename = staged.staged.first { $0.filename == "new.txt" }!
        let stagedPage = try await service.fetchReviewPage(file: stagedRename, at: rename)
        try require(stagedRename.oldFilename == "old.txt" && stagedPage.hunks.isEmpty, "staged rename cannot render a child file's patch")
        try commit("rename with directory replacement", at: rename)
        let branch = await service.fetchBranchReview(at: rename, baseBranch: "main", summariesOnly: true)
        let branchRename = branch.files.first { $0.filename == "new.txt" }!
        let branchPage = try await service.fetchReviewPage(file: branchRename, at: rename)
        try require(branchPage.hunks.isEmpty, "branch rename remains scoped to its exact entry")
        let sha = try git(["rev-parse", "HEAD"], at: rename)
        let commit = try await service.fetchCommitDiff(at: rename, sha: sha, summariesOnly: true)
        let commitPage = try await service.fetchReviewPage(file: commit.first { $0.filename == "new.txt" }!, at: rename)
        try require(commitPage.hunks.isEmpty, "commit rename remains scoped to its exact entry")
        print("Page retention, soft reset, rename scoping, and interrupted/failed PR regression checks passed")
    }

    static func concurrentProcessChecks() async throws {
        try await withThrowingTaskGroup(of: (Bool, Int, Int).self) { group in
            for _ in 0..<32 {
                group.addTask {
                    let service = GitService()
                    // Both pipes exceed their kernel buffers and must drain while
                    // many separate service actors occupy cooperative workers.
                    let result = await service.runGit([
                        "-c", "alias.pipe-check=!i=0; while [ $i -lt 10000 ]; do printf 'stdout line\\n'; printf 'stderr line\\n' >&2; i=$((i+1)); done", "pipe-check",
                    ], cwd: NSTemporaryDirectory())
                    return (result.success, result.stdout.components(separatedBy: "\n").count,
                            result.stderr.components(separatedBy: "\n").count)
                }
            }
            for try await (success, stdoutLines, stderrLines) in group {
                try require(success, "concurrent Git process completed")
                try require(stdoutLines == 10001, "all stdout drained")
                try require(stderrLines == 10001, "all stderr drained")
            }
        }
        let cancelService = GitService()
        let canceled = Task {
            await cancelService.runGit(["-c", "alias.cancel-check=!exec /bin/sleep 2", "cancel-check"], cwd: NSTemporaryDirectory())
        }
        try await Task.sleep(for: .milliseconds(100))
        canceled.cancel()
        let canceledResult = await canceled.value
        try require(!canceledResult.success, "canceled Git work terminates instead of completing obsolete reads")
        print("Concurrent Git pipe and cancellation checks passed")
    }
}

#if GIT_REVIEW_HARNESS
@main struct GitReviewHarness {
    @MainActor static func main() async throws {
        try await GitReviewChecks.repositoryChecks()
        try await GitReviewChecks.stateChecks()
        try await GitReviewChecks.emptyRefreshChecks()
        try await GitReviewChecks.lazyReviewChecks()
        try await GitReviewChecks.reviewRegressionChecks()
        try await GitReviewChecks.concurrentProcessChecks()
    }
}
#else
struct GitReviewTests {
    @Test func realRepositories() async throws { try await GitReviewChecks.repositoryChecks() }
    @Test @MainActor func selectionsAndRemoteRefresh() async throws { try await GitReviewChecks.stateChecks() }
    @Test @MainActor func emptyResultsStayVisibleDuringRefresh() async throws { try await GitReviewChecks.emptyRefreshChecks() }
    @Test @MainActor func boundedReviewPages() async throws { try await GitReviewChecks.lazyReviewChecks() }
    @Test @MainActor func reviewRegressions() async throws { try await GitReviewChecks.reviewRegressionChecks() }
    @Test func concurrentGitPipesComplete() async throws { try await GitReviewChecks.concurrentProcessChecks() }
}
#endif
