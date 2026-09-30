import Foundation

/// Codable representation of a PaneSplitNode tree for disk persistence.
/// Uses a class to break the value-type recursion for Codable.
nonisolated final class GridLayoutNode: Codable, Sendable {
    enum NodeType: String, Codable, Sendable {
        case leaf
        case split
    }

    let type: NodeType
    /// Persisted pane ID. Optional so layouts written before pane IDs were stored still load.
    let leafId: Int?
    let agentId: String?
    let axis: PaneSplitNode.Axis?
    let ratio: CGFloat?
    let first: GridLayoutNode?
    let second: GridLayoutNode?

    init(
        type: NodeType, leafId: Int?, agentId: String?, axis: PaneSplitNode.Axis?, ratio: CGFloat?,
        first: GridLayoutNode?, second: GridLayoutNode?
    ) {
        self.type = type
        self.leafId = leafId
        self.agentId = agentId
        self.axis = axis
        self.ratio = ratio
        self.first = first
        self.second = second
    }
}

/// One workspace as written to `.pu/workspaces.json`.
nonisolated struct PersistedWorkspace: Codable, Sendable {
    let id: String
    /// `nil` means the workspace lives in the project root rather than a worktree.
    let worktreeId: String?
    let focusedLeafId: Int
    let nextLeafId: Int
    let tree: GridLayoutNode
}

/// The whole per-project layout document. This file is the only place pane grouping
/// is stored — there is no second copy to drift from.
nonisolated struct PersistedWorkspaceDocument: Codable, Sendable {
    static let currentVersion = 2

    let version: Int
    let workspaces: [PersistedWorkspace]
}

/// Legacy `.pu/grid-layout.json` — a single app-wide grid owned by one agent.
private struct LegacyPersistedGridLayout: Codable {
    let ownerAgentId: String?
    let tree: GridLayoutNode
}

/// Reads and writes `.pu/workspaces.json`.
///
/// Loading never fabricates grouping: a missing or unreadable file yields an empty list,
/// and `WorkspaceReconciler` then adopts every manifest agent into its own workspace.
/// That is a deterministic outcome, not a drifted one.
nonisolated enum WorkspacePersistence {

    static func filePath(projectRoot: String) -> String {
        (projectRoot as NSString).appendingPathComponent(".pu").appending("/workspaces.json")
    }

    private static func legacyFilePath(projectRoot: String) -> String {
        (projectRoot as NSString).appendingPathComponent(".pu").appending("/grid-layout.json")
    }

    // MARK: - Save

    static func save(_ workspaces: [Workspace], projectRoot: String) {
        let document = PersistedWorkspaceDocument(
            version: PersistedWorkspaceDocument.currentVersion,
            workspaces: workspaces.map { workspace in
                PersistedWorkspace(
                    id: workspace.id,
                    worktreeId: workspace.container.worktreeId,
                    focusedLeafId: workspace.focusedLeafId,
                    nextLeafId: workspace.nextLeafId,
                    tree: workspace.root.toLayoutNode()
                )
            }
        )

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(document) else { return }

        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    // MARK: - Load

    static func load(projectRoot: String) -> [Workspace] {
        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        if let data = try? Data(contentsOf: url),
            let document = try? JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
        {
            return document.workspaces.map(workspace(from:))
        }
        return migrateLegacyLayout(projectRoot: projectRoot)
    }

    private static func workspace(from persisted: PersistedWorkspace) -> Workspace {
        var nextId = persisted.nextLeafId
        let root = PaneSplitNode.fromLayoutNode(persisted.tree, nextId: &nextId)
        var workspace = Workspace(
            id: persisted.id,
            container: persisted.worktreeId.map { .worktree($0) } ?? .projectRoot,
            root: root,
            focusedLeafId: persisted.focusedLeafId,
            nextLeafId: nextId
        )
        workspace.normalize()
        return workspace
    }

    /// Fold a pre-workspace `grid-layout.json` into a single workspace, then retire the file.
    /// Agents that file never mentioned are adopted by the reconciler, so nothing is lost.
    private static func migrateLegacyLayout(projectRoot: String) -> [Workspace] {
        let legacyURL = URL(fileURLWithPath: legacyFilePath(projectRoot: projectRoot))
        guard let data = try? Data(contentsOf: legacyURL) else { return [] }

        let tree: GridLayoutNode
        let ownerAgentId: String?
        if let legacy = try? JSONDecoder().decode(LegacyPersistedGridLayout.self, from: data) {
            tree = legacy.tree
            ownerAgentId = legacy.ownerAgentId
        } else if let bare = try? JSONDecoder().decode(GridLayoutNode.self, from: data) {
            tree = bare
            ownerAgentId = nil
        } else {
            return []
        }

        var nextId = 0
        let root = PaneSplitNode.fromLayoutNode(tree, nextId: &nextId)
        guard let anchorAgentId = ownerAgentId ?? root.leaves.compactMap(\.agentId).first else {
            try? FileManager.default.removeItem(at: legacyURL)
            return []
        }

        var workspace = Workspace(
            id: "ws-\(anchorAgentId)",
            container: .projectRoot,  // reconcile corrects this from the manifest
            root: root,
            focusedLeafId: root.firstLeafId,
            nextLeafId: nextId
        )
        workspace.normalize()

        let migrated = [workspace]
        save(migrated, projectRoot: projectRoot)
        try? FileManager.default.removeItem(at: legacyURL)
        return migrated
    }

    static func clear(projectRoot: String) {
        try? FileManager.default.removeItem(atPath: filePath(projectRoot: projectRoot))
    }
}
