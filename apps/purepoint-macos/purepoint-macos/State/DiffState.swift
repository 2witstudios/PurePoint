import Foundation
import Observation
import AppKit

nonisolated enum DiffTab: String, CaseIterable {
    case branch = "Branch Changes"
    case unstaged = "Uncommitted"
    case commits = "Commits"
    case prDiffs = "PR Diffs"
}

@Observable
@MainActor
final class DiffState {
    var activeTab: DiffTab = .branch
    var branchDiff: [FileDiff] = []
    var stagedDiff: [FileDiff] = []
    var unstagedDiff: [FileDiff] = []
    var untrackedDiff: [FileDiff] = []
    var commits: [GitCommitInfo] = []
    var selectedCommit: GitCommitInfo?
    var commitDiff: [FileDiff] = []
    var comparisonBase = ""
    var availableBases: [String] = []
    var branchError: String?
    var localError: String?
    var prError: String?
    var isLoadingBranch = false
    var isLoadingUnstaged = false
    var isLoadingCommit = false
    // Empty results are loaded evidence too; polling must not replace them with a spinner.
    private var hasLoadedBranch = false
    private var hasLoadedLocal = false
    private var hasLoadedCommit = false
    var isInitiallyLoadingBranch: Bool { isLoadingBranch && !hasLoadedBranch }
    var isInitiallyLoadingUnstaged: Bool { isLoadingUnstaged && !hasLoadedLocal }
    var isInitiallyLoadingCommit: Bool { isLoadingCommit && !hasLoadedCommit }
    var pullRequests: [PullRequestInfo] = []
    var selectedPR: PullRequestInfo?
    var prDiff: DiffData?
    var isLoadingPRs = false
    var isLoadingPRDiff = false
    var ghAvailable = true
    var error: String? { branchError ?? localError ?? prError }
    var localFileCount: Int { Set((stagedDiff + unstagedDiff + untrackedDiff).map(\.filename)).count }

    private let git: GitService
    init(gitService: GitService = .shared) { git = gitService }

    private var watcher: WorktreeWatcher?
    private var path: String?
    private var branch: String?
    private var requestedBase: String?
    private var generation = UUID()
    private var localTask: Task<Void, Never>?
    private var prTask: Task<Void, Never>?
    private var commitTask: Task<Void, Never>?
    private var prDiffTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private var initialPRSelectionMade = false
    private var lastPRRefresh = Date.distantPast

    func loadForWorktree(_ worktree: WorktreeModel) {
        load(path: worktree.path, branch: worktree.branch, base: worktree.baseBranch)
    }
    func loadForProject(root: String) { load(path: root, branch: nil, base: nil) }
    func loadForProject(projectRoot: String) { loadForProject(root: projectRoot) }

    private func load(path: String, branch: String?, base: String?) {
        stopWatching()
        self.path = path; self.branch = branch; requestedBase = base
        initialPRSelectionMade = false
        activeTab = .branch; comparisonBase = base ?? ""; availableBases = []
        branchDiff = []; stagedDiff = []; unstagedDiff = []; untrackedDiff = []
        commits = []; selectedCommit = nil; commitDiff = []
        pullRequests = []; selectedPR = nil; prDiff = nil
        branchError = nil; localError = nil; prError = nil
        hasLoadedBranch = false; hasLoadedLocal = false; hasLoadedCommit = false
        refresh()
        watcher = WorktreeWatcher(worktreePath: path) { [weak self] in
            Task { @MainActor in self?.refreshLocal() }
        }
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(2))
                guard !Task.isCancelled, let self else { return }
                // Recursive file changes need polling. Hidden/background app work is bounded.
                guard NSApplication.shared.isActive else { continue }
                self.refreshLocal()
                if Date().timeIntervalSince(self.lastPRRefresh) >= 30 { self.refreshPRs() }
            }
        }
    }

    func setComparisonBase(_ base: String) {
        requestedBase = base; comparisonBase = base
        generation = UUID()
        localTask?.cancel(); localTask = nil; commitTask?.cancel(); prTask?.cancel(); prTask = nil; prDiffTask?.cancel()
        isLoadingPRDiff = false; isLoadingPRs = false
        // Old branch and commit evidence belongs to a different comparison.
        branchDiff = []; commits = []; selectedCommit = nil; commitDiff = []; isLoadingCommit = false
        hasLoadedBranch = false; hasLoadedCommit = false
        refresh()
    }

    func selectCommit(_ commit: GitCommitInfo) {
        guard let path else { return }
        if selectedCommit?.sha != commit.sha { commitDiff = []; hasLoadedCommit = false }
        selectedCommit = commit; commitTask?.cancel(); isLoadingCommit = true
        let token = generation
        commitTask = Task {
            do {
                let files = try await git.fetchCommitDiff(at: path, sha: commit.sha)
                guard !Task.isCancelled, generation == token, selectedCommit?.sha == commit.sha else { return }
                commitDiff = files; hasLoadedCommit = true
            } catch {
                guard !Task.isCancelled, generation == token, selectedCommit?.sha == commit.sha else { return }
                branchError = error.localizedDescription
            }
            isLoadingCommit = false
        }
    }

    func selectPR(_ pr: PullRequestInfo) {
        guard let path else { return }
        let changed = selectedPR?.number != pr.number
        selectedPR = pr; prDiffTask?.cancel(); isLoadingPRDiff = true
        if changed { prDiff = nil }
        let token = generation
        prDiffTask = Task {
            do {
                let diff = try await git.fetchPRDiffChecked(cwd: path, prNumber: pr.number)
                guard !Task.isCancelled, generation == token, selectedPR?.number == pr.number else { return }
                prDiff = diff; prError = nil
            } catch {
                guard !Task.isCancelled, generation == token, selectedPR?.number == pr.number else { return }
                prError = error.localizedDescription
            }
            isLoadingPRDiff = false
        }
    }

    func loadUntrackedPreview(_ file: FileDiff) async {
        guard let path else { return }
        let token = generation
        let preview = await git.untrackedPreview(name: file.filename, at: path)
        guard generation == token, let index = untrackedDiff.firstIndex(where: { $0.filename == file.filename }) else { return }
        untrackedDiff[index] = preview
    }

    func refresh() { refreshLocal(); refreshPRs() }

    private func refreshLocal() {
        guard let path, localTask == nil else { return }
        let token = generation, base = requestedBase
        isLoadingBranch = true; isLoadingUnstaged = true
        localTask = Task {
            let branchReview = await git.fetchBranchReview(at: path, baseBranch: base)
            let local = await git.fetchLocalReview(at: path)
            guard !Task.isCancelled, generation == token else { return }
            comparisonBase = branchReview.comparisonBase
            availableBases = branchReview.availableBases
            branchError = branchReview.error; localError = local.error
            if branchReview.error == nil {
                branchDiff = branchReview.files; commits = branchReview.commits
                if let selectedCommit {
                    if commits.contains(where: { $0.sha == selectedCommit.sha }) { selectCommit(selectedCommit) }
                    else { commitTask?.cancel(); self.selectedCommit = nil; commitDiff = []; isLoadingCommit = false }
                }
            }
            if local.stagedError == nil { stagedDiff = local.staged }
            if local.unstagedError == nil { unstagedDiff = local.unstaged }
            if local.untrackedError == nil { untrackedDiff = local.untracked }
            hasLoadedBranch = true; hasLoadedLocal = true
            isLoadingBranch = false; isLoadingUnstaged = false; localTask = nil
        }
    }

    private func refreshPRs() {
        guard let path, prTask == nil else { return }
        let token = generation, branch = branch
        lastPRRefresh = Date(); isLoadingPRs = true
        prTask = Task {
            do {
                let prs = try await git.fetchPRListChecked(cwd: path, branch: branch)
                guard !Task.isCancelled, generation == token else { return }
                let shouldSelectInitialPR = !initialPRSelectionMade
                initialPRSelectionMade = true
                ghAvailable = true; pullRequests = prs; prError = nil
                if let selected = selectedPR, let updated = prs.first(where: { $0.number == selected.number }) { selectPR(updated) }
                else if shouldSelectInitialPR, let first = prs.first { selectPR(first) }
                else if prs.isEmpty || selectedPR != nil { prDiffTask?.cancel(); selectedPR = nil; prDiff = nil; isLoadingPRDiff = false }
            } catch {
                guard !Task.isCancelled, generation == token else { return }
                // Keep previously fetched evidence and expose auth/network/decoding failures.
                prError = error.localizedDescription; ghAvailable = false
            }
            isLoadingPRs = false; prTask = nil
        }
    }

    func stopWatching() {
        generation = UUID()
        path = nil
        watcher?.stop(); watcher = nil
        localTask?.cancel(); localTask = nil; prTask?.cancel(); prTask = nil
        commitTask?.cancel(); commitTask = nil; prDiffTask?.cancel(); prDiffTask = nil
        pollingTask?.cancel(); pollingTask = nil
        isLoadingBranch = false; isLoadingUnstaged = false; isLoadingCommit = false
        isLoadingPRs = false; isLoadingPRDiff = false
    }
}
