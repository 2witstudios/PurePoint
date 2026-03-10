import Foundation
import Network
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
    @ObservationIgnored private var statusSubscription: DaemonStatusSubscription?
    @ObservationIgnored private var statusSubscriptionTask: Task<Void, Never>?
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

        openTask = Task { [weak self] in
            do {
                try await DaemonLifecycle.ensureDaemon()
            } catch is CancellationError {
                return
            } catch {
                self?.appState?.daemonError = error.localizedDescription
                return
            }

            do {
                let client = DaemonClient()
                let response = try await client.send(.initProject(projectRoot: root))
                if case .error(_, let message) = response {
                    self?.appState?.daemonError = message
                    return
                }
            } catch is CancellationError {
                return
            } catch {
                self?.appState?.daemonError = error.localizedDescription
                return
            }

            guard let self, !Task.isCancelled else { return }

            let manifestPath = svc.manifestPath(projectRoot: root)
            self.manifestWatcher = ManifestWatcher(path: manifestPath) { [weak self] in
                self?.refresh()
            }

            self.startGridSubscription()
            self.startStatusSubscription()
            self.refresh()
            // Resume suspended agents after initial data load
            try? await Task.sleep(nanoseconds: 300_000_000)
            self.resumeSuspendedAgents()
        }
    }

    func stopWatching() {
        openTask?.cancel()
        refreshTask?.cancel()
        gridSubscriptionTask?.cancel()
        statusSubscriptionTask?.cancel()
        let currentGrid = gridSubscription
        let currentStatus = statusSubscription
        gridSubscription = nil
        statusSubscription = nil
        Task { await currentGrid?.stop() }
        Task { await currentStatus?.stop() }
        manifestWatcher?.stop()
        manifestWatcher = nil
    }

    // MARK: - Data

    func refresh() {
        let root = projectRoot
        let svc = service

        refreshTask?.cancel()
        refreshTask = Task { [weak self] in
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard let self, !Task.isCancelled else { return }

                self.assignPendingSpawnsToGrid(snapshot.rootAgents, incomingWorktrees: snapshot.worktrees)
                self.mergeWorktrees(snapshot.worktrees)
                self.mergeRootAgents(snapshot.rootAgents)
            } catch is CancellationError {
                // Task was cancelled (new refresh started) — ignore
            } catch {
                self?.appState?.daemonError = error.localizedDescription
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

    func createAgent(
        agent: String, prompt: String, name: String? = nil, isWorktree: Bool = false, selection: SidebarSelection?,
        command: String? = nil
    ) {
        let root = projectRoot
        let target = SpawnTargetResolver.resolve(
            isWorktree: isWorktree,
            selection: selection,
            worktreeIdForAgent: { self.worktreeId(forAgentId: $0) }
        )

        sendDaemonRequest(
            .spawn(
                projectRoot: root, prompt: prompt, agent: agent,
                name: name, root: target.root, worktree: target.worktree,
                command: command
            )
        ) { response in
            if case .spawnResult(_, let agentId, _) = response {
                self.appState?.pendingSelectAgentId = agentId
            }
        }
    }

    func spawnAgentForPane(agent: String, prompt: String, leafId: Int, gridState: GridState) {
        let root = projectRoot
        let spawnWorktree: String?
        if let ownerId = gridState.ownerAgentId,
            let wtId = worktreeId(forAgentId: ownerId)
        {
            spawnWorktree = wtId
        } else {
            spawnWorktree = nil
        }

        gridState.pendingSpawnLeafIds.insert(leafId)

        sendDaemonRequest(
            .spawn(
                projectRoot: root, prompt: prompt, agent: agent,
                root: spawnWorktree == nil, worktree: spawnWorktree
            ),
            onComplete: { gridState.pendingSpawnLeafIds.remove(leafId) }
        ) { response in
            if case .spawnResult(_, let agentId, _) = response {
                gridState.setAgent(agentId, forLeafId: leafId)
            }
        }
    }

    func createWorktree(name: String?) {
        sendDaemonRequest(.createWorktree(projectRoot: projectRoot, name: name)) { response in
            if case .createWorktreeResult(let worktreeId) = response {
                self.appState?.pendingSelectWorktreeId = worktreeId
            }
        }
    }

    func handlePaletteResult(_ result: CommandPaletteResult, selection: SidebarSelection?, hub: AgentsHubState) {
        switch result {
        case .spawnBuiltIn(let variant, let prompt, let name):
            createAgent(
                agent: variant.id, prompt: prompt ?? "", name: name, isWorktree: variant.kind == .worktree,
                selection: selection)
        case .spawnAgentDef(let def, let prompt):
            createAgent(
                agent: def.agentType, prompt: prompt ?? def.inlinePrompt ?? "",
                selection: selection, command: def.command)
        case .runSwarm(let def):
            let root = projectRoot
            Task { await hub.runSwarm(projectRoot: root, name: def.name) }
        case .createWorktree(let name):
            createWorktree(name: name)
        }
    }

    /// Eagerly remove an agent from the local model, then async kill via daemon.
    /// Used by pane-close to prevent sidebar flash.
    func removeAndKillAgent(_ agentId: String) {
        rootAgents.removeAll { $0.id == agentId }
        for i in worktrees.indices {
            worktrees[i].agents.removeAll { $0.id == agentId }
        }
        killAgent(agentId)
    }

    func killAgent(_ agentId: String) {
        sendDaemonCommand(.kill(projectRoot: projectRoot, target: .agent(agentId)))
    }

    func renameAgent(_ agentId: String, to name: String) {
        sendDaemonCommand(.rename(projectRoot: projectRoot, agentId: agentId, name: name))
    }

    func killAllAgents() {
        sendDaemonCommand(.kill(projectRoot: projectRoot, target: .all))
    }

    func deleteWorktree(_ worktreeId: String) {
        sendDaemonCommand(.deleteWorktree(projectRoot: projectRoot, worktreeId: worktreeId))
    }

    func killWorktreeAgents(_ worktreeId: String) {
        sendDaemonCommand(.kill(projectRoot: projectRoot, target: .worktree(worktreeId)))
    }

    // MARK: - Resume

    private func resumeSuspendedAgents() {
        for agent in allAgents where agent.suspended {
            let name = agent.displayName
            Task {
                let client = DaemonClient()
                let response = try? await client.send(.resume(projectRoot: projectRoot, agentId: agent.id))
                if case .error(_, let msg) = response {
                    self.appState?.daemonError = "Resume failed for \(name): \(msg)"
                }
            }
        }
    }

    // MARK: - Selective Merge

    /// Merge root agents by ID: update changed agents in-place, add new, remove stale.
    /// Only triggers view updates for agents whose observable properties changed.
    private func mergeRootAgents(_ incoming: [AgentModel]) {
        let incomingById = Dictionary(uniqueKeysWithValues: incoming.map { ($0.id, $0) })
        let currentById = Dictionary(uniqueKeysWithValues: rootAgents.map { ($0.id, $0) })

        // If the sets differ, just replace (avoids complex diff for add/remove)
        if Set(incomingById.keys) != Set(currentById.keys) || rootAgents != incoming {
            rootAgents = incoming
        }
    }

    /// Merge worktrees by ID with nested agent merge.
    private func mergeWorktrees(_ incoming: [WorktreeModel]) {
        if worktrees != incoming {
            worktrees = incoming
        }
    }

    // MARK: - Private

    /// Fire-and-forget daemon command with standard error handling.
    private func sendDaemonCommand(_ request: DaemonRequest) {
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(request)
                if case .error(_, let message) = response { self.appState?.daemonError = message }
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    /// Daemon request with response routing and standard error handling.
    private func sendDaemonRequest(
        _ request: DaemonRequest,
        onComplete: (() -> Void)? = nil,
        onSuccess: @escaping (DaemonResponse) -> Void
    ) {
        Task {
            defer { onComplete?() }
            do {
                let client = DaemonClient()
                let response = try await client.send(request)
                if case .error(_, let message) = response {
                    self.appState?.daemonError = message
                } else {
                    onSuccess(response)
                }
            } catch {
                self.appState?.daemonError = error.localizedDescription
            }
        }
    }

    /// Eagerly assign newly-appeared agents to pending grid leaves before merging,
    /// so they appear in childAgentIds immediately (sidebar leak prevention).
    private func assignPendingSpawnsToGrid(_ incomingRootAgents: [AgentModel], incomingWorktrees: [WorktreeModel] = [])
    {
        guard let gs = gridState, gs.projectRoot == projectRoot else { return }
        var pending = gs.pendingSpawnLeafIds
        guard !pending.isEmpty else { return }
        let currentIds = Set(rootAgents.map(\.id) + worktrees.flatMap(\.agents).map(\.id))
        let allIncoming = incomingRootAgents + incomingWorktrees.flatMap(\.agents)
        for agent in allIncoming where !currentIds.contains(agent.id) {
            guard let leafId = pending.first else { break }
            pending.remove(leafId)
            gs.pendingSpawnLeafIds.remove(leafId)
            gs.setAgent(agent.id, forLeafId: leafId)
        }
    }

    private func startStatusSubscription() {
        statusSubscriptionTask?.cancel()
        let previousStatus = statusSubscription
        Task { await previousStatus?.stop() }
        let sub = DaemonStatusSubscription(projectRoot: projectRoot)
        statusSubscription = sub
        statusSubscriptionTask = Task { [weak self] in
            await sub.start { worktrees, agents in
                self?.assignPendingSpawnsToGrid(agents, incomingWorktrees: worktrees)
                self?.mergeWorktrees(worktrees)
                self?.mergeRootAgents(agents)
            }
        }
    }

    private func startGridSubscription() {
        gridSubscriptionTask?.cancel()
        Task { await gridSubscription?.stop() }
        guard let gs = gridState else { return }
        let sub = DaemonGridSubscription(projectRoot: projectRoot, gridState: gs)
        gridSubscription = sub
        gridSubscriptionTask = Task { await sub.start() }
    }
}
