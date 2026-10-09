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
        state.setComparisonBase("missing-base"); try await settle(state)
        try require(state.branchError != nil && state.comparisonBase == "missing-base" && state.branchDiff.isEmpty, "explicit base failure")
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
