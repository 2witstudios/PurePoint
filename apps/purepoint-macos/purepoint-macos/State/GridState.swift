import Foundation
import Observation

@Observable
final class GridState {
    var root: PaneSplitNode
    var focusedLeafId: Int
    var isActive: Bool = false

    /// Project root for persistence. Set when grid is restored or first used.
    var projectRoot: String?

    /// Instance-scoped ID counter (not static — avoids the ppg-cli global state bug).
    private var nextLeafId: Int
    @ObservationIgnored private var saveWorkItem: DispatchWorkItem?

    init() {
        self.root = .leaf(id: 0, agentId: nil)
        self.focusedLeafId = 0
        self.nextLeafId = 1
    }

    /// The agent ID that owns this grid. Shown in sidebar; clicking restores grid.
    var ownerAgentId: String?

    /// Set after a UI-initiated split to auto-open the command palette in the new pane.
    var pendingPaletteLeafId: Int?

    /// Called when a non-owner pane is closed. Wired to ProjectState.removeAndKillAgent().
    @ObservationIgnored var onCloseAgent: ((String) -> Void)?

    /// Leaf IDs with in-flight spawn requests. Used by ProjectState.refresh() to eagerly
    /// assign new agents to grid leaves before they appear in rootAgents (sidebar leak fix).
    var pendingSpawnLeafIds: Set<Int> = []

    // MARK: - Queries

    var leafCount: Int { root.leafCount }

    func canSplit(axis: PaneSplitNode.Axis) -> Bool {
        root.canSplit(axis: axis)
    }

    /// Agent IDs in grid panes OTHER than the owner. Hidden from sidebar.
    var childAgentIds: Set<String> {
        let all = Set(root.allLeafIds.compactMap { root.agentId(forLeafId: $0) })
        guard let owner = ownerAgentId else { return all }
        return all.subtracting([owner])
    }

    // MARK: - Mutations

    /// Enter grid mode from single-pane view. The current agent becomes the grid owner.
    func enterGridMode(agentId: String, axis: PaneSplitNode.Axis) {
        root = .leaf(id: 0, agentId: agentId)
        focusedLeafId = 0
        nextLeafId = 1
        ownerAgentId = agentId
        splitFocused(axis: axis)  // Creates second empty pane
        isActive = true
        scheduleSave()
    }

    /// Exit grid mode entirely. Clears owner and resets to single leaf.
    func exitGrid() {
        isActive = false
        ownerAgentId = nil
        root = .leaf(id: 0, agentId: nil)
        nextLeafId = 1
        scheduleSave()
    }

    /// Suspend grid (hide but preserve state). Called when user navigates away.
    func suspend() {
        isActive = false
        scheduleSave()
    }

    /// Smart deactivation: exit fully if 1 pane (nothing to restore), suspend if multi-pane.
    func deactivate() {
        if root.leafCount <= 1 {
            exitGrid()
        } else {
            suspend()
        }
    }

    /// Whether a suspended grid can be restored (has saved state with an owner).
    var canRestore: Bool {
        ownerAgentId != nil && !isActive && root.leafCount > 1
    }

    /// Restore a suspended grid if the given agent is the owner. Returns true if restored.
    func restoreIfOwner(_ agentId: String) -> Bool {
        guard ownerAgentId == agentId, !isActive, root.leafCount > 1 else { return false }
        isActive = true
        return true
    }

    func splitFocused(axis: PaneSplitNode.Axis, agentId: String? = nil) {
        guard canSplit(axis: axis) else { return }
        root = root.splittingLeaf(id: focusedLeafId, axis: axis, nextId: &nextLeafId)
        let newLeafId = nextLeafId - 1
        if let agentId {
            root = root.settingAgent(agentId, forLeafId: newLeafId)
        }
        focusedLeafId = newLeafId
        scheduleSave()
    }

    func closeFocused() {
        guard leafCount > 1 else {
            exitGrid()  // Remote command edge case: can't close the only pane
            return
        }

        let closingId = focusedLeafId
        let closingAgentId = root.agentId(forLeafId: closingId)
        let siblingId = root.siblingLeafId(of: closingId)

        guard let newRoot = root.removingLeaf(id: closingId) else { return }
        root = newRoot
        focusedLeafId = siblingId ?? root.allLeafIds.first ?? 0

        if let agentId = closingAgentId {
            if agentId == ownerAgentId {
                // Closing the owner pane — promote surviving agent to owner.
                // Old owner stays alive (visible in sidebar as regular agent).
                // If no surviving agent (empty pane), exit grid entirely.
                guard let newOwner = root.agentId(forLeafId: focusedLeafId) else {
                    exitGrid()
                    return
                }
                ownerAgentId = newOwner
            } else {
                // Closing a child pane — kill the child agent
                onCloseAgent?(agentId)
            }
        }

        // Stay in grid mode with 1 pane — no view swap, no blank
        scheduleSave()
    }

    func setAgent(_ agentId: String, forLeafId leafId: Int) {
        root = root.settingAgent(agentId, forLeafId: leafId)
        scheduleSave()
    }

    func updateRatio(_ newRatio: CGFloat, forSplitIdentifiedByFirstLeaf leafId: Int) {
        root = root.settingRatio(newRatio, forSplitIdentifiedByFirstLeaf: leafId)
        scheduleSave()
    }

    // MARK: - Spatial Focus Navigation

    enum Direction {
        case up, down, left, right
    }

    func moveFocus(direction: Direction) {
        let (axis, forward): (PaneSplitNode.Axis, Bool) =
            switch direction {
            case .up: (.horizontal, false)
            case .down: (.horizontal, true)
            case .left: (.vertical, false)
            case .right: (.vertical, true)
            }

        if let adjacent = root.findAdjacentLeaf(from: focusedLeafId, axis: axis, forward: forward) {
            focusedLeafId = adjacent
        }
    }

    // MARK: - Remote Command Handling

    func handleRemoteCommand(_ command: GridCommandPayload) {
        switch command {
        case .split(let leafId, let axisStr):
            if let leafId { focusedLeafId = leafId }
            let axis: PaneSplitNode.Axis = axisStr == "h" ? .horizontal : .vertical
            splitFocused(axis: axis)
        case .close(let leafId):
            if let leafId { focusedLeafId = leafId }
            closeFocused()
        case .focus(let leafId, let directionStr):
            if let leafId {
                focusedLeafId = leafId
            } else if let directionStr {
                let dir: Direction =
                    switch directionStr {
                    case "up": .up
                    case "down": .down
                    case "left": .left
                    default: .right
                    }
                moveFocus(direction: dir)
            }
        case .setAgent(let leafId, let agentId):
            setAgent(agentId, forLeafId: Int(leafId))
        case .getLayout:
            break  // Daemon handles directly from file
        }
    }

    // MARK: - Persistence

    func save(projectRoot: String) {
        GridLayoutPersistence.save(root, ownerAgentId: ownerAgentId, projectRoot: projectRoot)
    }

    func restore(projectRoot: String, validAgentIds: Set<String> = []) {
        self.projectRoot = projectRoot
        guard let persisted = GridLayoutPersistence.load(projectRoot: projectRoot) else { return }
        root = persisted.root
        ownerAgentId = persisted.ownerAgentId
        nextLeafId = (root.allLeafIds.max() ?? 0) + 1
        focusedLeafId = root.allLeafIds.first ?? 0

        // Only activate if the owner still exists (or we haven't loaded agents yet)
        if let owner = ownerAgentId, !validAgentIds.isEmpty, !validAgentIds.contains(owner) {
            ownerAgentId = nil
            isActive = false
        } else {
            isActive = ownerAgentId != nil
        }
    }

    /// Debounced auto-save (1 second coalesce).
    func scheduleSave() {
        saveWorkItem?.cancel()
        let root = self.root
        let projectRoot = self.projectRoot
        let ownerAgentId = self.ownerAgentId
        let item = DispatchWorkItem {
            guard let projectRoot else { return }
            GridLayoutPersistence.save(root, ownerAgentId: ownerAgentId, projectRoot: projectRoot)
        }
        saveWorkItem = item
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.0, execute: item)
    }
}
