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

/// One tab as written to disk. `kind` is `agent`, `file`, `point_guard` or `empty`; an unknown kind
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
        case .pointGuard: (kind, agentId, path) = ("point_guard", nil, nil)
        case .agent(let agentId): (kind, self.agentId, path) = ("agent", agentId, nil)
        case .file(let path): (kind, agentId, self.path) = ("file", nil, path)
        }
    }

    var surface: Surface {
        switch kind {
        case "agent": Surface(id: id, content: agentId.map { .agent($0) } ?? .empty)
        case "file": Surface(id: id, content: .file(path: path))
        case "point_guard": Surface(id: id, content: .pointGuard)
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
    // Version 4 adds Point Guard tabs. Older version 3 builds must not overwrite them.
    static let currentVersion = 4

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
        guard let data = documentData(workspaces, activeWorkspaceId: activeWorkspaceId) else { return }

        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        try? FileManager.default.createDirectory(
            at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? data.write(to: url, options: .atomic)
    }

    /// Write a local project's layout unless the document on disk changed since this app last
    /// read or wrote it (`expectedModifiedMs`, nil when there was no document). Another Mac
    /// reaching this one over SSH writes the same file; its change is returned as a conflict
    /// for the caller to adopt instead of being overwritten. A document deleted meanwhile has
    /// nothing to adopt, so it is written again.
    static func saveLocal(_ data: Data, projectRoot: String, expectedModifiedMs: Int64?) -> RemoteSaveOutcome {
        let host = LocalProjectHost.shared
        let path = filePath(projectRoot: projectRoot)
        guard let stat = try? host.stat(path: path) else { return .skipped }
        let onDisk = stat.exists ? stat.modifiedMs : nil
        if let onDisk, onDisk != expectedModifiedMs {
            guard let current = try? host.read(path: path, maxBytes: maxRemoteLayoutBytes), !current.truncated,
                !isNewerThanSupported(data: current.data)
            else { return .skipped }
            return .conflict(current.data, modifiedMs: current.modifiedMs)
        }
        guard !isNewerThanSupported(projectRoot: projectRoot) else { return .skipped }
        do {
            return .written(
                modifiedMs: try host.write(
                    path: path, data: data, expectedModifiedMs: onDisk, createDirectories: true
                ).modifiedMs)
        } catch {
            return .skipped
        }
    }

    /// Modification time of a local project's layout document, nil when there is none.
    static func localModifiedMs(projectRoot: String) -> Int64? {
        guard let stat = try? LocalProjectHost.shared.stat(path: filePath(projectRoot: projectRoot)), stat.exists
        else { return nil }
        return stat.modifiedMs
    }

    /// The encoded layout document. Shared by local saves and saves through a remote host.
    static func documentData(_ workspaces: [Workspace], activeWorkspaceId: String?) -> Data? {
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
        return try? encoder.encode(document)
    }

    static func isNewerThanSupported(projectRoot: String) -> Bool {
        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        guard let data = try? Data(contentsOf: url) else { return false }
        return isNewerThanSupported(data: data)
    }

    static func isNewerThanSupported(data: Data) -> Bool {
        guard let probe = try? JSONDecoder().decode(PersistedVersionProbe.self, from: data) else { return false }
        return probe.version > PersistedWorkspaceDocument.currentVersion
    }

    /// Decode a layout document read through a host. `projectId` keys Point Guard workspaces.
    static func workspaces(from data: Data, projectId: String) -> [Workspace] {
        guard let document = try? JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data) else { return [] }
        return document.workspaces.map { workspace(from: $0, projectRoot: projectId) }
    }

    // MARK: - Remote hosts

    /// Load a remote project's layout through its host. Blocking: call off the main thread.
    /// Only a missing file is an empty layout. Any other failure throws, because an empty
    /// layout adopted after a failed read would be saved over the real one.
    static func loadRemote(projectRoot: String, projectId: String, host: any ProjectHost) throws -> RemoteLayout {
        let read: HostFileData
        do {
            read = try host.read(path: filePath(projectRoot: projectRoot), maxBytes: maxRemoteLayoutBytes)
        } catch HostError.notFound {
            return RemoteLayout(workspaces: [], modifiedMs: nil)
        }
        guard !read.truncated else { throw RemoteLayoutError.tooLarge }
        return RemoteLayout(workspaces: workspaces(from: read.data, projectId: projectId), modifiedMs: read.modifiedMs)
    }

    static let maxRemoteLayoutBytes = 4 * 1_048_576

    /// Save a remote project's layout through its host, never downgrading a newer document.
    /// The write is conditional on the modification time of the document this app last read
    /// or wrote (`expectedModifiedMs`), else of the one just read. When the document changed
    /// meanwhile (another Mac, or `pu grid` on the machine) nothing is written: the outcome
    /// carries that document so the caller adopts it instead of reverting it. Nothing is
    /// written either when the current document cannot be read (anything but a missing file).
    /// Blocking: call off the main thread.
    @discardableResult
    static func saveRemote(
        _ data: Data, projectRoot: String, host: any ProjectHost, expectedModifiedMs: Int64?
    ) -> RemoteSaveOutcome {
        let path = filePath(projectRoot: projectRoot)
        let onDisk: Int64?
        do {
            let existing = try host.read(path: path, maxBytes: maxRemoteLayoutBytes)
            if existing.truncated || isNewerThanSupported(data: existing.data) { return .skipped }
            if let expectedModifiedMs, existing.modifiedMs != expectedModifiedMs {
                return .conflict(existing.data, modifiedMs: existing.modifiedMs)
            }
            onDisk = existing.modifiedMs
        } catch HostError.notFound {
            onDisk = nil
        } catch {
            return .skipped
        }
        do {
            return .written(
                // `onDisk` equals `expectedModifiedMs` when both are known. A document deleted
                // since it was read has nothing to adopt, so it is written again.
                modifiedMs: try host.write(
                    path: path, data: data, expectedModifiedMs: onDisk, createDirectories: true
                ).modifiedMs)
        } catch HostError.conflict {
            // Changed between the read and the write: adopt what is there now.
            guard let current = try? host.read(path: path, maxBytes: maxRemoteLayoutBytes), !current.truncated,
                !isNewerThanSupported(data: current.data)
            else { return .skipped }
            return .conflict(current.data, modifiedMs: current.modifiedMs)
        } catch {
            return .skipped
        }
    }

    // MARK: - Load

    static func load(projectRoot: String) -> [Workspace] {
        (try? loadForRegistry(projectRoot: projectRoot)) ?? []
    }

    /// Registry loading must distinguish an unavailable identity migration from no layout.
    /// Never publish a new identity until its atomic write has succeeded.
    static func loadForRegistry(
        projectRoot: String,
        writeMigration: (Data, URL) throws -> Void = { try $0.write(to: $1, options: .atomic) }
    ) throws -> [Workspace] {
        let url = URL(fileURLWithPath: filePath(projectRoot: projectRoot))
        if let data = try? Data(contentsOf: url),
            let document = try? JSONDecoder().decode(PersistedWorkspaceDocument.self, from: data)
        {
            let restored = document.workspaces.map { workspace(from: $0, projectRoot: projectRoot) }
            // Copied/renamed project directories keep their layout, but must not publish an
            // ID belonging to another open project. Persist the new ID for CLI routing too.
            if zip(document.workspaces, restored).contains(where: { $0.id != $1.id }) {
                try migrateWorkspaceIds(in: data, to: restored, at: url, write: writeMigration)
            }
            return restored
        }
        return migrateLegacyLayout(projectRoot: projectRoot)
    }

    /// Identity-only migration also works for decodable newer documents. Re-encoding the
    /// typed model would drop unknown fields and surface kinds, so patch the raw JSON and
    /// retain its version. Normal layout saves still refuse to overwrite a newer schema.
    private static func migrateWorkspaceIds(
        in data: Data, to restored: [Workspace], at url: URL, write: (Data, URL) throws -> Void
    ) throws {
        guard var document = try JSONSerialization.jsonObject(with: data) as? [String: Any],
            var workspaces = document["workspaces"] as? [[String: Any]],
            workspaces.count == restored.count
        else { throw CocoaError(.fileReadCorruptFile) }
        let activeId = document["activeWorkspaceId"] as? String
        for index in workspaces.indices {
            if workspaces[index]["id"] as? String == activeId, activeId != nil {
                document["activeWorkspaceId"] = restored[index].id
            }
            workspaces[index]["id"] = restored[index].id
        }
        document["workspaces"] = workspaces
        let migrated = try JSONSerialization.data(withJSONObject: document, options: [.prettyPrinted, .sortedKeys])
        try write(migrated, url)
    }

    private static func workspace(from persisted: PersistedWorkspace, projectRoot: String) -> Workspace {
        var nextLeafId = persisted.nextLeafId
        var nextSurfaceId = persisted.nextSurfaceId ?? 0
        let filePaths = Dictionary(
            (persisted.filePanes ?? []).map { ($0.leafId, $0.path) }, uniquingKeysWith: { first, _ in first })
        var panes: [Int: Pane] = [:]
        let root = tree(
            from: persisted.tree, panes: &panes, legacyFilePaths: filePaths,
            nextLeafId: &nextLeafId, nextSurfaceId: &nextSurfaceId)
        return Workspace(
            id: persisted.id.hasPrefix("ws-point-guard:")
                ? Workspace.pointGuardId(projectRoot: projectRoot) : persisted.id,
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

/// A remote layout as read: its workspaces and the document's modification time (nil when
/// there is no document yet).
nonisolated struct RemoteLayout {
    let workspaces: [Workspace]
    let modifiedMs: Int64?
}

nonisolated enum RemoteLayoutError: LocalizedError {
    case tooLarge

    var errorDescription: String? {
        switch self {
        case .tooLarge: "The layout file on the remote machine is larger than 4 MB."
        }
    }
}

/// What a conditional layout save (remote, or local via `saveLocal`) did.
nonisolated enum RemoteSaveOutcome: Equatable, Sendable {
    case written(modifiedMs: Int64)
    /// The machine's document changed since this app last read or wrote it; it was not
    /// overwritten. Carries that document for the caller to adopt.
    case conflict(Data, modifiedMs: Int64)
    /// Nothing written: the document could not be read safely, or is a newer version.
    case skipped
}

/// Writes a remote project's layout through its host on a serial queue. Quit and disconnect
/// wait (bounded) for the writes already queued, so closing the SSH forward cannot cut off
/// the last layout change.
nonisolated final class RemoteLayoutSaver: @unchecked Sendable {
    private let queue = DispatchQueue(label: "purepoint.remote-layout-save")
    private let group = DispatchGroup()
    /// Writes `data` given the document's last known modification time.
    private let write: @Sendable (Data, Int64?) -> RemoteSaveOutcome
    /// Modification time of the document as this app last read or wrote it. Queue-confined.
    private var knownModifiedMs: Int64?
    private let conflictLock = NSLock()
    nonisolated(unsafe) private var conflictHandler: (@Sendable (Data, Int64) -> Void)?

    convenience init(projectRoot: String, host: any ProjectHost) {
        self.init { data, expected in
            WorkspacePersistence.saveRemote(data, projectRoot: projectRoot, host: host, expectedModifiedMs: expected)
        }
    }

    init(write: @escaping @Sendable (Data, Int64?) -> RemoteSaveOutcome) {
        self.write = write
    }

    /// Called (off the main thread) with the machine's document and its modification time
    /// when a save found it changed. The handler adopts it and then calls `loaded`, so
    /// writes already queued behind the conflict conflict too instead of reverting it.
    func onConflict(_ handler: @escaping @Sendable (Data, Int64) -> Void) {
        conflictLock.lock()
        conflictHandler = handler
        conflictLock.unlock()
    }

    /// The document as just loaded, so the next save is conditional on it.
    func loaded(modifiedMs: Int64?) {
        queue.async { self.knownModifiedMs = modifiedMs }
    }

    func save(_ workspaces: [Workspace], activeWorkspaceId: String?) {
        guard let data = WorkspacePersistence.documentData(workspaces, activeWorkspaceId: activeWorkspaceId) else { return }
        enqueue(data)
    }

    func enqueue(_ data: Data) {
        let group = self.group
        group.enter()
        queue.async {
            switch self.write(data, self.knownModifiedMs) {
            case .written(let modified):
                self.knownModifiedMs = modified
            case .conflict(let document, let modified):
                // The stamp advances only once the document is adopted (`loaded`).
                self.conflictLock.lock()
                let handler = self.conflictHandler
                self.conflictLock.unlock()
                handler?(document, modified)
            case .skipped:
                break
            }
            group.leave()
        }
    }

    /// Wait until every queued write finished or `deadline` passed; false on timeout.
    @discardableResult
    func flush(until deadline: DispatchTime) -> Bool {
        group.wait(timeout: deadline) == .success
    }
}
