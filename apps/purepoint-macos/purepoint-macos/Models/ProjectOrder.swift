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
    /// order after the ones it does. Roots compare by their resolved path, since the daemon
    /// reports canonical roots and a project may have been opened through a symlink.
    static func sorted<T>(_ items: [T], by roots: [String], root rootOf: (T) -> String) -> [T] {
        var rank: [String: Int] = [:]
        for (index, root) in roots.enumerated() where rank[canonical(root)] == nil {
            rank[canonical(root)] = index
        }
        return items.enumerated().sorted { left, right in
            let l = rank[canonical(rootOf(left.element))] ?? Int.max
            let r = rank[canonical(rootOf(right.element))] ?? Int.max
            return l != r ? l < r : left.offset < right.offset
        }.map(\.element)
    }

    /// The path with symlinks resolved, as the daemon stores it.
    static func canonical(_ path: String) -> String {
        // `realpath`, not `resolvingSymlinksInPath`: Foundation strips `/private`, the daemon doesn't.
        guard let resolved = realpath(path, nil) else { return path }
        defer { free(resolved) }
        return String(cString: resolved)
    }
}

@MainActor
extension ProjectOrder {
    static func moving(_ root: String, to index: Int, in projects: [ProjectState]) -> [ProjectState]? {
        moving(root, to: index, in: projects, root: { $0.projectRoot })
    }

    static func sorted(_ projects: [ProjectState], by roots: [String]) -> [ProjectState] {
        sorted(projects, by: roots, root: { $0.projectRoot })
    }
}
