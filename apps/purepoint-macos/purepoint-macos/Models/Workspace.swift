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

/// A workspace is the single unit of the UI: **one sidebar row, one pane layout.**
///
/// This type is the reason panes can no longer decompose into loose sidebar items.
/// The sidebar renders workspaces and nothing else — there is no code path that turns
/// an agent into a row on its own — so a pane and a stray row for the same agent are
/// not two states that must be kept in sync, they are one state.
///
/// Every live agent in the manifest occupies exactly one pane of exactly one workspace.
/// `WorkspaceReconciler.reconcile` is the only function that establishes that invariant,
/// and it is total: any (stored layout, manifest) pair maps to one canonical answer.
nonisolated struct Workspace: Identifiable, Equatable, Sendable {
    let id: String
    var container: WorkspaceContainer
    var root: PaneSplitNode
    var focusedLeafId: Int
    var nextLeafId: Int

    init(id: String, container: WorkspaceContainer, root: PaneSplitNode, focusedLeafId: Int, nextLeafId: Int) {
        self.id = id
        self.container = container
        self.root = root
        self.focusedLeafId = focusedLeafId
        self.nextLeafId = nextLeafId
    }

    /// A brand-new single-pane workspace holding one agent.
    /// The ID is derived from the agent so it is stable across launches without any
    /// random or time-based input — restarting can never invent a different grouping.
    static func adopting(agentId: String, container: WorkspaceContainer) -> Workspace {
        Workspace(
            id: "ws-\(agentId)",
            container: container,
            root: .leaf(id: 0, agentId: agentId),
            focusedLeafId: 0,
            nextLeafId: 1
        )
    }

    // MARK: - Queries

    var paneCount: Int { root.leafCount }

    /// Agent IDs occupying panes, in pane order.
    var agentIds: [String] { root.leaves.compactMap(\.agentId) }

    /// The agent that names this workspace in the sidebar — the first pane holding one.
    var primaryAgentId: String? { agentIds.first }

    var focusedAgentId: String? { root.agentId(forLeafId: focusedLeafId) }

    func contains(agentId: String) -> Bool { root.containsAgent(agentId) }

    // MARK: - Mutations

    mutating func split(leafId: Int, axis: PaneSplitNode.Axis, agentId: String? = nil) {
        guard root.canSplit(axis: axis) else { return }
        root = root.splittingLeaf(id: leafId, axis: axis, nextId: &nextLeafId)
        let newLeafId = nextLeafId - 1
        if let agentId {
            root = root.settingAgent(agentId, forLeafId: newLeafId)
        }
        focusedLeafId = newLeafId
    }

    /// Remove a pane. Returns the agent that occupied it, if any, so the caller can kill it.
    /// Returns `nil` for the whole result when this was the last pane — a workspace with no
    /// panes cannot exist, so the caller drops the workspace instead.
    mutating func closePane(leafId: Int) -> String? {
        let occupant = root.agentId(forLeafId: leafId)
        let sibling = root.siblingLeafId(of: leafId)
        guard let newRoot = root.removingLeaf(id: leafId) else { return occupant }
        root = newRoot
        if focusedLeafId == leafId {
            focusedLeafId = sibling ?? root.firstLeafId
        }
        return occupant
    }

    mutating func setAgent(_ agentId: String?, forLeafId leafId: Int) {
        root = root.settingAgent(agentId, forLeafId: leafId)
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

    /// Re-establish internal consistency after any structural edit.
    mutating func normalize() {
        let ids = root.allLeafIds
        if !ids.contains(focusedLeafId) {
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
    /// 1. Every live agent occupies exactly one pane of exactly one workspace.
    /// 2. No pane references an agent that is no longer in the manifest.
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

            // Drop panes whose agent died, and panes claiming an agent another workspace
            // already holds. A pane stored with no agent is a deliberate empty pane and stays.
            var stalePaneIds = Set<Int>()
            for leaf in workspace.root.leaves {
                guard let agentId = leaf.agentId else { continue }
                if containerByAgent[agentId] == nil || !claimed.insert(agentId).inserted {
                    stalePaneIds.insert(leaf.id)
                }
            }

            if !stalePaneIds.isEmpty {
                guard let pruned = workspace.root.removingLeaves(ids: stalePaneIds) else {
                    continue  // every pane went away — the workspace goes with it
                }
                workspace.root = pruned
            }

            // A workspace exists to hold agents. One that has lost them all is a ghost row.
            guard let primary = workspace.agentIds.first else { continue }

            workspace.container = containerByAgent[primary] ?? workspace.container
            workspace.normalize()
            result.append(workspace)
        }

        // Any live agent no pane claimed becomes its own single-pane workspace.
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
