import Foundation

/// Recursive binary split tree model for the pane grid.
/// Each node is either a leaf (single pane) or a split (two children with ratio).
indirect enum PaneSplitNode: Equatable {
    case leaf(id: Int, agentId: String?)
    case split(axis: Axis, ratio: CGFloat, first: PaneSplitNode, second: PaneSplitNode)

    enum Axis: String, Codable, Equatable {
        case horizontal // top/bottom
        case vertical   // left/right
    }

    // MARK: - Queries

    var allLeafIds: [Int] {
        switch self {
        case .leaf(let id, _): return [id]
        case .split(_, _, let first, let second): return first.allLeafIds + second.allLeafIds
        }
    }

    var leafCount: Int { allLeafIds.count }

    func agentId(forLeafId leafId: Int) -> String? {
        switch self {
        case .leaf(let id, let agentId): return id == leafId ? agentId : nil
        case .split(_, _, let first, let second):
            return first.agentId(forLeafId: leafId) ?? second.agentId(forLeafId: leafId)
        }
    }

    func containsAgent(_ agentId: String) -> Bool {
        switch self {
        case .leaf(_, let id): return id == agentId
        case .split(_, _, let first, let second):
            return first.containsAgent(agentId) || second.containsAgent(agentId)
        }
    }

    /// Find the leaf ID that contains a given agent.
    func leafId(forAgentId agentId: String) -> Int? {
        switch self {
        case .leaf(let id, let aId): return aId == agentId ? id : nil
        case .split(_, _, let first, let second):
            return first.leafId(forAgentId: agentId) ?? second.leafId(forAgentId: agentId)
        }
    }

    // MARK: - Structural Analysis (2x3 constraint)

    /// Count the number of "rows" (horizontal splits at the top level).
    var rowCount: Int {
        switch self {
        case .leaf: return 1
        case .split(.horizontal, _, let first, let second):
            return first.rowCount + second.rowCount
        case .split(.vertical, _, _, _): return 1
        }
    }

    /// Check if the tree can accommodate another split.
    /// Max 6 leaves (2 rows x 3 columns).
    func canSplit(axis: Axis) -> Bool {
        leafCount < 6
    }

    // MARK: - Mutations (return new tree)

    /// Split a leaf into two leaves along the given axis.
    func splittingLeaf(id targetId: Int, axis: Axis, ratio: CGFloat = 0.5, nextId: inout Int) -> PaneSplitNode {
        switch self {
        case .leaf(let id, let agentId) where id == targetId:
            let newId = nextId
            nextId += 1
            return .split(
                axis: axis,
                ratio: ratio,
                first: .leaf(id: id, agentId: agentId),
                second: .leaf(id: newId, agentId: nil)
            )
        case .leaf:
            return self
        case .split(let nodeAxis, let nodeRatio, let first, let second):
            return .split(
                axis: nodeAxis,
                ratio: nodeRatio,
                first: first.splittingLeaf(id: targetId, axis: axis, ratio: ratio, nextId: &nextId),
                second: second.splittingLeaf(id: targetId, axis: axis, ratio: ratio, nextId: &nextId)
            )
        }
    }

    /// Remove a leaf and collapse the tree.
    func removingLeaf(id targetId: Int) -> PaneSplitNode? {
        switch self {
        case .leaf(let id, _):
            return id == targetId ? nil : self
        case .split(let axis, let ratio, let first, let second):
            let newFirst = first.removingLeaf(id: targetId)
            let newSecond = second.removingLeaf(id: targetId)
            switch (newFirst, newSecond) {
            case (nil, nil): return nil
            case (nil, let remaining?): return remaining
            case (let remaining?, nil): return remaining
            case (let f?, let s?): return .split(axis: axis, ratio: ratio, first: f, second: s)
            }
        }
    }

    /// Update the ratio for a split identified by its first child's first leaf ID.
    func settingRatio(_ newRatio: CGFloat, forSplitIdentifiedByFirstLeaf targetLeafId: Int) -> PaneSplitNode {
        switch self {
        case .leaf:
            return self
        case .split(let axis, let ratio, let first, let second):
            if first.allLeafIds.first == targetLeafId {
                return .split(axis: axis, ratio: newRatio, first: first, second: second)
            }
            return .split(
                axis: axis,
                ratio: ratio,
                first: first.settingRatio(newRatio, forSplitIdentifiedByFirstLeaf: targetLeafId),
                second: second.settingRatio(newRatio, forSplitIdentifiedByFirstLeaf: targetLeafId)
            )
        }
    }

    /// Set the agent for a specific leaf.
    func settingAgent(_ agentId: String?, forLeafId targetId: Int) -> PaneSplitNode {
        switch self {
        case .leaf(let id, _) where id == targetId:
            return .leaf(id: id, agentId: agentId)
        case .leaf:
            return self
        case .split(let axis, let ratio, let first, let second):
            return .split(
                axis: axis,
                ratio: ratio,
                first: first.settingAgent(agentId, forLeafId: targetId),
                second: second.settingAgent(agentId, forLeafId: targetId)
            )
        }
    }

    // MARK: - Spatial Navigation

    /// Find the nearest leaf in the given direction from the specified leaf.
    /// Walks up the tree to find the nearest ancestor split matching the axis,
    /// then descends into the opposite child's nearest leaf.
    func findAdjacentLeaf(from leafId: Int, axis: Axis, forward: Bool) -> Int? {
        // Build path from root to the target leaf
        guard let path = pathTo(leafId: leafId) else { return nil }

        // Walk backwards through path to find nearest split matching the requested axis
        for i in stride(from: path.count - 1, through: 0, by: -1) {
            let step = path[i]
            guard case .split(let splitAxis, _, let first, let second) = step.node,
                  splitAxis == axis else { continue }

            let isInFirst = step.wentFirst
            // If forward and in first child, go to second's nearest leaf (front edge)
            // If backward and in second child, go to first's nearest leaf (back edge)
            if forward && isInFirst {
                return second.allLeafIds.first
            } else if !forward && !isInFirst {
                return first.allLeafIds.last
            }
        }
        return nil
    }

    /// Find the first leaf in the sibling subtree of the given leaf.
    /// Used for smarter focus after close — focuses the pane that fills the vacated space.
    func siblingLeafId(of leafId: Int) -> Int? {
        guard let path = pathTo(leafId: leafId), !path.isEmpty else { return nil }
        let lastStep = path.last!
        guard case .split(_, _, let first, let second) = lastStep.node else { return nil }
        let sibling = lastStep.wentFirst ? second : first
        return sibling.allLeafIds.first
    }

    private struct PathStep {
        let node: PaneSplitNode
        let wentFirst: Bool // true if we descended into 'first' child
    }

    /// Build the path from root to the leaf with the given ID.
    private func pathTo(leafId: Int) -> [PathStep]? {
        switch self {
        case .leaf(let id, _):
            return id == leafId ? [] : nil
        case .split(_, _, let first, let second):
            if let subpath = first.pathTo(leafId: leafId) {
                return [PathStep(node: self, wentFirst: true)] + subpath
            }
            if let subpath = second.pathTo(leafId: leafId) {
                return [PathStep(node: self, wentFirst: false)] + subpath
            }
            return nil
        }
    }

    // MARK: - Codable Persistence

    func toLayoutNode() -> GridLayoutNode {
        switch self {
        case .leaf(_, let agentId):
            return GridLayoutNode(type: .leaf, agentId: agentId, axis: nil, ratio: nil, first: nil, second: nil)
        case .split(let axis, let ratio, let first, let second):
            return GridLayoutNode(type: .split, agentId: nil, axis: axis, ratio: ratio, first: first.toLayoutNode(), second: second.toLayoutNode())
        }
    }

    static func fromLayoutNode(_ node: GridLayoutNode, nextId: inout Int) -> PaneSplitNode {
        switch node.type {
        case .leaf:
            let id = nextId
            nextId += 1
            return .leaf(id: id, agentId: node.agentId)
        case .split:
            guard let axis = node.axis, let ratio = node.ratio,
                  let first = node.first, let second = node.second else {
                let id = nextId
                nextId += 1
                return .leaf(id: id, agentId: nil)
            }
            return .split(
                axis: axis,
                ratio: ratio,
                first: fromLayoutNode(first, nextId: &nextId),
                second: fromLayoutNode(second, nextId: &nextId)
            )
        }
    }

    // Equatable conformance
    static func == (lhs: PaneSplitNode, rhs: PaneSplitNode) -> Bool {
        switch (lhs, rhs) {
        case (.leaf(let id1, let a1), .leaf(let id2, let a2)):
            return id1 == id2 && a1 == a2
        case (.split(let ax1, let r1, let f1, let s1), .split(let ax2, let r2, let f2, let s2)):
            return ax1 == ax2 && r1 == r2 && f1 == f2 && s1 == s2
        default:
            return false
        }
    }
}
