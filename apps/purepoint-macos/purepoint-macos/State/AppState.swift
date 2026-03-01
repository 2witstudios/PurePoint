import Foundation
import Observation

@Observable
final class AppState {
    var worktrees: [WorktreeModel] = []
    var rootAgents: [AgentModel] = []
    var projectRoot: String = ""
    var sessionName: String = ""
    var projectName: String { URL(fileURLWithPath: projectRoot).lastPathComponent }
    var isLoaded: Bool { !projectRoot.isEmpty }

    private let service: any WorkspaceService
    private var manifestWatcher: ManifestWatcher?

    init(service: any WorkspaceService = TmuxWorkspaceService()) {
        self.service = service
    }

    func openProject(_ root: String) {
        projectRoot = root

        // Initial load
        refresh()

        // Watch for manifest changes
        let manifestPath = service.manifestPath(projectRoot: root)
        manifestWatcher?.stop()
        manifestWatcher = ManifestWatcher(path: manifestPath) { [weak self] in
            self?.refresh()
        }
    }

    func refresh() {
        guard !projectRoot.isEmpty else { return }
        let root = projectRoot
        let svc = service

        DispatchQueue.global(qos: .utility).async { [weak self] in
            guard let snapshot = try? svc.loadWorkspace(projectRoot: root) else { return }
            DispatchQueue.main.async {
                guard let self, self.projectRoot == root else { return }
                self.worktrees = snapshot.worktrees
                self.rootAgents = snapshot.rootAgents
                self.sessionName = snapshot.sessionName
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
