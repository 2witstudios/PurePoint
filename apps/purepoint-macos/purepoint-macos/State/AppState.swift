import Foundation
import Observation

/// Multi-project container. Holds an array of ProjectState — one per open project.
/// Provides cross-project queries and manages global daemon lifecycle.
@Observable
@MainActor
final class AppState {
    var projects: [ProjectState] = []
    var selectedAgentId: String?
    var activeProjectRoot: String?
    var activeSidebarSelection: SidebarSelection?
    var daemonError: String?
    var showSettings = false
    var pendingSelectAgentId: String?
    var pendingFocusAgentId: String?

    var agentsHubState = AgentsHubState()

    weak var gridState: GridState?

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

        let project = ProjectState(projectRoot: root, service: service, gridState: gridState)
        project.appState = self
        projects.append(project)
        project.startWatching()

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
        persistOpenProjects()
    }

    func restoreProjects() {
        guard let paths = UserDefaults.standard.stringArray(forKey: Self.openProjectsKey) else { return }
        for path in paths {
            guard FileManager.default.fileExists(atPath: path) else { continue }
            openProject(path)
        }
        // Grid layout is restored on-demand when user clicks the owner agent
        // (via ContentView.onChange → gridState.restoreIfOwner)
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
            _ = try? await client.send(.shutdown)
            semaphore.signal()
        }
        // Timeout after 5s — don't hang indefinitely if daemon is unresponsive
        _ = semaphore.wait(timeout: .now() + 5.0)
    }

    // MARK: - Selection Persistence

    private static let selectedAgentKey = "PurePointSelectedAgentId"

    private func persistSelectedAgent() {
        UserDefaults.standard.set(selectedAgentId, forKey: Self.selectedAgentKey)
    }

    func restoreSelectedAgent() {
        if let savedId = UserDefaults.standard.string(forKey: Self.selectedAgentKey),
           agent(byId: savedId) != nil {
            selectedAgentId = savedId
        }
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
