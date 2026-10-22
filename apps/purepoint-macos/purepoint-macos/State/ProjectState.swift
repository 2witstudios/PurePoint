import Foundation
import Network
import Observation

/// Per-project state: agents, worktrees, manifest watching, and daemon interaction.
/// AppState holds an array of these — one per open project.
@Observable
@MainActor
final class ProjectState: Identifiable {
    /// The root on the machine that owns the project — what daemon requests and file paths use.
    let projectRoot: String
    /// Identity: `(machine, root)`. Its string form keys the registry, sidebar and selection.
    nonisolated let key: ProjectKey
    nonisolated var id: String { key.id }
    nonisolated var projectId: String { key.id }
    var isRemote: Bool { key.isRemote }
    var projectName: String { URL(fileURLWithPath: projectRoot).lastPathComponent }

    /// The daemon this project belongs to; remote projects reach theirs over an SSH forward.
    @ObservationIgnored let endpoint: DaemonEndpoint
    @ObservationIgnored let client: DaemonClient
    /// Where the project's files live and its git runs.
    @ObservationIgnored let host: any ProjectHost
    @ObservationIgnored let git: GitService
    /// Remote projects write their layout through the host; quit and disconnect flush it.
    @ObservationIgnored let remoteLayoutSaver: RemoteLayoutSaver?
    /// A remote project kept in the sidebar while its machine is unreachable, because it has
    /// file tabs with unsaved edits. Nothing is watched until the daemon is back.
    var isUnavailable = false

    let channel: ChannelState

    var rootAgents: [AgentModel] = []
    var worktrees: [WorktreeModel] = []

    /// Whether a snapshot of this project's manifest has been applied at least once.
    /// `worktrees` is empty both before that and for a project that genuinely has none, so a
    /// caller that has to tell the two apart — the sidebar replaying which rows are closed —
    /// reads this instead.
    private(set) var hasLoadedWorktrees = false

    /// Marks the snapshot as applied. Production sets it where the snapshot is read; tests
    /// use this to stand in for the daemon, which unit tests cannot run.
    func markWorktreesLoadedForTesting() {
        hasLoadedWorktrees = true
    }

    @ObservationIgnored weak var registry: WorkspaceRegistry?
    @ObservationIgnored weak var appState: AppState?

    @ObservationIgnored private let service: any WorkspaceService
    #if os(macOS)
    @ObservationIgnored private var manifestWatcher: ManifestWatcher?
    /// Watches this project's `.pu/workspaces.json`, which a Mac working on it over SSH writes too.
    @ObservationIgnored private var layoutWatcher: ManifestWatcher?
    #endif
    @ObservationIgnored private var gridSubscription: DaemonGridSubscription?
    @ObservationIgnored private var statusSubscription: DaemonStatusSubscription?
    @ObservationIgnored private var statusSubscriptionTask: Task<Void, Never>?
    @ObservationIgnored private var refreshTask: Task<Void, Never>?
    @ObservationIgnored private var openTask: Task<Void, Never>?
    @ObservationIgnored private var gridSubscriptionTask: Task<Void, Never>?
    @ObservationIgnored private var remoteLayoutTask: Task<Void, Never>?
    /// The delayed resubscribe after a remote daemon was lost.
    @ObservationIgnored private var resubscribeTask: Task<Void, Never>?

    /// Agents killed locally whose removal the daemon has not written to the manifest yet.
    /// Without this, a refresh landing in that window would re-adopt the dead agent into a
    /// brand-new workspace — a row appearing for a pane the user just closed.
    @ObservationIgnored private var killedAgentIds = Set<String>()

    /// How long to keep retrying daemon start + init before surfacing an error.
    private static let daemonConnectBudget: Duration = .seconds(20)

    #if os(macOS)
    init(
        projectRoot: String, service: any WorkspaceService, registry: WorkspaceRegistry?, channel: ChannelState? = nil,
        endpoint: DaemonEndpoint = .local, host: any ProjectHost = LocalProjectHost.shared
    ) {
        let key = ProjectKey(machineId: endpoint.machineId, root: projectRoot)
        let client = endpoint.makeClient()
        self.projectRoot = projectRoot
        self.key = key
        self.endpoint = endpoint
        self.client = client
        self.host = host
        self.git = host.isLocal ? GitService.shared : GitService(host: host)
        self.remoteLayoutSaver = endpoint.isLocal ? nil : RemoteLayoutSaver(projectRoot: projectRoot, host: host)
        self.channel =
            channel
            ?? (endpoint.isLocal
                ? ChannelState(projectRoot: projectRoot)
                : ChannelState(projectRoot: projectRoot, defaultsScope: key.id) { try await client.send($0) })
        self.service = service
        self.registry = registry
    }
    #else
    /// iOS has no local machine: every project names its machine endpoint and a
    /// `DaemonProjectHost` on it.
    init(
        projectRoot: String, service: any WorkspaceService, registry: WorkspaceRegistry?, channel: ChannelState? = nil,
        endpoint: DaemonEndpoint, host: any ProjectHost
    ) {
        let key = ProjectKey(machineId: endpoint.machineId, root: projectRoot)
        let client = endpoint.makeClient()
        self.projectRoot = projectRoot
        self.key = key
        self.endpoint = endpoint
        self.client = client
        self.host = host
        self.git = host.isLocal ? GitService.shared : GitService(host: host)
        self.remoteLayoutSaver = endpoint.isLocal ? nil : RemoteLayoutSaver(projectRoot: projectRoot, host: host)
        self.channel =
            channel
            ?? (endpoint.isLocal
                ? ChannelState(projectRoot: projectRoot)
                : ChannelState(projectRoot: projectRoot, defaultsScope: key.id) { try await client.send($0) })
        self.service = service
        self.registry = registry
    }
    #endif

    // MARK: - Lifecycle

    func startWatching() {
        #if os(macOS)
        guard endpoint.isLocal else {
            startWatchingRemote()
            return
        }
        let root = projectRoot
        let svc = service
        let client = self.client

        openTask?.cancel()
        refreshTask?.cancel()
        manifestWatcher?.stop()
        manifestWatcher = nil
        layoutWatcher?.stop()
        layoutWatcher = nil
        hasLoadedWorktrees = false

        openTask = Task { [weak self] in
            // Retry with backoff: right after an update relaunch the previous
            // daemon can still be draining its agents and holding the daemon
            // lock, so the first start attempt fails. Giving up on it left the
            // app daemonless until the user quit and relaunched.
            // Bounded by wall-clock time, not attempts: one attempt against a
            // daemon that never answers can itself take ~20s of health probes.
            let deadline = ContinuousClock.now + Self.daemonConnectBudget
            var attempt = 0
            while true {
                do {
                    try await DaemonLifecycle.ensureDaemon()
                    let response = try await client.send(.initProject(projectRoot: root))
                    if case .error(let code, let message) = response {
                        // At its connection limit: transient, so back off and retry.
                        if code == "BUSY" { throw DaemonClientError.busy }
                        self?.appState?.daemonError = message
                        return
                    }
                    break
                } catch is CancellationError {
                    return
                } catch {
                    attempt += 1
                    guard ContinuousClock.now < deadline else {
                        self?.appState?.daemonError = error.localizedDescription
                        return
                    }
                    print("[Daemon] connect attempt \(attempt) failed: \(error); retrying")
                    let delay = UInt64(min(attempt, 4)) * 1_000_000_000
                    do { try await Task.sleep(nanoseconds: delay) } catch { return }
                }
            }

            guard let self, !Task.isCancelled else { return }

            let manifestPath = svc.manifestPath(projectRoot: root)
            self.manifestWatcher = ManifestWatcher(path: manifestPath) { [weak self] in
                self?.refresh()
            }
            let projectId = self.projectId
            self.layoutWatcher = ManifestWatcher(
                path: WorkspacePersistence.filePath(projectRoot: root)
            ) { [weak self] in
                self?.registry?.adoptLocalLayoutIfChanged(projectRoot: projectId)
            }

            self.channel.startBackground()
            self.startGridSubscription()
            self.startStatusSubscription()
            // Resume from the snapshot taken after init, not after a fixed delay: a
            // slow first load would otherwise resume against the agents from before
            // init (or a daemon restart) and leave the newly suspended ones paused.
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard !Task.isCancelled else { return }
                self.apply(rootAgents: snapshot.rootAgents, worktrees: snapshot.worktrees)
                self.hasLoadedWorktrees = true
            } catch is CancellationError {
                return
            } catch {
                self.appState?.daemonError = error.localizedDescription
                return
            }
            self.resumeSuspendedAgents()
        }
        #else
        startWatchingRemote()
        #endif
    }

    /// A remote daemon is never started or initialized from here (MACH-003): the machine's
    /// connection owns reachability, and the project is already registered there. Load the
    /// layout through the host, then follow status and grid like a local project.
    private func startWatchingRemote() {
        let root = projectRoot
        let projectId = self.projectId
        let svc = service
        openTask?.cancel()
        refreshTask?.cancel()
        resubscribeTask?.cancel()
        #if os(macOS)
        manifestWatcher?.stop()
        manifestWatcher = nil
        layoutWatcher?.stop()
        layoutWatcher = nil
        #endif
        hasLoadedWorktrees = false

        startRemoteLayoutRestore()

        openTask = Task { [weak self] in
            guard let self, !Task.isCancelled else { return }
            self.channel.startBackground()
            self.startGridSubscription()
            self.startStatusSubscription()
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard !Task.isCancelled else { return }
                self.apply(rootAgents: snapshot.rootAgents, worktrees: snapshot.worktrees)
                self.hasLoadedWorktrees = true
            } catch is CancellationError {
                return
            } catch {
                // The machine row reports reachability; the status stream retries on its own.
                print("[Remote \(projectId)] status failed: \(error)")
            }
        }
    }

    /// (Re)start reading the remote layout. Retry Layout calls it to try now instead of
    /// waiting out the backoff.
    func retryRemoteLayout() {
        guard isRemote, !isUnavailable else { return }
        startRemoteLayoutRestore()
    }

    private func startRemoteLayoutRestore() {
        remoteLayoutTask?.cancel()
        remoteLayoutTask = nil
        guard let registry, let saver = remoteLayoutSaver else { return }
        let root = projectRoot
        let projectId = self.projectId
        let host = self.host
        remoteLayoutTask = Task {
            await Self.restoreRemoteLayout(
                projectRoot: root, projectId: projectId, host: host, registry: registry, saver: saver)
        }
    }

    /// Read a remote layout through its host until it arrives, then adopt it and route the
    /// project's saves to `saver`. A failed read (anything but a missing file) surfaces as a
    /// layout error and is retried with backoff; the saver is not registered meanwhile, so no
    /// empty layout can be saved over the real one.
    static func restoreRemoteLayout(
        projectRoot: String, projectId: String, host: any ProjectHost, registry: WorkspaceRegistry,
        saver: RemoteLayoutSaver, retryDelay: @escaping (Int) -> Duration = { .seconds(min(2 * $0, 30)) }
    ) async {
        var attempt = 0
        while !Task.isCancelled {
            do {
                let layout = try await HostIO.run {
                    try WorkspacePersistence.loadRemote(projectRoot: projectRoot, projectId: projectId, host: host)
                }
                guard !Task.isCancelled else { return }
                saver.loaded(modifiedMs: layout.modifiedMs)
                let target = WeakRegistryRef(registry)
                // Weak: the saver keeps this handler, which must not keep the saver.
                let weakSaver = WeakSaverRef(saver)
                saver.onConflict { document, modified in
                    Task { @MainActor in
                        target.value?.adoptExternalLayout(
                            projectId, workspaces: WorkspacePersistence.workspaces(from: document, projectId: projectId))
                        weakSaver.value?.loaded(modifiedMs: modified)
                    }
                }
                registry.registerRemoteProject(projectId, restored: layout.workspaces) { workspaces, active in
                    saver.save(workspaces, activeWorkspaceId: active)
                }
                return
            } catch {
                guard !Task.isCancelled else { return }
                attempt += 1
                registry.setRemoteLayoutError(projectId, error.localizedDescription)
                do { try await Task.sleep(for: retryDelay(attempt)) } catch { return }
            }
        }
    }

    func stopWatching() {
        invalidateSignals()
        channel.shutdown()
        openTask?.cancel()
        remoteLayoutTask?.cancel()
        remoteLayoutTask = nil
        resubscribeTask?.cancel()
        resubscribeTask = nil
        refreshTask?.cancel()
        gridSubscriptionTask?.cancel()
        statusSubscriptionTask?.cancel()
        let currentGrid = gridSubscription
        let currentStatus = statusSubscription
        gridSubscription = nil
        statusSubscription = nil
        Task { await currentGrid?.stop() }
        Task { await currentStatus?.stop() }
        #if os(macOS)
        manifestWatcher?.stop()
        manifestWatcher = nil
        layoutWatcher?.stop()
        layoutWatcher = nil
        #endif
    }

    #if os(iOS)
    /// Re-open the daemon streams and re-read status after the app was in the background,
    /// where iOS suspends it and its SSH streams drop or stall. Nothing to do for a project
    /// that is not being watched.
    func resumeStreams() {
        guard !isUnavailable, statusSubscription != nil || gridSubscription != nil else { return }
        resubscribeTask?.cancel()
        resubscribeTask = nil
        startGridSubscription()
        startStatusSubscription()
        refresh()
    }
    #endif

    // MARK: - Data

    private func invalidateSignals() {
        rootAgents = rootAgents.map { var agent = $0; agent.signal = nil; return agent }
        worktrees = worktrees.map { worktree in
            var copy = worktree
            copy.agents = copy.agents.map { var agent = $0; agent.signal = nil; return agent }
            return copy
        }
    }

    func refresh() {
        let root = projectRoot
        let svc = service
        #if os(macOS)
        layoutWatcher?.retry()  // the layout file may not have existed when watching began
        #endif

        refreshTask?.cancel()
        refreshTask = Task { [weak self] in
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard let self, !Task.isCancelled else { return }

                self.apply(rootAgents: snapshot.rootAgents, worktrees: snapshot.worktrees)
                self.hasLoadedWorktrees = true
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
                if self.appState?.projectState(forId: self.projectId) != nil {
                    self.appState?.activeProjectRoot = self.projectId
                }
                self.appState?.pendingSelectAgentId = agentId
            }
        }
    }

    /// Spawn an agent into a specific tab.
    ///
    /// The tab is reserved before the request goes out, so whichever arrives first — the
    /// manifest write or the spawn response — the agent lands in this tab and never
    /// surfaces as a workspace of its own. The request also names the tab, which the daemon
    /// records on the agent, so any other Mac showing this project places it there too.
    func spawnAgentForSurface(
        agent: String, prompt: String, workspaceId: String, surfaceId: Int,
        name: String? = nil, command: String? = nil
    ) {
        guard let registry, let workspace = registry.workspace(id: workspaceId) else { return }
        let root = projectId
        let spawnWorktree = workspace.container.worktreeId

        registry.reserveSurface(projectRoot: root, workspaceId: workspaceId, surfaceId: surfaceId)
        let request = DaemonRequest.spawn(
            projectRoot: projectRoot, prompt: prompt, agent: agent,
            name: name, root: spawnWorktree == nil, worktree: spawnWorktree, command: command,
            placement: SpawnPlacement(workspace: workspaceId, surface: surfaceId)
        )

        guard let saver = remoteLayoutSaver else {
            sendSurfaceSpawn(request, workspaceId: workspaceId, surfaceId: surfaceId)
            return
        }
        // The machine's app (if one has this project open) must see the new tab before the
        // agent appears, or it has nowhere to put it: write the layout first.
        registry.saveNow(projectRoot: root)
        Task {
            _ = await HostIO.value { saver.flush(until: .now() + 5) }
            self.sendSurfaceSpawn(request, workspaceId: workspaceId, surfaceId: surfaceId)
        }
    }

    private func sendSurfaceSpawn(_ request: DaemonRequest, workspaceId: String, surfaceId: Int) {
        let root = projectId
        let registry = self.registry
        sendDaemonRequest(
            request,
            onFailure: { [weak registry] in
                registry?.releaseReservation(projectRoot: root, workspaceId: workspaceId, surfaceId: surfaceId)
            }
        ) { [weak registry] response in
            if case .spawnResult(_, let agentId, _) = response {
                registry?.fulfillReservation(
                    projectRoot: root, workspaceId: workspaceId, surfaceId: surfaceId, agentId: agentId)
            } else {
                registry?.releaseReservation(projectRoot: root, workspaceId: workspaceId, surfaceId: surfaceId)
            }
        }
    }

    func createWorktree(name: String?) {
        sendDaemonRequest(.createWorktree(projectRoot: projectRoot, name: name)) { response in
            if case .createWorktreeResult(let worktreeId) = response {
                if self.appState?.projectState(forId: self.projectId) != nil {
                    self.appState?.activeProjectRoot = self.projectId
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
        case .spawnAgentDef(let def, let prompt, let name):
            createAgent(
                agent: def.agentType, prompt: prompt ?? def.inlinePrompt ?? "",
                name: name, selection: selection, command: def.command)
        case .runSwarm(let def):
            let root = projectRoot
            if isRemote {
                // The swarm runs on this project's machine, resolved by its daemon.
                sendDaemonCommand(.runSwarm(projectRoot: root, swarmName: def.name, vars: [:]))
            } else {
                Task { await hub.runSwarm(projectRoot: root, name: def.name) }
            }
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
        registry?.reconcile(projectRoot: projectId, rootAgents: rootAgents, worktrees: worktrees)
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
            let client = self.client
            Task {
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
        registry?.reconcile(projectRoot: projectId, rootAgents: rootAgents, worktrees: worktrees)
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
        let client = self.client
        Task {
            do {
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
        let client = self.client
        Task {
            do {
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
        let sub = DaemonStatusSubscription(projectRoot: projectRoot, client: client)
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
                    guard let self else { return }
                    self.invalidateSignals()
                    if self.isRemote {
                        // Never start a remote daemon; resubscribe once it answers again.
                        self.resubscribeTask?.cancel()
                        self.resubscribeTask = Task { [weak self] in
                            do { try await Task.sleep(for: .seconds(5)) } catch { return }
                            #if os(iOS)
                            await AppActivity.shared.untilForeground()
                            if Task.isCancelled { return }
                            #endif
                            self?.startWatching()
                        }
                    } else {
                        self.startWatching()
                    }
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
        let sub = DaemonGridSubscription(projectRoot: projectRoot, projectId: projectId, registry: registry, client: client)
        gridSubscription = sub
        gridSubscriptionTask = Task { await sub.start() }
    }
}

/// Lets a save callback running off the main thread reach the registry without retaining it.
nonisolated final class WeakRegistryRef: @unchecked Sendable {
    @MainActor weak var value: WorkspaceRegistry?

    @MainActor init(_ value: WorkspaceRegistry) {
        self.value = value
    }
}

/// Lets a saver's own conflict handler reach it without retaining it.
nonisolated final class WeakSaverRef: @unchecked Sendable {
    weak var value: RemoteLayoutSaver?

    init(_ value: RemoteLayoutSaver) {
        self.value = value
    }
}
