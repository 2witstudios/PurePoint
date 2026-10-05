import Foundation

/// Where a workspace lives: directly in the project root, or inside a git worktree.
/// Mirrors the manifest's only two containers (`Manifest.agents` vs `WorktreeEntry.agents`).
nonisolated enum WorkspaceContainer: Equatable, Hashable, Sendable {
    case projectRoot
    case worktree(String)

    var worktreeId: String? {
        if case .worktree(let id) = self { return id }
        return nil
    }
}

/// What a tab shows. Shells are agents too (the daemon spawns them under an `ag-` ID).
nonisolated enum SurfaceContent: Equatable, Sendable {
    /// A tab waiting for the palette: spawn an agent, open a file, or stay empty.
    case empty
    case agent(String)
    /// The file navigator/editor. `path` is the absolute file to show; `nil` opens the navigator alone.
    case file(path: String?)

    var agentId: String? {
        if case .agent(let id) = self { return id }
        return nil
    }
}

/// One tab. The ID is stable for the tab's lifetime — moving it between panes keeps it —
/// so it is what reservations, drag and drop, and `pu grid tab` address.
nonisolated struct Surface: Identifiable, Equatable, Sendable {
    let id: Int
    var content: SurfaceContent
}

/// The content of one leaf of the split tree: an ordered stack of tabs, one of them active.
/// A pane always holds at least one tab; an "empty pane" is a pane whose only tab is `.empty`.
nonisolated struct Pane: Equatable, Sendable {
    var tabs: [Surface]
    var activeTabId: Int

    var activeTab: Surface? { tabs.first { $0.id == activeTabId } ?? tabs.first }

    var activeIndex: Int? { tabs.firstIndex { $0.id == activeTabId } }

    func index(of surfaceId: Int) -> Int? { tabs.firstIndex { $0.id == surfaceId } }

    /// Remove a tab. When it was the active one, its left neighbour becomes active —
    /// or the new first tab when it was leftmost.
    @discardableResult
    mutating func remove(surfaceId: Int) -> Surface? {
        guard let index = index(of: surfaceId) else { return nil }
        let removed = tabs.remove(at: index)
        if activeTabId == surfaceId, !tabs.isEmpty {
            activeTabId = tabs[max(0, index - 1)].id
        }
        return removed
    }
}

/// A workspace is the single unit of the UI: **one sidebar row, one pane layout.**
///
/// This type is the reason panes can no longer decompose into loose sidebar items.
/// The sidebar renders workspaces and nothing else — there is no code path that turns
/// an agent into a row on its own — so a pane and a stray row for the same agent are
/// not two states that must be kept in sync, they are one state.
///
/// Layout and content are separate: `root` is pure geometry, and `panes` maps each of its
/// leaves to a tab stack of surfaces. Invariants, restored by `normalize()`:
/// 1. The keys of `panes` are exactly the tree's leaf IDs, and every pane has a tab.
/// 2. Surface IDs are unique within the workspace and below `nextSurfaceId`.
/// 3. Every live agent in the manifest occupies exactly one surface of exactly one workspace —
///    `WorkspaceReconciler.reconcile` is the only function that establishes that, and it is
///    total: any (stored layout, manifest) pair maps to one canonical answer.
nonisolated struct Workspace: Identifiable, Equatable, Sendable {
    let id: String
    var container: WorkspaceContainer
    var root: PaneSplitNode
    var panes: [Int: Pane]
    var focusedLeafId: Int
    var nextLeafId: Int
    var nextSurfaceId: Int

    /// Leaves missing from `panes` get a single empty tab, so a caller can describe just the
    /// geometry plus whichever panes it cares about.
    init(
        id: String, container: WorkspaceContainer, root: PaneSplitNode, panes: [Int: Pane] = [:],
        focusedLeafId: Int, nextLeafId: Int, nextSurfaceId: Int = 0
    ) {
        self.id = id
        self.container = container
        self.root = root
        self.panes = panes
        self.focusedLeafId = focusedLeafId
        self.nextLeafId = nextLeafId
        self.nextSurfaceId = nextSurfaceId
        normalize()
    }

    /// A brand-new single-pane workspace holding one agent.
    /// The ID is derived from the agent so it is stable across launches without any
    /// random or time-based input — restarting can never invent a different grouping.
    static func adopting(agentId: String, container: WorkspaceContainer) -> Workspace {
        Workspace(
            id: "ws-\(agentId)",
            container: container,
            root: .leaf(id: 0),
            panes: [0: Pane(tabs: [Surface(id: 0, content: .agent(agentId))], activeTabId: 0)],
            focusedLeafId: 0,
            nextLeafId: 1,
            nextSurfaceId: 1
        )
    }

    // MARK: - Queries

    var paneCount: Int { root.leafCount }

    var tabCount: Int { panes.values.reduce(0) { $0 + $1.tabs.count } }

    /// Every tab in layout order: panes in tree order, tabs in stack order.
    var surfaces: [(leafId: Int, surface: Surface)] {
        root.allLeafIds.flatMap { leafId in
            (panes[leafId]?.tabs ?? []).map { (leafId: leafId, surface: $0) }
        }
    }

    /// Agent IDs occupying tabs, in layout order.
    var agentIds: [String] { surfaces.compactMap(\.surface.content.agentId) }

    /// The agent that names this workspace in the sidebar — the first tab holding one.
    var primaryAgentId: String? { agentIds.first }

    var focusedPane: Pane? { panes[focusedLeafId] }

    var focusedSurface: Surface? { focusedPane?.activeTab }

    var focusedAgentId: String? { focusedSurface?.content.agentId }

    func contains(agentId: String) -> Bool { agentIds.contains(agentId) }

    /// The pane holding a tab.
    func leafId(ofSurface surfaceId: Int) -> Int? {
        surfaces.first { $0.surface.id == surfaceId }?.leafId
    }

    func surface(id surfaceId: Int) -> Surface? {
        surfaces.first { $0.surface.id == surfaceId }?.surface
    }

    /// The tab holding an agent, and the pane it is in.
    func location(ofAgent agentId: String) -> (leafId: Int, surfaceId: Int)? {
        surfaces.first { $0.surface.content.agentId == agentId }.map { ($0.leafId, $0.surface.id) }
    }

    // MARK: - Pane Mutations

    /// Split a pane. The new pane starts with one tab showing `content` and takes focus.
    /// Returns the new pane's leaf ID, or `nil` when there is no room for another pane.
    @discardableResult
    mutating func split(leafId: Int, axis: PaneSplitNode.Axis, content: SurfaceContent = .empty) -> Int? {
        guard root.canSplit(axis: axis), panes[leafId] != nil else { return nil }
        root = root.splittingLeaf(id: leafId, axis: axis, nextId: &nextLeafId)
        let newLeafId = nextLeafId - 1
        let surface = makeSurface(content)
        panes[newLeafId] = Pane(tabs: [surface], activeTabId: surface.id)
        focusedLeafId = newLeafId
        return newLeafId
    }

    /// Remove a pane and every tab in it. Returns the agents those tabs held so the caller
    /// can kill them. Removing the last pane leaves the workspace with no panes at all —
    /// a state that cannot be shown, so the caller drops the workspace instead.
    @discardableResult
    mutating func closePane(leafId: Int) -> [String] {
        guard let pane = panes.removeValue(forKey: leafId) else { return [] }
        let sibling = root.siblingLeafId(of: leafId)
        if let newRoot = root.removingLeaf(id: leafId) {
            root = newRoot
            if focusedLeafId == leafId {
                focusedLeafId = sibling ?? root.firstLeafId
            }
        }
        return pane.tabs.compactMap(\.content.agentId)
    }

    /// Whether closing this tab would leave the workspace with nothing to show.
    func isLastTab(_ surfaceId: Int) -> Bool {
        tabCount == 1 && leafId(ofSurface: surfaceId) != nil
    }

    mutating func setRatio(_ ratio: CGFloat, forSplitIdentifiedByFirstLeaf leafId: Int) {
        root = root.settingRatio(ratio, forSplitIdentifiedByFirstLeaf: leafId)
    }

    mutating func moveFocus(direction: FocusDirection) {
        let (axis, forward) = direction.axisAndDirection
        if let adjacent = root.findAdjacentLeaf(from: focusedLeafId, axis: axis, forward: forward) {
            focusedLeafId = adjacent
        }
    }

    // MARK: - Tab Mutations

    /// Open a tab right after the pane's active one. It becomes active and its pane takes focus.
    @discardableResult
    mutating func newTab(leafId: Int, content: SurfaceContent = .empty) -> Int? {
        guard var pane = panes[leafId] else { return nil }
        let surface = makeSurface(content)
        let insertAt = (pane.activeIndex ?? pane.tabs.count - 1) + 1
        pane.tabs.insert(surface, at: insertAt)
        pane.activeTabId = surface.id
        panes[leafId] = pane
        focusedLeafId = leafId
        return surface.id
    }

    /// Show a tab and focus its pane.
    mutating func selectTab(_ surfaceId: Int) {
        guard let leafId = leafId(ofSurface: surfaceId) else { return }
        panes[leafId]?.activeTabId = surfaceId
        focusedLeafId = leafId
    }

    /// Show the tab at a 0-based position in a pane. Out-of-range positions do nothing.
    mutating func selectTab(leafId: Int, index: Int) {
        guard let pane = panes[leafId], pane.tabs.indices.contains(index) else { return }
        selectTab(pane.tabs[index].id)
    }

    mutating func selectLastTab(leafId: Int) {
        guard let last = panes[leafId]?.tabs.last else { return }
        selectTab(last.id)
    }

    /// Step through a pane's tabs, wrapping at either end.
    mutating func cycleTab(leafId: Int, by offset: Int) {
        guard let pane = panes[leafId], !pane.tabs.isEmpty else { return }
        let current = pane.activeIndex ?? 0
        let count = pane.tabs.count
        selectTab(pane.tabs[((current + offset) % count + count) % count].id)
    }

    /// Close a tab. Returns the agent it held so the caller can kill it. A pane whose last
    /// tab closes is removed, exactly as if the pane itself had been closed.
    @discardableResult
    mutating func closeTab(_ surfaceId: Int) -> [String] {
        guard let leafId = leafId(ofSurface: surfaceId) else { return [] }
        if panes[leafId]?.tabs.count == 1 {
            return closePane(leafId: leafId)
        }
        let removed = panes[leafId]?.remove(surfaceId: surfaceId)
        return removed?.content.agentId.map { [$0] } ?? []
    }

    /// Move a tab to a 0-based position in another (or the same) pane. The tab keeps its ID
    /// and content — an agent in it keeps running — and becomes active where it lands.
    /// A source pane left with no tabs collapses.
    mutating func moveTab(_ surfaceId: Int, toLeaf targetLeafId: Int, index: Int? = nil) {
        guard let sourceLeafId = leafId(ofSurface: surfaceId), panes[targetLeafId] != nil,
            let surface = panes[sourceLeafId]?.remove(surfaceId: surfaceId)
        else { return }

        var target = panes[targetLeafId]!
        let insertAt = min(max(0, index ?? target.tabs.count), target.tabs.count)
        target.tabs.insert(surface, at: insertAt)
        target.activeTabId = surface.id
        panes[targetLeafId] = target

        if panes[sourceLeafId]?.tabs.isEmpty == true {
            panes[sourceLeafId] = nil
            root = root.removingLeaf(id: sourceLeafId) ?? root
        }
        focusedLeafId = targetLeafId
    }

    /// Move a tab out of its pane into a new pane split off beside it — tmux's break-pane,
    /// within the workspace. Does nothing for a pane's only tab (there is nothing to break
    /// away from) or when the grid is full. Returns the new pane's leaf ID.
    @discardableResult
    mutating func breakTab(_ surfaceId: Int, axis: PaneSplitNode.Axis) -> Int? {
        guard let sourceLeafId = leafId(ofSurface: surfaceId),
            let source = panes[sourceLeafId], source.tabs.count > 1,
            root.canSplit(axis: axis)
        else { return nil }

        root = root.splittingLeaf(id: sourceLeafId, axis: axis, nextId: &nextLeafId)
        let newLeafId = nextLeafId - 1
        guard let surface = panes[sourceLeafId]?.remove(surfaceId: surfaceId) else { return nil }
        panes[newLeafId] = Pane(tabs: [surface], activeTabId: surface.id)
        focusedLeafId = newLeafId
        return newLeafId
    }

    /// Stack every tab of one pane onto the end of another and remove the emptied pane —
    /// the inverse of `breakTab`. The joined pane's active tab stays active.
    mutating func joinPane(_ leafId: Int, into targetLeafId: Int) {
        guard leafId != targetLeafId, let source = panes[leafId], panes[targetLeafId] != nil else { return }
        let activeId = source.activeTab?.id
        panes[targetLeafId]?.tabs.append(contentsOf: source.tabs)
        if let activeId { panes[targetLeafId]?.activeTabId = activeId }
        panes[leafId] = nil
        root = root.removingLeaf(id: leafId) ?? root
        focusedLeafId = targetLeafId
    }

    /// Change what a tab shows — a reservation being fulfilled, or the palette's choice.
    mutating func setContent(_ content: SurfaceContent, forSurface surfaceId: Int) {
        guard let leafId = leafId(ofSurface: surfaceId),
            let index = panes[leafId]?.index(of: surfaceId)
        else { return }
        panes[leafId]?.tabs[index].content = content
    }

    /// Remove every tab matching `predicate`, collapsing panes left empty. Returns `false`
    /// when no pane survives, in which case the workspace must be dropped.
    mutating func removeSurfaces(where predicate: (Surface) -> Bool) -> Bool {
        var emptied = Set<Int>()
        for (leafId, var pane) in panes {
            let doomed = pane.tabs.filter(predicate).map(\.id)
            guard !doomed.isEmpty else { continue }
            for surfaceId in doomed { pane.remove(surfaceId: surfaceId) }
            if pane.tabs.isEmpty {
                emptied.insert(leafId)
            } else {
                panes[leafId] = pane
            }
        }
        guard !emptied.isEmpty else { return true }
        for leafId in emptied { panes[leafId] = nil }
        guard let pruned = root.removingLeaves(ids: emptied) else { return false }
        root = pruned
        return true
    }

    private mutating func makeSurface(_ content: SurfaceContent) -> Surface {
        defer { nextSurfaceId += 1 }
        return Surface(id: nextSurfaceId, content: content)
    }

    // MARK: - Normalize

    /// Re-establish internal consistency after any structural edit.
    mutating func normalize() {
        let ids = root.allLeafIds
        let live = Set(ids)
        panes = panes.filter { live.contains($0.key) }

        // Surface IDs must be unique across the workspace; a duplicate (hand-edited or
        // corrupt file) gets a fresh ID rather than silently aliasing another tab.
        nextSurfaceId = max(nextSurfaceId, (panes.values.flatMap(\.tabs).map(\.id).max() ?? -1) + 1)
        var seen = Set<Int>()
        for leafId in ids {
            var pane = panes[leafId] ?? Pane(tabs: [], activeTabId: -1)
            for index in pane.tabs.indices where !seen.insert(pane.tabs[index].id).inserted {
                let wasActive = pane.activeTabId == pane.tabs[index].id
                pane.tabs[index] = Surface(id: nextSurfaceId, content: pane.tabs[index].content)
                seen.insert(nextSurfaceId)
                if wasActive { pane.activeTabId = nextSurfaceId }
                nextSurfaceId += 1
            }
            if pane.tabs.isEmpty {
                pane.tabs = [Surface(id: nextSurfaceId, content: .empty)]
                seen.insert(nextSurfaceId)
                nextSurfaceId += 1
            }
            if pane.index(of: pane.activeTabId) == nil {
                pane.activeTabId = pane.tabs[0].id
            }
            panes[leafId] = pane
        }

        if !live.contains(focusedLeafId) {
            focusedLeafId = ids.first ?? 0
        }
        nextLeafId = max(nextLeafId, (ids.max() ?? -1) + 1)
    }
}

nonisolated enum FocusDirection: Sendable {
    case up, down, left, right

    var axisAndDirection: (PaneSplitNode.Axis, Bool) {
        switch self {
        case .up: (.horizontal, false)
        case .down: (.horizontal, true)
        case .left: (.vertical, false)
        case .right: (.vertical, true)
        }
    }
}

// MARK: - Reconciler

/// One live agent as the manifest reports it.
nonisolated struct LiveAgent: Equatable, Sendable {
    let id: String
    let container: WorkspaceContainer
}

/// The single function that turns (stored layout, manifest) into the canonical workspace list.
///
/// It is pure, total, and order-stable: the same inputs always yield the same output, so a
/// restart reproduces exactly the grouping the user last saw. Nothing else in the app is
/// allowed to construct or filter the sidebar's row set.
nonisolated enum WorkspaceReconciler {

    /// Guarantees on the returned list:
    /// 1. Every live agent occupies exactly one tab of exactly one workspace.
    /// 2. No tab references an agent that is no longer in the manifest.
    /// 3. Every workspace holds at least one live agent (no ghost rows).
    /// 4. Each workspace's container matches where its agents actually live.
    static func reconcile(stored: [Workspace], live: [LiveAgent]) -> [Workspace] {
        let containerByAgent = Dictionary(live.map { ($0.id, $0.container) }, uniquingKeysWith: { first, _ in first })

        var claimed = Set<String>()
        var result: [Workspace] = []
        var seenWorkspaceIds = Set<String>()

        for var workspace in stored {
            // A duplicated workspace ID would make selection ambiguous; the first wins.
            guard seenWorkspaceIds.insert(workspace.id).inserted else { continue }

            // Drop tabs whose agent died, and tabs claiming an agent another tab already
            // holds. Empty and file tabs are deliberate and stay.
            var staleSurfaceIds = Set<Int>()
            for (_, surface) in workspace.surfaces {
                guard let agentId = surface.content.agentId else { continue }
                if containerByAgent[agentId] == nil || !claimed.insert(agentId).inserted {
                    staleSurfaceIds.insert(surface.id)
                }
            }

            if !staleSurfaceIds.isEmpty {
                guard workspace.removeSurfaces(where: { staleSurfaceIds.contains($0.id) }) else {
                    continue  // every pane went away — the workspace goes with it
                }
            }

            // A workspace exists to hold agents. One that has lost them all is a ghost row.
            guard let primary = workspace.agentIds.first else { continue }

            workspace.container = containerByAgent[primary] ?? workspace.container
            workspace.normalize()
            result.append(workspace)
        }

        // Any live agent no tab claimed becomes its own single-pane workspace.
        // This is the step that makes a loose sidebar item impossible: an agent the layout
        // forgot still surfaces as a workspace, never as a bare agent row.
        for agent in live where !claimed.contains(agent.id) {
            claimed.insert(agent.id)
            result.append(.adopting(agentId: agent.id, container: agent.container))
        }

        return result
    }

    /// Flatten a project's manifest into the ordered agent list the reconciler expects.
    /// Root agents first, then each worktree's agents — manifest order throughout.
    static func liveAgents(rootAgents: [AgentModel], worktrees: [WorktreeModel]) -> [LiveAgent] {
        var live = rootAgents.map { LiveAgent(id: $0.id, container: .projectRoot) }
        for worktree in worktrees {
            live.append(contentsOf: worktree.agents.map { LiveAgent(id: $0.id, container: .worktree(worktree.id)) })
        }
        return live
    }
}
