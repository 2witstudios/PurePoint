import Foundation
import Observation

/// Multi-project container. Holds an array of ProjectState — one per open project.
/// Provides cross-project queries and manages global daemon lifecycle.
@Observable
@MainActor
final class AppState {
    /// Local projects, in sidebar order. Remote projects belong to their machine.
    var projects: [ProjectState] = []
    /// Machines reachable over SSH, each with the projects its daemon has registered.
    var machines: [MachineState] = []
    /// Folder-backed daemon state for Point Guard, kept out of the project sidebar.
    private var pointGuardProjects: [String: ProjectState] = [:]
    /// Ownership survives project promotion and lazy workspace restoration.
    private var pointGuardFolders = Set<String>()
    /// Unknown until Pi reports its actual configured folder.
    var pointGuardRoot: String?

    private var localManagedProjects: [ProjectState] {
        projects + pointGuardProjects.values.filter { hidden in
            !projects.contains { $0.projectRoot == hidden.projectRoot }
        }
    }

    /// Every project the app follows: local, Point Guard folders, and each machine's.
    private var managedProjects: [ProjectState] {
        localManagedProjects + remoteProjects
    }

    var remoteProjects: [ProjectState] { machines.flatMap(\.projects) }

    /// The active project's key (`ProjectKey.id`) — its root for local projects.
    var activeProjectRoot: String?

    /// The active project when it is local. Library screens (agent definitions, schedules,
    /// triggers, agent settings) talk to the local daemon only.
    var activeLocalProjectRoot: String? {
        guard let activeProjectRoot, !ProjectKey(id: activeProjectRoot).isRemote else {
            return projects.first?.projectRoot
        }
        return activeProjectRoot
    }
    // ContentView starts on Dashboard before its first selection-change callback.
    var activeSidebarSelection: SidebarSelection? = .nav(.dashboard)
    var daemonError: String?
    var showSettings = false
    /// Agent ID a spawn just returned. Resolved to its workspace once the manifest lands.
    var pendingSelectAgentId: String?
    var pendingSelectWorktreeId: String?
    /// Workspace to select once the sidebar has it — used to restore the last session.
    var pendingSelectWorkspaceId: String?
    var pendingFocusAgentId: String?

    @ObservationIgnored lazy var pointGuardChat = PiChatModel()
    @ObservationIgnored lazy var pointGuardService = PointGuardServiceModel()

    var agentsHubState = AgentsHubState()
    var agentConfigState = AgentConfigState()
    var scheduleState = ScheduleState()
    var triggersState = TriggersState()

    weak var registry: WorkspaceRegistry?

    /// The agent in the focused pane of the on-screen workspace, or nil.
    var focusedAgentId: String? { registry?.activeWorkspace?.focusedAgentId }

    @ObservationIgnored private let service: any WorkspaceService
    @ObservationIgnored private let startProject: @MainActor (ProjectState) -> Void
    @ObservationIgnored private var binaryWatcher: ManifestWatcher?

    private static let openProjectsKey = "PurePointOpenProjects"

    init(
        service: any WorkspaceService = DaemonWorkspaceService(),
        startProject: @escaping @MainActor (ProjectState) -> Void = { $0.startWatching() }
    ) {
        self.service = service
        self.startProject = startProject
    }

    var isLoaded: Bool { !projects.isEmpty }

    // MARK: - Project Management

    func openProject(_ root: String) {
        guard !projects.contains(where: { $0.projectRoot == root }) else { return }

        let project = pointGuardProjects.removeValue(forKey: root)
            ?? ProjectState(projectRoot: root, service: service, registry: registry)
        project.appState = self
        projects.append(project)
        startProject(project)
        initializeActiveProjectIfNeeded(root: root)

        startBinaryWatcherIfNeeded()

        persistOpenProjects()
    }

    /// Returns true when the root is no longer managed and its local files can be discarded.
    @discardableResult
    func closeProject(_ root: String) -> Bool {
        let key = ProjectKey(id: root)
        if key.isRemote {
            // Removing a remote project only hides it here; nothing on the machine changes.
            guard let machine = machines.first(where: { $0.machine.id == key.machineId }) else { return false }
            machine.hideProject(root: key.root)
            if activeProjectRoot == root { activeProjectRoot = projects.first?.projectRoot }
            return false
        }
        guard let index = projects.firstIndex(where: { $0.projectRoot == root }) else { return false }
        let project = projects.remove(at: index)
        if root == pointGuardRoot || pointGuardFolders.contains(root)
            || registry?.workspacesByProject[root]?.contains(where: \.isPointGuard) == true
        {
            pointGuardFolders.insert(root)
            pointGuardProjects[root] = project
        } else {
            project.stopWatching()
            registry?.forgetProject(root)
        }
        if activeProjectRoot == root {
            activeProjectRoot = projects.first?.projectRoot
        }
        persistOpenProjects()
        return projectState(forRoot: root) == nil
    }

    func restoreProjects(projectRoot: String? = nil) {
        let paths = projectRoot.map { [$0] }
            ?? UserDefaults.standard.stringArray(forKey: Self.openProjectsKey) ?? []
        // Opening projects persists current state, so snapshot both saved lists first.
        let savedPointGuardFolders = UserDefaults.standard.stringArray(forKey: "PurePointPointGuardFolders") ?? []
        pointGuardFolders.formUnion(savedPointGuardFolders)
        for path in paths {
            guard projectRoot != nil || FileManager.default.fileExists(atPath: path) else { continue }
            openProject(path)
        }
        for root in savedPointGuardFolders {
            guard FileManager.default.fileExists(atPath: root) else { continue }
            ensurePointGuardProject(root: root)
        }
        // Workspace layouts load lazily in WorkspaceRegistry.reconcile, on the first
        // manifest read for each project.
    }

    // MARK: - Machines

    /// Read `~/.pu/machines.json` and connect every machine the user has not disconnected.
    func loadMachines() {
        let registered = (try? MachineRegistryStore.load()) ?? []
        for machine in registered where !machines.contains(where: { $0.machine.id == machine.id }) {
            let state = MachineState(machine: machine, appState: self, registry: registry)
            machines.append(state)
            if !MachineState.isUserDisconnected(machine.id) { state.connect() }
        }
    }

    @discardableResult
    func addMachine(name: String, sshHost: String) throws -> MachineState {
        let machine = try MachineRegistryStore.update { machines in
            try MachineRegistryStore.add(name: name, sshHost: sshHost, to: &machines)
        }
        let state = MachineState(machine: machine, appState: self, registry: registry)
        machines.append(state)
        state.connect()
        return state
    }

    /// Forget a machine: close its forward and drop it from the shared registry. Its daemon,
    /// projects and agents on the machine are untouched.
    func removeMachine(_ state: MachineState) {
        state.disconnect()
        machines.removeAll { $0 === state }
        let id = state.machine.id
        try? MachineRegistryStore.update { machines in machines.removeAll { $0.id == id } }
        if let active = activeProjectRoot, ProjectKey(id: active).machineId == id {
            activeProjectRoot = projects.first?.projectRoot
        }
    }

    // MARK: - Active Project Routing

    /// Update `activeProjectRoot` and `activeSidebarSelection` based on what the user selected
    /// in the sidebar. Terminal, agent, worktree, and project selections resolve to their owning
    /// project; nav and nil selections preserve the last-known project.
    func updateActiveProject(for selection: SidebarSelection?) {
        activeSidebarSelection = selection

        switch selection {
        case .workspace(let id):
            if registry?.workspace(id: id)?.isPointGuard != true,
                let root = registry?.projectRoot(forWorkspace: id) {
                activeProjectRoot = root
            }
        case .worktree(let id):
            if let root = projectState(forWorktreeId: id)?.projectId {
                activeProjectRoot = root
            }
        case .project(let root), .channel(let root):
            activeProjectRoot = root
        case nil, .nav:
            break  // keep last known project
        }
    }

    /// Set `activeProjectRoot` to the given root if no project is active yet.
    /// Called when a project is first opened so Cmd+N works before any sidebar click.
    func initializeActiveProjectIfNeeded(root: String) {
        if activeProjectRoot == nil {
            activeProjectRoot = root
        }
    }

    /// Pi's configured folder is independent of the selected coding project.
    func updatePointGuardRoot(_ cwd: String) {
        pointGuardRoot = cwd.isEmpty ? nil : URL(fileURLWithPath: cwd).standardizedFileURL.path
    }

    private func ensurePointGuardProject(root: String) {
        pointGuardFolders.insert(root)
        UserDefaults.standard.set(Array(pointGuardFolders), forKey: "PurePointPointGuardFolders")
        guard projectState(forRoot: root) == nil else { return }
        let project = ProjectState(projectRoot: root, service: service, registry: registry)
        project.appState = self
        pointGuardProjects[root] = project
        startProject(project)
        startBinaryWatcherIfNeeded()
    }

    @discardableResult
    func activatePointGuardWorkspace() -> String? {
        guard let registry else { return nil }
        guard let root = pointGuardRoot else {
            registry.activeWorkspaceId = nil
            return nil
        }
        ensurePointGuardProject(root: root)
        guard let id = try? registry.ensurePointGuardWorkspace(projectRoot: root) else {
            registry.activeWorkspaceId = nil
            return nil
        }
        registry.activate(workspaceId: id)
        return id
    }

    /// Cmd+N uses the same folder and palette as Point Guard splits.
    func newPointGuardTab() {
        guard let registry, let id = activatePointGuardWorkspace(),
            let workspace = registry.workspace(id: id)
        else { return }
        registry.pendingPaletteSurfaceId = registry.newTab(workspaceId: id, leafId: workspace.focusedLeafId)
    }

    /// Load the pane's own local definitions as well as definitions from visible projects.
    /// The owning root comes first so a Point Guard-only session still queries global definitions.
    func paletteProjectRoots(forWorkspace workspaceId: String) -> [String] {
        let visibleRoots = projects.map(\.projectRoot)
        // Definitions come from the local daemon, so a remote workspace adds no root of its own.
        guard let root = registry?.projectRoot(forWorkspace: workspaceId), !ProjectKey(id: root).isRemote else {
            return visibleRoots
        }
        return [root] + visibleRoots.filter { $0 != root }
    }

    // MARK: - Cross-Project Queries

    func agent(byId id: String) -> AgentModel? {
        for project in managedProjects {
            if let agent = project.agent(byId: id) { return agent }
        }
        return nil
    }

    func projectState(forAgentId agentId: String) -> ProjectState? {
        managedProjects.first { $0.agent(byId: agentId) != nil }
    }

    func projectState(forWorktreeId worktreeId: String) -> ProjectState? {
        managedProjects.first { $0.worktrees.contains { $0.id == worktreeId } }
    }

    /// The project with this key (`ProjectKey.id`; a local project's id is its root).
    func projectState(forId id: String) -> ProjectState? {
        managedProjects.first { $0.projectId == id }
    }

    func projectState(forRoot root: String) -> ProjectState? { projectState(forId: root) }

    func projectState(forWorkspace workspaceId: String, registry: WorkspaceRegistry) -> ProjectState? {
        registry.projectRoot(forWorkspace: workspaceId).flatMap { projectState(forId: $0) }
    }

    func agentId(forSessionId sessionId: String) -> String? {
        for project in managedProjects {
            if let agent = project.allAgents.first(where: { $0.sessionId == sessionId }) {
                return agent.id
            }
        }
        return nil
    }

    func worktreeId(forPath path: String) -> String? {
        let normalizedPath = URL(fileURLWithPath: path).standardizedFileURL.path
        for project in managedProjects {
            if let worktree = project.worktrees.first(where: {
                URL(fileURLWithPath: $0.path).standardizedFileURL.path == normalizedPath
            }) {
                return worktree.id
            }
        }
        return nil
    }

    // MARK: - Lifecycle

    /// Synchronously suspend all agents and shut down daemon.
    /// Must complete before the process exits — uses DispatchSemaphore to block.
    func shutdownWithSuspend() {
        pointGuardService.stopForTermination()
        persistSelectedAgent()
        registry?.saveAll()

        for project in managedProjects {
            project.stopWatching()
        }
        // Remote daemons own their agents (MACH-004): only the SSH forwards close — after
        // the layout saves saveAll just queued have gone through them (2 s at most overall).
        let remoteSaveDeadline = DispatchTime.now() + 2.0
        for machine in machines {
            machine.disconnectForTermination(deadline: remoteSaveDeadline)
        }
        binaryWatcher?.stop()
        binaryWatcher = nil

        // Block until daemon RPCs complete (macOS terminates shortly after willTerminate)
        let projectRoots = localManagedProjects.map(\.projectRoot)
        let semaphore = DispatchSemaphore(value: 0)
        Task.detached {
            let client = DaemonClient()
            // Only an instance that owns the daemon stops anything. A second app
            // instance attached to a shared daemon must leave the owner's agents
            // running: suspending them would strand them, since that daemon stays
            // up and nothing would resume them.
            guard await DaemonLifecycle.didLaunchDaemon() else {
                semaphore.signal()
                return
            }
            for root in projectRoots {
                _ = try? await client.send(.suspend(projectRoot: root, target: .all))
            }
            _ = try? await client.send(.shutdown)
            semaphore.signal()
        }
        // Timeout after 5s — don't hang indefinitely if daemon is unresponsive
        _ = semaphore.wait(timeout: .now() + 5.0)
    }

    // MARK: - Selection Persistence

    private static let activeWorkspaceKey = "PurePointActiveWorkspaceId"

    private func persistSelectedAgent() {
        UserDefaults.standard.set(registry?.activeWorkspaceId, forKey: Self.activeWorkspaceKey)
    }

    /// Remember the workspace that was on screen so the UI can select it once it exists.
    /// Reconcile may not have run for every project yet, so existence is not checked here.
    @discardableResult
    func restoreActiveWorkspace() -> String? {
        guard let savedId = UserDefaults.standard.string(forKey: Self.activeWorkspaceKey) else { return nil }
        pendingSelectWorkspaceId = savedId
        return savedId
    }

    /// The pending workspace to select, once the registry has it. The pending ID is cleared
    /// when it resolves, or when every open project has reconciled and it still names nothing.
    func resolvePendingWorkspaceSelection() -> String? {
        guard let pendingId = pendingSelectWorkspaceId, let registry else { return nil }

        if let root = registry.projectRoot(forWorkspace: pendingId) {
            pendingSelectWorkspaceId = nil
            if registry.workspace(id: pendingId)?.isPointGuard != true {
                activeProjectRoot = root
            }
            return pendingId
        }
        // An unreachable machine may never load its layouts; it must not hold the selection.
        if localManagedProjects.allSatisfy({ registry.workspacesByProject[$0.projectId] != nil }) {
            pendingSelectWorkspaceId = nil
        }
        return nil
    }

    // MARK: - Private

    private func persistOpenProjects() {
        let paths = projects.map(\.projectRoot)
        UserDefaults.standard.set(paths, forKey: Self.openProjectsKey)
        UserDefaults.standard.set(Array(pointGuardFolders), forKey: "PurePointPointGuardFolders")
    }

    private func startBinaryWatcherIfNeeded() {
        guard binaryWatcher == nil, let binPath = DaemonLifecycle.findBinary() else { return }
        binaryWatcher = ManifestWatcher(path: binPath) { [weak self] in
            self?.restartDaemonAndRefresh()
        }
    }

    private func restartDaemonAndRefresh() {
        Task {
            do {
                try await DaemonLifecycle.restartDaemon()
                // Re-run the full open sequence, not just a refresh: init marks the
                // agents the old daemon stopped as suspended, and they get resumed.
                for project in localManagedProjects {
                    startProject(project)
                }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }
}

extension AppState {
    /// The directory a workspace's file panes are rooted at: its worktree, else the project root.
    func fileRoot(forWorkspace workspaceId: String, registry: WorkspaceRegistry) -> String? {
        guard let projectId = registry.projectRoot(forWorkspace: workspaceId) else { return nil }
        let project = projectState(forId: projectId)
        if let worktreeId = registry.workspace(id: workspaceId)?.container.worktreeId,
            let path = project?.worktrees.first(where: { $0.id == worktreeId })?.path
        {
            return path
        }
        return project?.projectRoot ?? ProjectKey(id: projectId).root
    }

    /// Where a workspace's files live: this Mac, or its project's machine.
    func host(forWorkspace workspaceId: String, registry: WorkspaceRegistry) -> any ProjectHost {
        projectState(forWorkspace: workspaceId, registry: registry)?.host ?? LocalProjectHost.shared
    }

    /// The daemon owning an agent, so terminals attach to the right machine.
    func client(forAgentId agentId: String) -> DaemonClient? {
        projectState(forAgentId: agentId)?.client
    }
}
