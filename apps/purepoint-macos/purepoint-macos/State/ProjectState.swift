import Foundation
import Observation

/// Per-project state: agents, worktrees, manifest watching, and daemon interaction.
/// AppState holds an array of these — one per open project.
@Observable
@MainActor
final class ProjectState: Identifiable {
    let projectRoot: String
    nonisolated var id: String { projectRoot }
    var projectName: String { URL(fileURLWithPath: projectRoot).lastPathComponent }

    var rootAgents: [AgentModel] = []
    var worktrees: [WorktreeModel] = []

    @ObservationIgnored weak var gridState: GridState?
    @ObservationIgnored weak var appState: AppState?

    @ObservationIgnored private let service: any WorkspaceService
    @ObservationIgnored private var manifestWatcher: ManifestWatcher?
    @ObservationIgnored private var gridSubscription: DaemonGridSubscription?
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var openTask: Task<Void, Never>?
    @ObservationIgnored private var gridSubscriptionTask: Task<Void, Never>?

    init(projectRoot: String, service: any WorkspaceService, gridState: GridState?) {
        self.projectRoot = projectRoot
        self.service = service
        self.gridState = gridState
    }

    // MARK: - Lifecycle

    func startWatching() {
        let root = projectRoot
        let svc = service

        openTask?.cancel()
        refreshTask?.cancel()
        manifestWatcher?.stop()
        manifestWatcher = nil

        openTask = Task {
            do {
                try await DaemonLifecycle.ensureDaemon()
            } catch is CancellationError {
                return
            } catch {
                self.appState?.daemonError = error.localizedDescription
                return
            }

            do {
                let client = DaemonClient()
                let response = try await client.send(.initProject(projectRoot: root))
                if case .error(_, let message) = response {
                    self.appState?.daemonError = message
                    return
                }
            } catch is CancellationError {
                return
            } catch {
                self.appState?.daemonError = error.localizedDescription
                return
            }

            guard !Task.isCancelled else { return }

            let manifestPath = svc.manifestPath(projectRoot: root)
            self.manifestWatcher = ManifestWatcher(path: manifestPath) { [weak self] in
                self?.refresh()
            }

            self.startGridSubscription()
            self.refresh()
        }
    }

    func stopWatching() {
        openTask?.cancel()
        refreshTask?.cancel()
        gridSubscriptionTask?.cancel()
        Task { await gridSubscription?.stop() }
        manifestWatcher?.stop()
        manifestWatcher = nil
    }

    // MARK: - Data

    func refresh() {
        let root = projectRoot
        let svc = service

        refreshTask?.cancel()
        refreshTask = Task {
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard !Task.isCancelled else { return }

                // Sidebar leak fix: eagerly assign new root agents to pending grid leaves
                // before updating rootAgents, so they appear in childAgentIds immediately.
                if let gs = gridState, gs.projectRoot == root {
                    var pending = gs.pendingSpawnLeafIds
                    if !pending.isEmpty {
                        let currentIds = Set(self.rootAgents.map(\.id))
                        let newAgents = snapshot.rootAgents.filter { !currentIds.contains($0.id) }
                        for agent in newAgents {
                            guard let leafId = pending.first else { break }
                            pending.remove(leafId)
                            gs.pendingSpawnLeafIds.remove(leafId)
                            gs.setAgent(agent.id, forLeafId: leafId)
                        }
                    }
                }

                if self.worktrees != snapshot.worktrees {
                    self.worktrees = snapshot.worktrees
                }
                if self.rootAgents != snapshot.rootAgents {
                    self.rootAgents = snapshot.rootAgents
                }
            } catch is CancellationError {
                // Task was cancelled (new refresh started) — ignore
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    // MARK: - Queries

    func agent(byId id: String) -> AgentModel? {
        for wt in worktrees {
            if let agent = wt.agents.first(where: { $0.id == id }) { return agent }
        }
        return rootAgents.first(where: { $0.id == id })
    }

    var allAgents: [AgentModel] {
        worktrees.flatMap(\.agents) + rootAgents
    }

    func worktreeId(forAgentId agentId: String) -> String? {
        for wt in worktrees {
            if wt.agents.contains(where: { $0.id == agentId }) { return wt.id }
        }
        return nil
    }

    // MARK: - Agent Operations

    func createAgent(variant: AgentVariant, prompt: String?, selection: SidebarSelection?) {
        let root = projectRoot

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
                    projectRoot: root, prompt: prompt ?? "", agent: variant.id,
                    root: spawnRoot, worktree: spawnWorktree
                ))
                if case .error(_, let message) = response {
                    self.appState?.daemonError = message
                }
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    func spawnAgentForPane(variant: AgentVariant, prompt: String?, leafId: Int, gridState: GridState) {
        let root = projectRoot

        // Mark leaf as pending for sidebar leak prevention
        gridState.pendingSpawnLeafIds.insert(leafId)

        Task {
            defer { gridState.pendingSpawnLeafIds.remove(leafId) }
            do {
                let client = DaemonClient()
                let response = try await client.send(.spawn(
                    projectRoot: root, prompt: prompt ?? "", agent: variant.id,
                    root: true, worktree: nil
                ))
                switch response {
                case .spawnResult(_, let agentId, _):
                    gridState.setAgent(agentId, forLeafId: leafId)
                case .error(_, let message):
                    self.appState?.daemonError = message
                default:
                    break
                }
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    func killAgent(_ agentId: String) {
        let root = projectRoot
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(.kill(projectRoot: root, target: .agent(agentId)))
                if case .error(_, let message) = response { self.appState?.daemonError = message }
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    func killWorktreeAgents(_ worktreeId: String) {
        let root = projectRoot
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(.kill(projectRoot: root, target: .worktree(worktreeId)))
                if case .error(_, let message) = response { self.appState?.daemonError = message }
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    // MARK: - Private

    private func startGridSubscription() {
        gridSubscriptionTask?.cancel()
        Task { await gridSubscription?.stop() }
        guard let gs = gridState else { return }
        let sub = DaemonGridSubscription(projectRoot: projectRoot, gridState: gs)
        gridSubscription = sub
        gridSubscriptionTask = Task { await sub.start() }
    }
}
