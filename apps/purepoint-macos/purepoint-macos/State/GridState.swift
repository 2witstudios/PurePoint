import Foundation
import Observation

@Observable
final class GridState {
    var root: PaneSplitNode
    var focusedLeafId: Int
    var isActive: Bool = false

    /// Instance-scoped ID counter (not static — avoids the ppg-cli global state bug).
    private var nextLeafId: Int

    init() {
        self.root = .leaf(id: 0, agentId: nil)
        self.focusedLeafId = 0
        self.nextLeafId = 1
    }

    // MARK: - Queries

    var leafCount: Int { root.leafCount }

    func canSplit(axis: PaneSplitNode.Axis) -> Bool {
        root.canSplit(axis: axis)
    }

    // MARK: - Mutations

    func setInitialAgent(_ agentId: String) {
        root = root.settingAgent(agentId, forLeafId: focusedLeafId)
        isActive = true
    }

    func splitFocused(axis: PaneSplitNode.Axis, agentId: String? = nil) {
        guard canSplit(axis: axis) else { return }
        root = root.splittingLeaf(id: focusedLeafId, axis: axis, nextId: &nextLeafId)
        // Focus the new pane
        let newLeafId = nextLeafId - 1
        if let agentId {
            root = root.settingAgent(agentId, forLeafId: newLeafId)
        }
        focusedLeafId = newLeafId
    }

    func closeFocused() {
        guard leafCount > 1 else {
            // Last pane — exit grid mode
            isActive = false
            return
        }

        guard let newRoot = root.removingLeaf(id: focusedLeafId) else { return }
        root = newRoot
        // Focus the first remaining leaf
        focusedLeafId = root.allLeafIds.first ?? 0
    }

    func setAgent(_ agentId: String, forLeafId leafId: Int) {
        root = root.settingAgent(agentId, forLeafId: leafId)
    }

    func moveFocus(direction: Direction) {
        let leaves = root.allLeafIds
        guard let currentIndex = leaves.firstIndex(of: focusedLeafId) else { return }

        let nextIndex: Int
        switch direction {
        case .next:
            nextIndex = (currentIndex + 1) % leaves.count
        case .previous:
            nextIndex = (currentIndex - 1 + leaves.count) % leaves.count
        }
        focusedLeafId = leaves[nextIndex]
    }

    enum Direction {
        case next, previous
    }

    // MARK: - Persistence

    func save(projectRoot: String) {
        GridLayoutPersistence.save(root, projectRoot: projectRoot)
    }

    func restore(projectRoot: String) {
        guard let restored = GridLayoutPersistence.load(projectRoot: projectRoot) else { return }
        root = restored
        nextLeafId = (root.allLeafIds.max() ?? 0) + 1
        focusedLeafId = root.allLeafIds.first ?? 0
        isActive = root.leafCount > 1 || root.allLeafIds.contains(where: { root.agentId(forLeafId: $0) != nil })
    }
}
