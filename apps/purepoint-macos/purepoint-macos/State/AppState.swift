import Foundation
import Observation

/// Multi-project container. Holds an array of ProjectState — one per open project.
/// Provides cross-project queries and manages global daemon lifecycle.
@Observable
@MainActor
final class AppState {
    var projects: [ProjectState] = []
    var activeProjectRoot: String?
    var activeSidebarSelection: SidebarSelection?
    var daemonError: String?
    var showSettings = false
    /// Agent ID a spawn just returned. Resolved to its workspace once the manifest lands.
    var pendingSelectAgentId: String?
    var pendingSelectWorktreeId: String?
    /// Workspace to select once the sidebar has it — used to restore the last session.
    var pendingSelectWorkspaceId: String?
    var pendingFocusAgentId: String?

    var pointGuardShellId: String?

    var agentsHubState = AgentsHubState()
    var agentConfigState = AgentConfigState()
    var scheduleState = ScheduleState()
    var triggersState = TriggersState()

    weak var registry: WorkspaceRegistry?

    /// The agent in the focused pane of the on-screen workspace, or nil.
    var focusedAgentId: String? { registry?.activeWorkspace?.focusedAgentId }

    @ObservationIgnored private let service: any WorkspaceService
    @ObservationIgnored private var binaryWatcher: ManifestWatcher?

    private static let openProjectsKey = "PurePointOpenProjects"

    init(service: any WorkspaceService = DaemonWorkspaceService()) {
        self.service = service
    }

    var isLoaded: Bool { !projects.isEmpty }

    // MARK: - Project Management

    func openProject(_ root: String) {
        guard !projects.contains(where: { $0.projectRoot == root }) else { return }

        let project = ProjectState(projectRoot: root, service: service, registry: registry)
        project.appState = self
        projects.append(project)
        project.startWatching()
        initializeActiveProjectIfNeeded(root: root)

        // Watch daemon binary for changes (shared — only start once)
        if binaryWatcher == nil, let binPath = DaemonLifecycle.findBinary() {
            binaryWatcher = ManifestWatcher(path: binPath) { [weak self] in
                self?.restartDaemonAndRefresh()
            }
        }

        persistOpenProjects()
    }

    func closeProject(_ root: String) {
        guard let index = projects.firstIndex(where: { $0.projectRoot == root }) else { return }
        projects[index].stopWatching()
        projects.remove(at: index)
        registry?.forgetProject(root)
        persistOpenProjects()
    }

    func restoreProjects() {
        guard let paths = UserDefaults.standard.stringArray(forKey: Self.openProjectsKey) else { return }
        for path in paths {
            guard FileManager.default.fileExists(atPath: path) else { continue }
            openProject(path)
        }
        // Workspace layouts load lazily in WorkspaceRegistry.reconcile, on the first
        // manifest read for each project.
    }

    // MARK: - Active Project Routing

    /// Update `activeProjectRoot` and `activeSidebarSelection` based on what the user selected
    /// in the sidebar. Terminal, agent, worktree, and project selections resolve to their owning
    /// project; nav and nil selections preserve the last-known project.
    func updateActiveProject(for selection: SidebarSelection?) {
        activeSidebarSelection = selection

        switch selection {
        case .workspace(let id):
            if let root = registry?.projectRoot(forWorkspace: id) {
                activeProjectRoot = root
            }
        case .worktree(let id):
            if let root = projectState(forWorktreeId: id)?.projectRoot {
                activeProjectRoot = root
            }
        case .project(let root):
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

    // MARK: - Cross-Project Queries

    func agent(byId id: String) -> AgentModel? {
        for project in projects {
            if let agent = project.agent(byId: id) { return agent }
        }
        return nil
    }

    func projectState(forAgentId agentId: String) -> ProjectState? {
        projects.first { $0.agent(byId: agentId) != nil }
    }

    func projectState(forWorktreeId worktreeId: String) -> ProjectState? {
        projects.first { $0.worktrees.contains { $0.id == worktreeId } }
    }

    func projectState(forRoot root: String) -> ProjectState? {
        projects.first { $0.projectRoot == root }
    }

    func agentId(forSessionId sessionId: String) -> String? {
        for project in projects {
            if let agent = project.allAgents.first(where: { $0.sessionId == sessionId }) {
                return agent.id
            }
        }
        return nil
    }

    func worktreeId(forPath path: String) -> String? {
        let normalizedPath = URL(fileURLWithPath: path).standardizedFileURL.path
        for project in projects {
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
        persistSelectedAgent()
        registry?.saveAll()

        for project in projects {
            project.stopWatching()
        }
        binaryWatcher?.stop()
        binaryWatcher = nil

        // Block until daemon RPCs complete (macOS terminates shortly after willTerminate)
        let projectRoots = projects.map(\.projectRoot)
        let semaphore = DispatchSemaphore(value: 0)
        Task.detached {
            let client = DaemonClient()
            for root in projectRoots {
                _ = try? await client.send(.suspend(projectRoot: root, target: .all))
            }
            // Only shut down a daemon this instance launched — a second app
            // instance attached to a shared daemon must not kill it on quit.
            if await DaemonLifecycle.didLaunchDaemon() {
                _ = try? await client.send(.shutdown)
            }
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
            activeProjectRoot = root
            return pendingId
        }
        if projects.allSatisfy({ registry.workspacesByProject[$0.projectRoot] != nil }) {
            pendingSelectWorkspaceId = nil
        }
        return nil
    }

    // MARK: - Private

    private func persistOpenProjects() {
        let paths = projects.map(\.projectRoot)
        UserDefaults.standard.set(paths, forKey: Self.openProjectsKey)
    }

    private func restartDaemonAndRefresh() {
        Task {
            do {
                try await DaemonLifecycle.restartDaemon()
                for project in projects {
                    project.refresh()
                }
            } catch {
                self.daemonError = error.localizedDescription
            }
        }
    }
}
