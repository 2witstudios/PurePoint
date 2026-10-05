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

    @ObservationIgnored weak var registry: WorkspaceRegistry?
    @ObservationIgnored weak var appState: AppState?

    @ObservationIgnored private let service: any WorkspaceService
    @ObservationIgnored private var manifestWatcher: ManifestWatcher?
    @ObservationIgnored private var gridSubscription: DaemonGridSubscription?
    @ObservationIgnored private var statusSubscription: DaemonStatusSubscription?
    @ObservationIgnored private var statusSubscriptionTask: Task<Void, Never>?
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var openTask: Task<Void, Never>?
    @ObservationIgnored private var gridSubscriptionTask: Task<Void, Never>?

    /// Agents killed locally whose removal the daemon has not written to the manifest yet.
    /// Without this, a refresh landing in that window would re-adopt the dead agent into a
    /// brand-new workspace — a row appearing for a pane the user just closed.
    @ObservationIgnored private var killedAgentIds = Set<String>()

    init(projectRoot: String, service: any WorkspaceService, registry: WorkspaceRegistry?) {
        self.projectRoot = projectRoot
        self.service = service
        self.registry = registry
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
            // Resume from the snapshot taken after init, not after a fixed delay: a
            // slow first load would otherwise resume against the agents from before
            // init (or a daemon restart) and leave the newly suspended ones paused.
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard !Task.isCancelled else { return }
                self.apply(rootAgents: snapshot.rootAgents, worktrees: snapshot.worktrees)
            } catch is CancellationError {
                return
            } catch {
                self.appState?.daemonError = error.localizedDescription
                return
            }
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

                self.apply(rootAgents: snapshot.rootAgents, worktrees: snapshot.worktrees)
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
            worktreeIdForWorkspace: { self.registry?.workspace(id: $0)?.container.worktreeId }
        )

        sendDaemonRequest(
            .spawn(
                projectRoot: root, prompt: prompt, agent: agent,
                name: name, root: target.root, worktree: target.worktree,
                command: command
            )
        ) { response in
            if case .spawnResult(_, let agentId, _) = response {
                if self.appState?.projectState(forRoot: self.projectRoot) != nil {
                    self.appState?.activeProjectRoot = self.projectRoot
                }
                self.appState?.pendingSelectAgentId = agentId
            }
        }
    }

    /// Spawn an agent into a specific pane.
    ///
    /// The pane is reserved before the request goes out, so whichever arrives first — the
    /// manifest write or the spawn response — the agent lands in this pane and never
    /// surfaces as a workspace of its own.
    func spawnAgentForPane(agent: String, prompt: String, workspaceId: String, leafId: Int) {
        guard let registry, let workspace = registry.workspace(id: workspaceId) else { return }
        let root = projectRoot
        let spawnWorktree = workspace.container.worktreeId

        registry.reservePane(projectRoot: root, workspaceId: workspaceId, leafId: leafId)

        sendDaemonRequest(
            .spawn(
                projectRoot: root, prompt: prompt, agent: agent,
                root: spawnWorktree == nil, worktree: spawnWorktree
            ),
            onFailure: { [weak registry] in
                registry?.releaseReservation(projectRoot: root, workspaceId: workspaceId, leafId: leafId)
            }
        ) { [weak registry] response in
            if case .spawnResult(_, let agentId, _) = response {
                registry?.fulfillReservation(
                    projectRoot: root, workspaceId: workspaceId, leafId: leafId, agentId: agentId)
            } else {
                registry?.releaseReservation(projectRoot: root, workspaceId: workspaceId, leafId: leafId)
            }
        }
    }

    func createWorktree(name: String?) {
        sendDaemonRequest(.createWorktree(projectRoot: projectRoot, name: name)) { response in
            if case .createWorktreeResult(let worktreeId) = response {
                if self.appState?.projectState(forRoot: self.projectRoot) != nil {
                    self.appState?.activeProjectRoot = self.projectRoot
                }
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
        case .openFilePane:
            break  // Only offered by pane-targeted palettes, which handle it themselves.
        }
    }

    /// Eagerly remove an agent from the local model, then async kill via daemon.
    /// Used by pane-close to prevent sidebar flash.
    func removeAndKillAgent(_ agentId: String) {
        killedAgentIds.insert(agentId)
        rootAgents.removeAll { $0.id == agentId }
        for i in worktrees.indices {
            worktrees[i].agents.removeAll { $0.id == agentId }
        }
        registry?.reconcile(projectRoot: projectRoot, rootAgents: rootAgents, worktrees: worktrees)
        killAgent(agentId)
    }

    func killAgent(_ agentId: String) {
        killedAgentIds.insert(agentId)
        // The agent stays hidden while the request is pending; if the daemon refuses, it is
        // still alive and must come back.
        sendDaemonRequest(
            .kill(projectRoot: projectRoot, target: .agent(agentId)),
            onFailure: { [weak self] in
                self?.killedAgentIds.remove(agentId)
                self?.refresh()
            }
        ) { _ in }
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

    /// The one place manifest data enters this project's state.
    ///
    /// Reconciling on the same edge that delivers the agents is what keeps panes and rows
    /// in step: there is no window in which an agent exists but its workspace does not.
    private func apply(rootAgents incoming: [AgentModel], worktrees incomingWorktrees: [WorktreeModel]) {
        var filteredRoot = incoming
        var filteredWorktrees = incomingWorktrees

        if !killedAgentIds.isEmpty {
            // A kill the manifest has caught up on can stop being suppressed.
            let stillPresent = Set((incoming + incomingWorktrees.flatMap(\.agents)).map(\.id))
            killedAgentIds.formIntersection(stillPresent)

            filteredRoot = incoming.filter { !killedAgentIds.contains($0.id) }
            filteredWorktrees = incomingWorktrees.map { worktree in
                var copy = worktree
                copy.agents = worktree.agents.filter { !killedAgentIds.contains($0.id) }
                return copy
            }
        }

        mergeWorktrees(filteredWorktrees)
        mergeRootAgents(filteredRoot)
        registry?.reconcile(projectRoot: projectRoot, rootAgents: rootAgents, worktrees: worktrees)
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
        onFailure: (() -> Void)? = nil,
        onSuccess: @escaping (DaemonResponse) -> Void
    ) {
        Task {
            do {
                let client = DaemonClient()
                let response = try await client.send(request)
                if case .error(_, let message) = response {
                    self.appState?.daemonError = message
                    onFailure?()
                } else {
                    onSuccess(response)
                }
            } catch {
                self.appState?.daemonError = error.localizedDescription
                onFailure?()
            }
        }
    }

    private func startStatusSubscription() {
        statusSubscriptionTask?.cancel()
        let previousStatus = statusSubscription
        Task { await previousStatus?.stop() }
        let sub = DaemonStatusSubscription(projectRoot: projectRoot)
        statusSubscription = sub
        statusSubscriptionTask = Task { [weak self] in
            await sub.start(
                onEvent: { worktrees, agents in
                    self?.apply(rootAgents: agents, worktrees: worktrees)
                },
                // The daemon is gone (crashed, or exited with the app instance
                // that launched it). startWatching ensures a daemon, then inits and
                // resumes, so this project's agents come back on the new one.
                onDaemonLost: {
                    self?.startWatching()
                }
            )
        }
    }

    private func startGridSubscription() {
        gridSubscriptionTask?.cancel()
        // Capture before reassigning: the Task runs later and would otherwise stop
        // the new subscription and leak the old one.
        let previousGrid = gridSubscription
        gridSubscription = nil
        Task { await previousGrid?.stop() }
        guard let registry else { return }
        let sub = DaemonGridSubscription(projectRoot: projectRoot, registry: registry)
        gridSubscription = sub
        gridSubscriptionTask = Task { await sub.start() }
    }
}
