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
        let state = DiffState(gitService: GitService(ghPath: fake))
        let worktree = WorktreeModel(id: "test", name: "test", path: path, branch: "feature", status: "active", agents: [], baseBranch: "main")
        state.loadForWorktree(worktree)
        try await settle(state)
        try require(state.activeTab == .branch && state.commits.count == 2 && state.comparisonBase == "main", "default branch and recorded base")
        try require(state.prDiff?.files.first?.added == 1 && state.prError == nil, "remote initial patch")
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
    @MainActor static func settle(_ state: DiffState) async throws {
        for _ in 0..<200 {
            if !state.isLoadingBranch && !state.isLoadingUnstaged && !state.isLoadingPRs && !state.isLoadingPRDiff && !state.isLoadingCommit { return }
            try await Task.sleep(for: .milliseconds(25))
        }
        throw Failure(message: "state refresh timed out")
    }
}

#if GIT_REVIEW_HARNESS
@main struct GitReviewHarness {
    @MainActor static func main() async throws {
        try await GitReviewChecks.repositoryChecks()
        try await GitReviewChecks.stateChecks()
    }
}
#else
struct GitReviewTests {
    @Test func realRepositories() async throws { try await GitReviewChecks.repositoryChecks() }
    @Test @MainActor func selectionsAndRemoteRefresh() async throws { try await GitReviewChecks.stateChecks() }
}
#endif
