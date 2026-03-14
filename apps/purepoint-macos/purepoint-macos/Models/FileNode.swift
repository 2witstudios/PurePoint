import Foundation

struct FileNode: Identifiable {
    let id: String  // relative path from worktree root
    let name: String
    let absolutePath: String
    let isDirectory: Bool
    var children: [FileNode]?  // nil = unexpanded directory
}

// MARK: - FileTreeNode (NSOutlineView reference-type wrapper)

class FileTreeNode {
    let name: String
    let absolutePath: String
    let relativePath: String
    let isDirectory: Bool
    var children: [FileTreeNode]

    init(name: String, absolutePath: String, relativePath: String, isDirectory: Bool, children: [FileTreeNode] = []) {
        self.name = name
        self.absolutePath = absolutePath
        self.relativePath = relativePath
        self.isDirectory = isDirectory
        self.children = children
    }

    var isExpandable: Bool { isDirectory }

    static func sorted(_ nodes: [FileTreeNode]) -> [FileTreeNode] {
        nodes.sorted { a, b in
            if a.isDirectory != b.isDirectory { return a.isDirectory }
            return a.name.localizedCaseInsensitiveCompare(b.name) == .orderedAscending
        }
    }
}
