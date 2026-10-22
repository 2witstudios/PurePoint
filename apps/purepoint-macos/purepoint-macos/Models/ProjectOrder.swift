import Foundation

/// The sidebar's project order. Each machine's daemon saves the order of its projects so
/// every device lists them the same way; these helpers apply it to the rows on screen.
nonisolated enum ProjectOrder {
    /// `items` with the one rooted at `root` moved to `index`, or nil when nothing moves.
    static func moving<T>(
        _ root: String, to index: Int, in items: [T], root rootOf: (T) -> String
    ) -> [T]? {
        guard let from = items.firstIndex(where: { rootOf($0) == root }) else { return nil }
        var next = items
        let item = next.remove(at: from)
        let to = min(max(index, 0), next.count)
        guard to != from else { return nil }
        next.insert(item, at: to)
        return next
    }

    /// `items` sorted into the order of `roots`. Items `roots` doesn't list keep their relative
    /// order after the ones it does.
    static func sorted<T>(_ items: [T], by roots: [String], root rootOf: (T) -> String) -> [T] {
        var rank: [String: Int] = [:]
        for (index, root) in roots.enumerated() where rank[root] == nil {
            rank[root] = index
        }
        return items.enumerated().sorted { left, right in
            let l = rank[rootOf(left.element)] ?? Int.max
            let r = rank[rootOf(right.element)] ?? Int.max
            return l != r ? l < r : left.offset < right.offset
        }.map(\.element)
    }
}

@MainActor
extension ProjectOrder {
    static func moving(_ root: String, to index: Int, in projects: [ProjectState]) -> [ProjectState]? {
        moving(root, to: index, in: projects, root: { $0.projectRoot })
    }
}
