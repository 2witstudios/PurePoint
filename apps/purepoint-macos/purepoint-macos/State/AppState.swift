import Foundation
import Observation

@Observable
final class AppState {
    var worktrees: [WorktreeModel] = []
    var rootAgents: [AgentModel] = []
    var projectRoot: String = ""
    var projectName: String { URL(fileURLWithPath: projectRoot).lastPathComponent }
    var isLoaded: Bool { !projectRoot.isEmpty }
    var daemonError: String?

    private let service: any WorkspaceService
    private var manifestWatcher: ManifestWatcher?

    init(service: any WorkspaceService = DaemonWorkspaceService()) {
        self.service = service
    }

    func openProject(_ root: String) {
        projectRoot = root
        daemonError = nil
        manifestWatcher?.stop()
        manifestWatcher = nil

        // Ensure daemon is running, then start watching and load workspace
        let svc = service
        Task {
            do {
                try await DaemonLifecycle.ensureDaemon()
            } catch {
                self.daemonError = error.localizedDescription
            }

            // Start manifest watcher after daemon is ready (avoids transient errors
            // from querying the daemon before it's healthy).
            let manifestPath = svc.manifestPath(projectRoot: root)
            self.manifestWatcher = ManifestWatcher(path: manifestPath) { [weak self] in
                self?.refresh()
            }

            self.refresh()
        }
    }

    func refresh() {
        guard !projectRoot.isEmpty else { return }
        let root = projectRoot
        let svc = service

        Task {
            do {
                let snapshot = try await svc.loadWorkspace(projectRoot: root)
                guard self.projectRoot == root else { return }
                self.worktrees = snapshot.worktrees
                self.rootAgents = snapshot.rootAgents
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
}
