import AppKit
import CryptoKit
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
    @ObservationIgnored private var machineRegistryWatcher: MachineRegistryWatcher?

    private static let openProjectsKey = "PurePointOpenProjects"

    init(
        service: any WorkspaceService = DaemonWorkspaceService(),
        startProject: @escaping @MainActor (ProjectState) -> Void = { $0.startWatching() }
    ) {
        self.service = service
        self.startProject = startProject
    }

    var isLoaded: Bool { !projects.isEmpty }

    // Freshness advances even when no new status packet arrives.
    var signalClock = Date()
    @ObservationIgnored private lazy var signalNotifications = AgentSignalNotifications()

    private func signalIdentity(projectID: String, agentID: String) -> String {
        let data = (try? JSONEncoder().encode([projectID, agentID])) ?? Data()
        return SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
    }

    func updateAgentSignals() {
        signalClock = Date()
        var foreground: String?
        let agents = managedProjects.flatMap { project in
            project.allAgents.map { agent -> AgentSignalAlertPolicy.Agent in
                let identity = self.signalIdentity(projectID: project.projectId, agentID: agent.id)
                if NSApplication.shared.isActive, NSApplication.shared.keyWindow != nil, activeProjectRoot == project.projectId,
                    focusedAgentId == agent.id { foreground = identity }
                return .init(identity: identity, launch: agent.signal?.identity.launch_id ?? "",
                    presentation: agent.signal?.presentation(agentID: agent.id, sessionID: agent.sessionId,
                        available: !project.isUnavailable && agent.status.isAlive && !agent.suspended, now: signalClock) ?? .unknown,
                    qualificationKey: agent.signal?.qualificationKey,
                    macOSActionable: agent.signal?.pending_attention.contains {
                        $0.event_id == agent.signal?.presentation(agentID: agent.id, sessionID: agent.sessionId,
                            available: !project.isUnavailable && agent.status.isAlive && !agent.suspended, now: signalClock).eventID && $0.actionability?.macos == true
                    } ?? false)
            }
        }
        signalNotifications.open = { [weak self] destination in
            guard let self,
                let match = self.managedProjects.lazy.compactMap({ project -> (ProjectState, AgentModel)? in
                    guard let agent = project.allAgents.first(where: {
                        self.signalIdentity(projectID: project.projectId, agentID: $0.id) == destination.identity
                    }) else { return nil }
                    return (project, agent)
                }).first else { return }
            let (project, agent) = match
            guard
                agent.signal?.identity.launch_id == destination.launch,
                agent.signal?.presentation(agentID: agent.id, sessionID: agent.sessionId,
                    available: !project.isUnavailable && agent.status.isAlive && !agent.suspended).eventID == destination.presentation.eventID
            else { return }
            Task { [weak self, weak project] in
                guard let self, let project,
                    let response = try? await project.client.send(.status(projectRoot: project.projectRoot)),
                    case .statusReport(let worktrees, let roots) = response else { return }
                let current = roots.first(where: { $0.id == agent.id })?.signal
                    ?? worktrees.lazy.compactMap { $0.agents[agent.id]?.signal }.first
                guard current?.identity.launch_id == destination.launch,
                    current?.presentation(agentID: agent.id, sessionID: agent.sessionId).eventID == destination.presentation.eventID,
                    self.managedProjects.contains(where: { $0 === project }), !project.isUnavailable else { return }
                // Navigation uses existing workspace/tab routing, never an input command.
                project.refresh()
                self.activeProjectRoot = project.projectId
                self.pendingSelectAgentId = agent.id
                NSApplication.shared.activate(ignoringOtherApps: true)
            }
        }
        signalNotifications.update(agents, foregroundIdentity: foreground)
    }

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

    /// Whether a path is too broad to open as a project: `/`, `$HOME`, or an ancestor
    /// of `$HOME`. Mirrors the daemon's `is_too_broad_project_root` validation so the
    /// app doesn't attempt to open roots the daemon will reject.
    func isTooBroadProjectRoot(_ path: String) -> Bool {
        let url = URL(fileURLWithPath: path)
        let canonical = url.resolvingSymlinksInPath().standardizedFileURL.path
        // Filesystem root
        if canonical == "/" { return true }
        // $HOME or an ancestor of $HOME
        if let home = ProcessInfo.processInfo.environment["HOME"] {
            let homeCanonical = URL(fileURLWithPath: home)
                .resolvingSymlinksInPath().standardizedFileURL.path
            if homeCanonical.hasPrefix(canonical + "/") || homeCanonical == canonical {
                return true
            }
        }
        return false
    }

    func restoreProjects(projectRoot: String? = nil) {
        let paths = projectRoot.map { [$0] }
            ?? UserDefaults.standard.stringArray(forKey: Self.openProjectsKey) ?? []
        // Opening projects persists current state, so snapshot both saved lists first.
        let savedPointGuardFolders = UserDefaults.standard.stringArray(forKey: "PurePointPointGuardFolders") ?? []
        pointGuardFolders.formUnion(savedPointGuardFolders)
        for path in paths {
            guard projectRoot != nil || FileManager.default.fileExists(atPath: path) else { continue }
            guard !isTooBroadProjectRoot(path) else { continue }
            openProject(path)
        }
        for root in savedPointGuardFolders {
            guard FileManager.default.fileExists(atPath: root) else { continue }
            guard !isTooBroadProjectRoot(root) else { continue }
            ensurePointGuardProject(root: root)
        }
        // Workspace layouts load lazily in WorkspaceRegistry.reconcile, on the first
        // manifest read for each project.
    }

    // MARK: - Machines

    /// Read `~/.pu/machines.json` and connect every machine the user has not disconnected.
    /// Starts a file watcher so later CLI or other-instance edits show up without a restart.
    func loadMachines() {
        FileIOService.removeLocalCopies()
        reloadMachines()
        startMachineRegistryWatcher()
    }

    /// Re-read `~/.pu/machines.json` and diff against the current machines: connect any new
    /// ones, update existing ones whose name or SSH host changed, and remove any that are no
    /// longer registered. Called at startup and by the registry file watcher.
    func reloadMachines() {
        let registered: [Machine]
        do {
            registered = try MachineRegistryStore.load()
        } catch {
            daemonError = "Could not read machines from ~/.pu/machines.json: \(error.localizedDescription)"
            return
        }
        for machine in registered {
            if let existing = machines.first(where: { $0.machine.id == machine.id }) {
                existing.update(from: machine)
            } else {
                let state = MachineState(machine: machine, appState: self, registry: registry)
                machines.append(state)
                if !MachineState.isUserDisconnected(machine.id) { state.connect() }
            }
        }
        // Remove machines no longer in the registry (CLI remove or another instance).
        // KeepUnsaved preserves projects with unsaved tabs as unavailable.
        let registeredIds = Set(registered.map(\.id))
        let removed = machines.filter { !registeredIds.contains($0.machine.id) }
        for state in removed {
            state.disconnect(keepUnsaved: true)
            machines.removeAll { $0 === state }
            if let active = activeProjectRoot, ProjectKey(id: active).machineId == state.machine.id {
                activeProjectRoot = projects.first?.projectRoot
            }
        }
    }

    /// Watch `~/.pu/machines.json` for changes from the CLI or another app instance.
    private func startMachineRegistryWatcher() {
        guard machineRegistryWatcher == nil else { return }
        machineRegistryWatcher = MachineRegistryWatcher { [weak self] in
            self?.reloadMachines()
        }
    }

    /// The registry update takes a file lock shared with `pu machine`, so it runs off the
    /// main thread.
    @discardableResult
    func addMachine(name: String, sshHost: String) async throws -> MachineState {
        let machine = try await Task.detached {
            try MachineRegistryStore.update { machines in
                try MachineRegistryStore.add(name: name, sshHost: sshHost, to: &machines)
            }
        }.value
        // The watcher may have reloaded between the registry write and this resume, adding
        // this machine already; reuse that state rather than creating a duplicate.
        if let existing = machines.first(where: { $0.machine.id == machine.id }) {
            existing.update(from: machine)
            if !MachineState.isUserDisconnected(machine.id) { existing.connect() }
            return existing
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
        let name = state.machine.name
        Task {
            do {
                try await Task.detached {
                    try MachineRegistryStore.update { machines in machines.removeAll { $0.id == id } }
                }.value
            } catch {
                daemonError = "Could not remove \(name) from ~/.pu/machines.json: \(error.localizedDescription)"
            }
        }
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
        // Broad roots have no ProjectState owner for routing; skip daemon registration.
        guard !isTooBroadProjectRoot(root) else { return }
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
        guard !isTooBroadProjectRoot(root) else {
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
        machineRegistryWatcher?.stop()
        machineRegistryWatcher = nil
        FileIOService.removeLocalCopies()

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
        if let project = projectState(forWorkspace: workspaceId, registry: registry) { return project.host }
        // A remote workspace whose project is not available must never fall back to this
        // Mac's disk with the remote path.
        if let projectId = registry.projectRoot(forWorkspace: workspaceId), ProjectKey(id: projectId).isRemote {
            return UnavailableProjectHost()
        }
        return LocalProjectHost.shared
    }

    /// The daemon owning an agent, so terminals attach to the right machine.
    func client(forAgentId agentId: String) -> DaemonClient? {
        projectState(forAgentId: agentId)?.client
    }
}
