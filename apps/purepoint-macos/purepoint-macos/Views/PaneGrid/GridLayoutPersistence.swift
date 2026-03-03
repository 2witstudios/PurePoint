import Foundation

/// Codable representation of a PaneSplitNode tree for disk persistence.
/// Uses a class to break the value-type recursion for Codable.
final class GridLayoutNode: Codable, Sendable {
    enum NodeType: String, Codable {
        case leaf
        case split
    }

    let type: NodeType
    let agentId: String?
    let axis: PaneSplitNode.Axis?
    let ratio: CGFloat?
    let first: GridLayoutNode?
    let second: GridLayoutNode?

    init(type: NodeType, agentId: String?, axis: PaneSplitNode.Axis?, ratio: CGFloat?, first: GridLayoutNode?, second: GridLayoutNode?) {
        self.type = type
        self.agentId = agentId
        self.axis = axis
        self.ratio = ratio
        self.first = first
        self.second = second
    }
}

/// Top-level persisted grid state including ownership.
struct PersistedGridLayout: Codable {
    let ownerAgentId: String?
    let tree: GridLayoutNode
}

/// Result of loading a persisted grid.
struct RestoredGrid {
    let root: PaneSplitNode
    let ownerAgentId: String?
}

enum GridLayoutPersistence {
    private static func filePath(projectRoot: String) -> String {
        (projectRoot as NSString)
            .appendingPathComponent(".pu")
            .appending("/grid-layout.json")
    }

    static func save(_ node: PaneSplitNode, ownerAgentId: String?, projectRoot: String) {
        let persisted = PersistedGridLayout(ownerAgentId: ownerAgentId, tree: node.toLayoutNode())
        guard let data = try? JSONEncoder().encode(persisted) else { return }
        let path = filePath(projectRoot: projectRoot)
        try? data.write(to: URL(fileURLWithPath: path), options: .atomic)
    }

    static func load(projectRoot: String) -> RestoredGrid? {
        let path = filePath(projectRoot: projectRoot)
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)) else { return nil }

        // Try new format first (with ownerAgentId)
        if let persisted = try? JSONDecoder().decode(PersistedGridLayout.self, from: data) {
            var nextId = 0
            let root = PaneSplitNode.fromLayoutNode(persisted.tree, nextId: &nextId)
            return RestoredGrid(root: root, ownerAgentId: persisted.ownerAgentId)
        }

        // Fall back to legacy format (bare GridLayoutNode)
        if let layout = try? JSONDecoder().decode(GridLayoutNode.self, from: data) {
            var nextId = 0
            let root = PaneSplitNode.fromLayoutNode(layout, nextId: &nextId)
            return RestoredGrid(root: root, ownerAgentId: nil)
        }

        return nil
    }

    static func clear(projectRoot: String) {
        let path = filePath(projectRoot: projectRoot)
        try? FileManager.default.removeItem(atPath: path)
    }
}
