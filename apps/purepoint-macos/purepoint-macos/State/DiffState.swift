import Foundation
import Observation
#if os(macOS)
import AppKit
#else
import UIKit
#endif

nonisolated enum DiffTab: String, CaseIterable {
    case branch = "Branch Changes"
    case unstaged = "Uncommitted"
    case commits = "Commits"
    case prDiffs = "PR Diffs"
}

@Observable
@MainActor
final class DiffState {
    var activeTab: DiffTab = .branch {
        didSet {
            if summariesOnly, activeTab != .prDiffs { prDiffTask?.cancel(); isLoadingPRDiff = false }
            if activeTab == .prDiffs, (prDiff == nil || isPRDiffStale) {
                if let selectedPR { selectPR(selectedPR) }
                else if !hasSelectedPR, let first = pullRequests.first { selectPR(first) }
            }
        }
    }
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
    var commitError: String?
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
    private var loadedPRVersion: String?
    var isPRDiffStale: Bool { prDiff != nil && loadedPRVersion != selectedPR?.updatedAt }
    var isLoadingPRs = false
    var isLoadingPRDiff = false
    var ghAvailable = true
    var error: String? { branchError ?? commitError ?? localError ?? prError }
    var localFileCount: Int { Set((stagedDiff + unstagedDiff + untrackedDiff).map(\.filename)).count }

    private var git: GitService
    private let observeChanges: Bool
    private var remoteGit: GitService
    private let summariesOnly: Bool
    init(gitService: GitService? = nil, observeChanges: Bool = true, summariesOnly: Bool = true) {
        git = gitService ?? GitService()
        remoteGit = gitService ?? GitService()
        self.observeChanges = observeChanges
        self.summariesOnly = summariesOnly
    }

    #if os(macOS)
    private var watcher: WorktreeWatcher?
    #endif
    private var path: String?
    private var branch: String?
    private var requestedBase: String?
    private var generation = UUID()
    var reviewIdentity: UUID { generation }
    private var localTask: Task<Void, Never>?
    private var prTask: Task<Void, Never>?
    private var commitTask: Task<Void, Never>?
    private var prDiffTask: Task<Void, Never>?
    private var pollingTask: Task<Void, Never>?
    private var initialPRSelectionMade = false
    private var hasSelectedPR = false
    private var lastPRRefresh = Date.distantPast

    /// Route git through a project's host. A remote host reaches git over its machine's daemon.
    func use(host: any ProjectHost) {
        guard host.identity != git.host.identity else { return }
        stopWatching()
        git = GitService(host: host)
        remoteGit = GitService(host: host)
        previewGit = GitService(host: host)
    }

    func loadForWorktree(_ worktree: WorktreeModel) {
        load(path: worktree.path, branch: worktree.branch, base: worktree.baseBranch)
    }
    func loadForProject(root: String) { load(path: root, branch: nil, base: nil) }
    func loadForProject(projectRoot: String) { loadForProject(root: projectRoot) }

    private func load(path: String, branch: String?, base: String?) {
        stopWatching()
        self.path = path; self.branch = branch; requestedBase = base
        initialPRSelectionMade = false; hasSelectedPR = false
        activeTab = .branch; comparisonBase = base ?? ""; availableBases = []
        branchDiff = []; stagedDiff = []; unstagedDiff = []; untrackedDiff = []
        commits = []; selectedCommit = nil; commitDiff = []
        pullRequests = []; selectedPR = nil; prDiff = nil; loadedPRVersion = nil
        branchError = nil; commitError = nil; localError = nil; prError = nil
        hasLoadedBranch = false; hasLoadedLocal = false; hasLoadedCommit = false
        refresh()
        // Explicitly driven callers can own refresh timing without background races.
        guard observeChanges else { return }
        // Metadata watching is local-only; remote worktrees rely on the polling below.
        #if os(macOS)
        if git.host.isLocal {
            watcher = WorktreeWatcher(worktreePath: path) { [weak self] in
                Task { @MainActor in self?.refreshLocal() }
            }
        }
        #endif
        let pollSeconds = git.host.isLocal ? 2 : 5
        pollingTask = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(pollSeconds))
                #if os(iOS)
                // Paused in the background; refreshes as soon as the app returns.
                await AppActivity.shared.untilForeground()
                #endif
                guard !Task.isCancelled, let self else { return }
                #if os(macOS)
                // Recursive file changes need polling. Hidden/background app work is bounded.
                guard NSApplication.shared.isActive else { continue }
                #endif
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
        selectedCommit = commit; commitTask?.cancel(); isLoadingCommit = true; commitError = nil
        let token = generation
        commitTask = Task {
            do {
                let files = try await git.fetchCommitDiff(at: path, sha: commit.sha, summariesOnly: summariesOnly)
                guard !Task.isCancelled, generation == token, selectedCommit?.sha == commit.sha else { return }
                commitDiff = files; hasLoadedCommit = true
            } catch {
                guard !Task.isCancelled, generation == token, selectedCommit?.sha == commit.sha else { return }
                commitError = error.localizedDescription
            }
            isLoadingCommit = false
        }
    }

    func selectPR(_ pr: PullRequestInfo) {
        guard let path else { return }
        let changed = selectedPR?.number != pr.number
        hasSelectedPR = true
        selectedPR = pr; prDiffTask?.cancel(); isLoadingPRDiff = true
        if changed { prDiff = nil; loadedPRVersion = nil }
        let token = generation
        prDiffTask = Task {
            do {
                let diff = try await remoteGit.fetchPRDiffChecked(cwd: path, prNumber: pr.number, summariesOnly: summariesOnly)
                guard !Task.isCancelled, generation == token, selectedPR?.number == pr.number else { return }
                prDiff = diff; loadedPRVersion = pr.updatedAt; prError = nil
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

    private var previewGit = GitService()
    func loadPage(_ file: FileDiff, after page: ReviewPageCursor? = nil) async throws -> ReviewPage {
        guard let path else { throw CancellationError() }
        let token = generation
        let loaded = try await previewGit.fetchReviewPage(file: file, at: path, cursor: page)
        guard !Task.isCancelled, token == generation else { throw CancellationError() }
        return loaded
    }

    func refresh() { refreshLocal(); refreshPRs(forceDiff: true) }

    private func refreshLocal() {
        guard let path, localTask == nil else { return }
        let token = generation, base = requestedBase
        isLoadingBranch = true; isLoadingUnstaged = true
        localTask = Task {
            let branchReview = await git.fetchBranchReview(at: path, baseBranch: base, summariesOnly: summariesOnly)
            guard !Task.isCancelled, generation == token else { return }
            let local = await git.fetchLocalReview(at: path, summariesOnly: summariesOnly)
            guard !Task.isCancelled, generation == token else { return }
            comparisonBase = branchReview.comparisonBase
            availableBases = branchReview.availableBases
            branchError = branchReview.error; localError = local.error
            if branchReview.error == nil {
                branchDiff = branchReview.files; commits = branchReview.commits
                if let selectedCommit {
                    if commits.contains(where: { $0.sha == selectedCommit.sha }) { if !summariesOnly || (!hasLoadedCommit && !isLoadingCommit) { selectCommit(selectedCommit) } }
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

    func refreshPRs(forceDiff: Bool = false) {
        guard let path, prTask == nil else { return }
        let token = generation, branch = branch
        lastPRRefresh = Date(); isLoadingPRs = true
        prTask = Task {
            do {
                let prs = try await remoteGit.fetchPRListChecked(cwd: path, branch: branch)
                guard !Task.isCancelled, generation == token else { return }
                let shouldSelectInitialPR = !initialPRSelectionMade
                initialPRSelectionMade = true
                ghAvailable = true; pullRequests = prs; prError = nil
                if let selected = selectedPR, let updated = prs.first(where: { $0.number == selected.number }) {
                    if !summariesOnly || activeTab == .prDiffs {
                        if forceDiff || !summariesOnly || loadedPRVersion != updated.updatedAt { selectPR(updated) }
                    } else if selected.updatedAt != updated.updatedAt {
                        selectedPR = updated
                    }
                }
                else if shouldSelectInitialPR, !hasSelectedPR, let first = prs.first, !summariesOnly || activeTab == .prDiffs { selectPR(first) }
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
        #if os(macOS)
        watcher?.stop(); watcher = nil
        #endif
        localTask?.cancel(); localTask = nil; prTask?.cancel(); prTask = nil
        commitTask?.cancel(); commitTask = nil; prDiffTask?.cancel(); prDiffTask = nil
        pollingTask?.cancel(); pollingTask = nil
        isLoadingBranch = false; isLoadingUnstaged = false; isLoadingCommit = false
        isLoadingPRs = false; isLoadingPRDiff = false
    }
}
