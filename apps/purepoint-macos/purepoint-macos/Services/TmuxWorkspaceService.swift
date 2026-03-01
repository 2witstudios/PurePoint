import Foundation

nonisolated struct TmuxWorkspaceService: WorkspaceService {
    func manifestPath(projectRoot: String) -> String {
        (projectRoot as NSString)
            .appendingPathComponent(".pu")
            .appending("/manifest.json")
    }

    func loadWorkspace(projectRoot: String) throws -> WorkspaceSnapshot {
        let manifest = try readManifest(projectRoot: projectRoot)

        // Lexicographic sort — requires ISO 8601 dates from manifest (e.g. "2025-01-15T10:30:00Z")
        let worktrees: [WorktreeModel] = manifest.worktrees.values
            .sorted(by: { $0.createdAt < $1.createdAt })
            .map { entry in
                WorktreeModel(
                    id: entry.id,
                    name: entry.name,
                    path: entry.path,
                    branch: entry.branch,
                    status: entry.status,
                    tmuxWindow: entry.tmuxWindow,
                    agents: entry.agents.values
                        .sorted(by: { $0.startedAt < $1.startedAt })
                        .map { AgentModel(from: $0) }
                )
            }

        let rootAgents: [AgentModel] = (manifest.agents ?? [:]).values
            .sorted(by: { $0.startedAt < $1.startedAt })
            .map { AgentModel(from: $0) }

        return WorkspaceSnapshot(
            worktrees: worktrees,
            rootAgents: rootAgents,
            sessionName: manifest.sessionName
        )
    }

    private func readManifest(projectRoot: String) throws -> ManifestModel {
        let path = manifestPath(projectRoot: projectRoot)
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        return try JSONDecoder().decode(ManifestModel.self, from: data)
    }
}
