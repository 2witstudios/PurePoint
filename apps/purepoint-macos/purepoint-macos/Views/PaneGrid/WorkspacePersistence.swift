import Foundation

/// Codable representation of a PaneSplitNode tree for disk persistence.
/// Uses a class to break the value-type recursion for Codable.
///
/// A leaf carries its pane's tab stack (`tabs` + `activeTabId`). Version 2 files stored a
/// single `agentId` on the leaf instead; it is still decoded so they migrate in place.
nonisolated final class GridLayoutNode: Codable, Sendable {
    enum NodeType: String, Codable, Sendable {
        case leaf
        case split
    }

    let type: NodeType
    /// Persisted pane ID. Optional so layouts written before pane IDs were stored still load.
    let leafId: Int?
    /// Version 2 only: the one agent a leaf held before panes had tabs.
    let agentId: String?
    let activeTabId: Int?
    let tabs: [PersistedSurface]?
    let axis: PaneSplitNode.Axis?
    let ratio: CGFloat?
    let first: GridLayoutNode?
    let second: GridLayoutNode?

    init(
        type: NodeType, leafId: Int?, agentId: String? = nil, activeTabId: Int? = nil,
        tabs: [PersistedSurface]? = nil, axis: PaneSplitNode.Axis?, ratio: CGFloat?,
        first: GridLayoutNode?, second: GridLayoutNode?
    ) {
        self.type = type
        self.leafId = leafId
        self.agentId = agentId
        self.activeTabId = activeTabId
        self.tabs = tabs
        self.axis = axis
        self.ratio = ratio
        self.first = first
        self.second = second
    }
}

/// One tab as written to disk. `kind` is `agent`, `file` or `empty`; an unknown kind
/// (a newer app's file) loads as an empty tab rather than failing the whole document.
nonisolated struct PersistedSurface: Codable, Sendable, Equatable {
    let id: Int
    let kind: String
    let agentId: String?
    let path: String?

    init(_ surface: Surface) {
        id = surface.id
        switch surface.content {
        case .empty: (kind, agentId, path) = ("empty", nil, nil)
        case .agent(let agentId): (kind, self.agentId, path) = ("agent", agentId, nil)
        case .file(let path): (kind, agentId, self.path) = ("file", nil, path)
        }
    }

    var surface: Surface {
        switch kind {
        case "agent": Surface(id: id, content: agentId.map { .agent($0) } ?? .empty)
        case "file": Surface(id: id, content: .file(path: path))
        default: Surface(id: id, content: .empty)
        }
    }
}

/// One workspace as written to `.pu/workspaces.json`.
nonisolated struct PersistedWorkspace: Codable, Sendable {
    let id: String
    /// `nil` means the workspace lives in the project root rather than a worktree.
    let worktreeId: String?
    let focusedLeafId: Int
    let nextLeafId: Int
    /// Optional so version 2 documents, written before tabs existed, still decode.
    let nextSurfaceId: Int?
    let tree: GridLayoutNode
    /// Version 2 only: leaves showing files, before a file became a kind of tab. Never written.
    let filePanes: [PersistedFilePane]?
}

/// Version 2 only: a file pane's leaf and the file it had open.
nonisolated struct PersistedFilePane: Codable, Sendable, Equatable {
    let leafId: Int
    let path: String?
}

/// The whole per-project layout document. This file is the only place pane grouping
/// is stored — there is no second copy to drift from.
nonisolated struct PersistedWorkspaceDocument: Codable, Sendable {
    static let currentVersion = 3

    let version: Int
    let workspaces: [PersistedWorkspace]
    /// The workspace on screen when this was written — where `pu grid` commands without
    /// `--workspace` land. Read by the CLI only; the app restores selection from UserDefaults.
    var activeWorkspaceId: String? = nil
}

/// Just the version, readable from any document — including one too new to decode.
nonisolated private struct PersistedVersionProbe: Decodable {
    let version: Int
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

    /// Write the layout — unless the file on disk was written by a newer build. Overwriting it
    /// would downgrade it and silently drop whatever that build stored (an older build reading
    /// a v3 file this way is how tab groupings would be lost), so a newer file is left alone.
    static func save(_ workspaces: [Workspace], projectRoot: String, activeWorkspaceId: String? = nil) {
        guard !isNewerThanSupported(projectRoot: projectRoot) else { return }
        let document = PersistedWorkspaceDocument(
            version: PersistedWorkspaceDocument.currentVersion,
            workspaces: workspaces.map { workspace in
                PersistedWorkspace(
                    id: workspace.id,
                    worktreeId: workspace.container.worktreeId,
                    focusedLeafId: workspace.focusedLeafId,
                    nextLeafId: workspace.nextLeafId,
                    nextSurfaceId: workspace.nextSurfaceId,
                    tree: layoutNode(workspace.root, panes: workspace.panes),
                    filePanes: nil
                )
            },
            activeWorkspaceId: activeWorkspaceId
        )

        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        guard let data = try? encoder.encode(document) else { return }

        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    static func isNewerThanSupported(projectRoot: String) -> Bool {
        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        guard let data = try? Data(contentsOf: url),
            let probe = try? JSONDecoder().decode(PersistedVersionProbe.self, from: data)
        else { return false }
        return probe.version > PersistedWorkspaceDocument.currentVersion
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
        var nextLeafId = persisted.nextLeafId
        var nextSurfaceId = persisted.nextSurfaceId ?? 0
        let filePaths = Dictionary(
            (persisted.filePanes ?? []).map { ($0.leafId, $0.path) }, uniquingKeysWith: { first, _ in first })
        var panes: [Int: Pane] = [:]
        let root = tree(
            from: persisted.tree, panes: &panes, legacyFilePaths: filePaths,
            nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
        return Workspace(
            id: persisted.id,
            container: persisted.worktreeId.map { .worktree($0) } ?? .projectRoot,
            root: root,
            panes: panes,
            focusedLeafId: persisted.focusedLeafId,
            nextLeafId: nextLeafId,
            nextSurfaceId: nextSurfaceId
        )
    }

    // MARK: - Tree Coding

    /// Leaf IDs are written to disk. They are the handles the daemon's grid protocol
    /// addresses panes by, so a restart that renumbered them would silently retarget
    /// every in-flight `pu grid` command. Surface IDs are written for the same reason.
    static func layoutNode(_ node: PaneSplitNode, panes: [Int: Pane]) -> GridLayoutNode {
        switch node {
        case .leaf(let id):
            let pane = panes[id]
            return GridLayoutNode(
                type: .leaf, leafId: id, activeTabId: pane?.activeTabId,
                tabs: (pane?.tabs ?? []).map(PersistedSurface.init), axis: nil, ratio: nil, first: nil, second: nil)
        case .split(let axis, let ratio, let first, let second):
            return GridLayoutNode(
                type: .split, leafId: nil, axis: axis, ratio: ratio,
                first: layoutNode(first, panes: panes), second: layoutNode(second, panes: panes))
        }
    }

    /// Rebuild geometry and tab stacks from disk. Persisted IDs are reused; the counters only
    /// supply IDs for older files that lack them. A version 2 leaf becomes a single tab: its
    /// agent, else its file pane, else an empty tab.
    static func tree(
        from node: GridLayoutNode, panes: inout [Int: Pane], legacyFilePaths: [Int: String?] = [:],
        nextLeafId: inout Int, nextSurfaceId: inout Int
    ) -> PaneSplitNode {
        if node.type == .split, let axis = node.axis, let ratio = node.ratio,
            let first = node.first, let second = node.second
        {
            return .split(
                axis: axis,
                ratio: ratio,
                first: tree(
                    from: first, panes: &panes, legacyFilePaths: legacyFilePaths,
                    nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId),
                second: tree(
                    from: second, panes: &panes, legacyFilePaths: legacyFilePaths,
                    nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
            )
        }

        let id = node.leafId ?? nextLeafId
        nextLeafId = max(nextLeafId, id + 1)

        if let stored = node.tabs, !stored.isEmpty {
            let tabs = stored.map(\.surface)
            nextSurfaceId = max(nextSurfaceId, (tabs.map(\.id).max() ?? -1) + 1)
            panes[id] = Pane(tabs: tabs, activeTabId: node.activeTabId ?? tabs[0].id)
        } else {
            let content: SurfaceContent
            if let agentId = node.agentId {
                content = .agent(agentId)
            } else if let path = legacyFilePaths[id] {
                content = .file(path: path)
            } else {
                content = .empty
            }
            let surface = Surface(id: nextSurfaceId, content: content)
            nextSurfaceId += 1
            panes[id] = Pane(tabs: [surface], activeTabId: surface.id)
        }
        return .leaf(id: id)
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

        var nextLeafId = 0
        var nextSurfaceId = 0
        var panes: [Int: Pane] = [:]
        let root = self.tree(from: tree, panes: &panes, nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
        let firstAgentId = root.allLeafIds.lazy.compactMap { panes[$0]?.tabs.first?.content.agentId }.first
        guard let anchorAgentId = ownerAgentId ?? firstAgentId else {
            try? FileManager.default.removeItem(at: legacyURL)
            return []
        }

        let workspace = Workspace(
            id: "ws-\(anchorAgentId)",
            container: .projectRoot,  // reconcile corrects this from the manifest
            root: root,
            panes: panes,
            focusedLeafId: root.firstLeafId,
            nextLeafId: nextLeafId,
            nextSurfaceId: nextSurfaceId
        )

        let migrated = [workspace]
        save(migrated, projectRoot: projectRoot)
        try? FileManager.default.removeItem(at: legacyURL)
        return migrated
    }

    static func clear(projectRoot: String) {
        try? FileManager.default.removeItem(atPath: filePath(projectRoot: projectRoot))
    }
}
