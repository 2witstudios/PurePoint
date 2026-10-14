import Foundation

/// Which sidebar rows the user closed.
///
/// The outline view rebuilds its node objects on every data refresh and AppKit tracks
/// expansion by item identity, so what is open cannot live in the tree itself — a rebuild
/// would forget it and every row would snap back open. Keys are `SidebarNode.rowKey`, which
/// is parent-scoped, so two rows sharing a bare id never stand in for each other.
@MainActor
final class SidebarExpansionStore {
    /// Rows the user closed. Anything else is open — which is what a fresh launch shows —
    /// so this is the smaller set to keep, and it stays stable as rows come and go.
    private(set) var closedRowKeys: Set<String>

    private let defaults: UserDefaults
    private let key: String

    init(defaults: UserDefaults = .standard, key: String = SidebarExpansionStore.storageKey) {
        self.defaults = defaults
        self.key = key
        closedRowKeys = Set(defaults.stringArray(forKey: key) ?? [])
    }

    /// Records one row the user opened or closed.
    func setExpanded(_ rowKey: String, isExpanded: Bool) {
        var keys = closedRowKeys
        if isExpanded {
            keys.remove(rowKey)
        } else {
            keys.insert(rowKey)
        }
        record(keys)
    }

    /// Records the rows a tree currently shows closed, dropping keys that no longer exist
    /// so a removed row cannot resurrect itself when it comes back.
    func record(_ closedRowKeys: Set<String>) {
        guard closedRowKeys != self.closedRowKeys else { return }
        self.closedRowKeys = closedRowKeys
        defaults.set(closedRowKeys.sorted(), forKey: key)
    }

    nonisolated private static let storageKey = "PurePointSidebarExpansion"
}
