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
    var daemonError: String?
    var pendingSelectAgentId: String?
    var pendingFocusAgentId: String?

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
        // Restore grid layout from whichever project has a saved layout
        if let gs = gridState {
            for project in projects {
                gs.restore(projectRoot: project.projectRoot)
                if gs.isActive { break }
            }
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

    // MARK: - Lifecycle

    func shutdown() {
        for project in projects {
            project.stopWatching()
        }
        binaryWatcher?.stop()
        binaryWatcher = nil
        Task {
            let client = DaemonClient()
            _ = try? await client.send(.shutdown)
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
