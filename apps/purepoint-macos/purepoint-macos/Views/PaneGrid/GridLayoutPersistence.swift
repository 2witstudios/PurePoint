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

enum GridLayoutPersistence {
    private static func filePath(projectRoot: String) -> String {
        (projectRoot as NSString)
            .appendingPathComponent(".pu")
            .appending("/grid-layout.json")
    }

    static func save(_ node: PaneSplitNode, projectRoot: String) {
        let layout = node.toLayoutNode()
        guard let data = try? JSONEncoder().encode(layout) else { return }
        let path = filePath(projectRoot: projectRoot)
        try? data.write(to: URL(fileURLWithPath: path), options: .atomic)
    }

    static func load(projectRoot: String) -> PaneSplitNode? {
        let path = filePath(projectRoot: projectRoot)
        guard let data = try? Data(contentsOf: URL(fileURLWithPath: path)),
              let layout = try? JSONDecoder().decode(GridLayoutNode.self, from: data) else {
            return nil
        }
        var nextId = 0
        return PaneSplitNode.fromLayoutNode(layout, nextId: &nextId)
    }

    static func clear(projectRoot: String) {
        let path = filePath(projectRoot: projectRoot)
        try? FileManager.default.removeItem(atPath: path)
    }
}
