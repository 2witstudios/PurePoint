import Foundation
import Observation

nonisolated enum DiffTab: String, CaseIterable {
    case unstaged = "Unstaged Changes"
    case prDiffs = "PR Diffs"
}

@Observable
@MainActor
final class DiffState {
    var activeTab: DiffTab = .unstaged
    var unstagedDiff: DiffData?
    var isLoadingUnstaged = false
    var pullRequests: [PullRequestInfo] = []
    var selectedPR: PullRequestInfo?
    var prDiff: DiffData?
    var isLoadingPRs = false
    var isLoadingPRDiff = false
    var error: String?
    var ghAvailable = true

    private var watcher: WorktreeWatcher?
    private var currentWorktreePath: String?
    private var currentProjectRoot: String?
    private var unstagedTask: Task<Void, Never>?
    private var prTask: Task<Void, Never>?

    // MARK: - Load for Worktree

    func loadForWorktree(_ worktree: WorktreeModel) {
        let path = worktree.path
        let branch = worktree.branch
        currentWorktreePath = path
        currentProjectRoot = nil

        unstagedTask?.cancel()
        prTask?.cancel()
        unstagedTask = Task { await fetchUnstaged(path: path) }
        prTask = Task { await fetchPRs(cwd: path, branch: branch) }

        startWatching(path: path)
    }

    // MARK: - Load for Project

    func loadForProject(projectRoot: String) {
        currentProjectRoot = projectRoot
        currentWorktreePath = nil

        unstagedTask?.cancel()
        prTask?.cancel()
        unstagedTask = Task { await fetchUnstaged(path: projectRoot) }
        prTask = Task { await fetchPRs(cwd: projectRoot, branch: nil) }

        startWatching(path: projectRoot)
    }

    // MARK: - PR Selection

    func selectPR(_ pr: PullRequestInfo) {
        selectedPR = pr
        let cwd = currentWorktreePath ?? currentProjectRoot ?? ""
        guard !cwd.isEmpty else { return }

        isLoadingPRDiff = true
        prDiff = nil

        Task {
            let diff = await GitService.shared.fetchPRDiff(cwd: cwd, prNumber: pr.number)
            guard !Task.isCancelled else { return }
            self.prDiff = diff
            self.isLoadingPRDiff = false
        }
    }

    // MARK: - Refresh

    func refresh() {
        let path = currentWorktreePath ?? currentProjectRoot
        guard let path else { return }
        unstagedTask?.cancel()
        unstagedTask = Task { await fetchUnstaged(path: path) }
        // Don't re-fetch PRs on refresh — they don't change on file save
    }

    // MARK: - Watcher

    func startWatching(path: String) {
        watcher?.stop()
        watcher = WorktreeWatcher(worktreePath: path) { [weak self] in
            let captured = self
            Task { @MainActor in
                captured?.refresh()
            }
        }
    }

    func stopWatching() {
        watcher?.stop()
        watcher = nil
        unstagedTask?.cancel()
        prTask?.cancel()
    }

    // MARK: - Private

    private func fetchUnstaged(path: String) async {
        isLoadingUnstaged = true
        error = nil
        let diff = await GitService.shared.fetchUnstagedDiff(worktreePath: path)
        guard !Task.isCancelled else { return }
        self.unstagedDiff = diff
        self.isLoadingUnstaged = false
    }

    private func fetchPRs(cwd: String, branch: String?) async {
        isLoadingPRs = true
        let available = await GitService.shared.isGhAvailable(cwd: cwd)
        guard !Task.isCancelled else { return }
        self.ghAvailable = available

        if available {
            let prs = await GitService.shared.fetchPRList(cwd: cwd, branch: branch)
            guard !Task.isCancelled else { return }
            self.pullRequests = prs
            // Auto-select first PR if none selected
            if selectedPR == nil, let first = prs.first {
                selectPR(first)
            }
        } else {
            self.pullRequests = []
        }
        self.isLoadingPRs = false
    }
}
