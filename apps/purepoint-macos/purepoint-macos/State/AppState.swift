import Foundation
import Observation

@Observable
@MainActor
final class AppState {
    var worktrees: [WorktreeModel] = []
    var rootAgents: [AgentModel] = []
    var projectRoot: String = ""
    var projectName: String { URL(fileURLWithPath: projectRoot).lastPathComponent }
    var isLoaded: Bool { !projectRoot.isEmpty }
    var daemonError: String?

    /// The agent ID currently selected in the sidebar (for single-pane display).
    /// Written by ContentView's onChange(of: selection), read by app-level commands.
    var selectedAgentId: String?

    /// Set by the app entry point for grid subscription wiring and restore.
    weak var gridState: GridState?

    private let service: any WorkspaceService
    private var manifestWatcher: ManifestWatcher?
    private var binaryWatcher: ManifestWatcher?
    private var refreshTask: Task<Void, Never>?
    private var openProjectTask: Task<Void, Never>?
    private var gridSubscription: DaemonGridSubscription?
    private var gridSubscriptionTask: Task<Void, Never>?

    init(service: any WorkspaceService = DaemonWorkspaceService()) {
        self.service = service
    }

    func openProject(_ root: String) {
        projectRoot = root
        daemonError = nil
        manifestWatcher?.stop()
        manifestWatcher = nil
        openProjectTask?.cancel()
        refreshTask?.cancel()

        // Ensure daemon is running, then start watching and load workspace
        let svc = service
        openProjectTask = Task {
            do {
                try await DaemonLifecycle.ensureDaemon()
            } catch is CancellationError {
                return
            } catch {
                self.daemonError = error.localizedDescription
                return
            }

            // Initialize project (creates .pu/manifest.json if missing)
            do {
                let client = DaemonClient()
                let response = try await client.send(.initProject(projectRoot: root))
                if case .error(_, let message) = response {
                    self.daemonError = message
                    return
                }
            } catch is CancellationError {
                return
            } catch {
                self.daemonError = error.localizedDescription
                return
            }

            guard !Task.isCancelled else { return }

            // Start manifest watcher after daemon is ready (avoids transient errors
            // from querying the daemon before it's healthy).
            let manifestPath = svc.manifestPath(projectRoot: root)
            self.manifestWatcher = ManifestWatcher(path: manifestPath) { [weak self] in
                self?.refresh()
            }

            // Watch the daemon binary for changes (auto-restart on rebuild)
            if let binPath = DaemonLifecycle.findBinary() {
                self.binaryWatcher = ManifestWatcher(path: binPath) { [weak self] in
                    self?.restartDaemonAndRefresh()
                }
            }

            // Restore grid layout if one was saved
            self.gridState?.restore(projectRoot: root)

            // Start grid subscription
            self.startGridSubscription(projectRoot: root)

            self.refresh()
        }
    }

    func refresh() {
        guard !projectRoot.isEmpty else { return }
        let root = projectRoot
        let svc = service

        refreshTask?.cancel()
        refreshTask = Task {
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard !Task.isCancelled, self.projectRoot == root else { return }
                if self.worktrees != snapshot.worktrees {
                    self.worktrees = snapshot.worktrees
                }
                if self.rootAgents != snapshot.rootAgents {
                    self.rootAgents = snapshot.rootAgents
                }
            } catch is CancellationError {
                // Task was cancelled (new refresh started) — ignore
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }

    /// Find an agent by ID across all worktrees and root agents.
    func agent(byId id: String) -> AgentModel? {
        for wt in worktrees {
            if let agent = wt.agents.first(where: { $0.id == id }) {
                return agent
            }
        }
        return rootAgents.first(where: { $0.id == id })
    }

    /// All agents across all worktrees and root level.
    var allAgents: [AgentModel] {
        worktrees.flatMap(\.agents) + rootAgents
    }

    /// Find the worktree containing a given agent, if any.
    func worktreeId(forAgentId agentId: String) -> String? {
        for wt in worktrees {
            if wt.agents.contains(where: { $0.id == agentId }) { return wt.id }
        }
        return nil
    }

    func createAgent(variant: AgentVariant, prompt: String?, selection: SidebarSelection?) {
        guard !projectRoot.isEmpty else { return }

        // Resolve spawn target on main actor before entering Task
        let spawnRoot: Bool
        let spawnWorktree: String?

        switch selection {
        case .worktree(let id):
            spawnRoot = false
            spawnWorktree = id
        case .agent(let id):
            if let wtId = worktreeId(forAgentId: id) {
                spawnRoot = false
                spawnWorktree = wtId
            } else {
                spawnRoot = true
                spawnWorktree = nil
            }
        case nil, .nav, .project, .terminal:
            spawnRoot = true
            spawnWorktree = nil
        }

        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(.spawn(
                    projectRoot: projectRoot, prompt: prompt ?? "", agent: variant.id,
                    root: spawnRoot, worktree: spawnWorktree
                ))
                if case .error(_, let message) = response {
                    self.daemonError = message
                }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }

    func spawnAgentForPane(variant: AgentVariant, prompt: String?, leafId: Int, gridState: GridState) {
        guard !projectRoot.isEmpty else { return }
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(.spawn(
                    projectRoot: projectRoot, prompt: prompt ?? "", agent: variant.id,
                    root: true, worktree: nil
                ))
                switch response {
                case .spawnResult(_, let agentId, _):
                    gridState.setAgent(agentId, forLeafId: leafId)
                case .error(_, let message):
                    self.daemonError = message
                default:
                    break
                }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }

    func killAgent(_ agentId: String) {
        guard !projectRoot.isEmpty else { return }
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(.kill(projectRoot: projectRoot, target: .agent(agentId)))
                if case .error(_, let message) = response { self.daemonError = message }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }

    func killWorktreeAgents(_ worktreeId: String) {
        guard !projectRoot.isEmpty else { return }
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(.kill(projectRoot: projectRoot, target: .worktree(worktreeId)))
                if case .error(_, let message) = response { self.daemonError = message }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }

    private func startGridSubscription(projectRoot: String) {
        gridSubscriptionTask?.cancel()
        Task { await gridSubscription?.stop() }
        guard let gs = gridState else { return }
        let sub = DaemonGridSubscription(projectRoot: projectRoot, gridState: gs)
        gridSubscription = sub
        gridSubscriptionTask = Task { await sub.start() }
    }

    private func restartDaemonAndRefresh() {
        Task {
            do {
                try await DaemonLifecycle.restartDaemon()
                self.refresh()
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }

    /// Shut down the daemon and stop watching. Called on app termination.
    func shutdown() {
        openProjectTask?.cancel()
        refreshTask?.cancel()
        gridSubscriptionTask?.cancel()
        Task { await gridSubscription?.stop() }
        manifestWatcher?.stop()
        manifestWatcher = nil
        binaryWatcher?.stop()
        binaryWatcher = nil
        Task {
            let client = DaemonClient()
            _ = try? await client.send(.shutdown)
        }
    }
}
