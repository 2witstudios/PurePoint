import Foundation

/// A launch-time tap may precede project/status restoration. Keep only bounded local intent.
nonisolated struct AgentSignalPendingTaps {
    private var pending: [(key: String, received: Date)] = []
    mutating func enqueue(_ key: String, now: Date = Date()) {
        guard !key.isEmpty, key.utf8.count <= 8192 else { return }
        prune(now: now)
        guard !pending.contains(where: { $0.key == key }) else { return }
        pending.append((key, now))
        if pending.count > 16 { pending.removeFirst(pending.count - 16) }
    }
    mutating func next(availableKeys: Set<String>, now: Date = Date()) -> String? {
        prune(now: now)
        return pending.first(where: { availableKeys.contains($0.key) })?.key
    }
    mutating func complete(_ key: String) { pending.removeAll { $0.key == key } }
    private mutating func prune(now: Date) {
        pending.removeAll { now < $0.received || now.timeIntervalSince($0.received) >= 120 }
    }
}
