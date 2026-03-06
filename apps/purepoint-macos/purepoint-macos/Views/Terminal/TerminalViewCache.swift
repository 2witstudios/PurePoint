import AppKit
import Foundation

/// Caches terminal NSViews by agent ID. Uses hide/show pattern — never
/// creates/destroys views on switch. LRU eviction removes completed agent
/// views after 30s idle.
@Observable
final class TerminalViewCache {
    @ObservationIgnored private var views: [String: TerminalPaneNSView] = [:]
    @ObservationIgnored private var lastAccess: [String: Date] = [:]
    @ObservationIgnored private var visibleAgentIds: Set<String> = []
    @ObservationIgnored private var evictionTimer: Timer?
    private static let evictionDelay: TimeInterval = 30

    init() {
        evictionTimer = Timer.scheduledTimer(withTimeInterval: 20, repeats: true) { [weak self] _ in
            guard let self else { return }
            self.evictStale(visibleIds: self.visibleAgentIds)
        }
    }

    deinit {
        evictionTimer?.invalidate()
        for (_, view) in views {
            view.tearDown()
        }
    }

    /// Get or create a terminal view for an agent.
    func terminalView(for agent: AgentModel) -> TerminalPaneNSView {
        lastAccess[agent.id] = Date()

        if let existing = views[agent.id] {
            existing.reconnectIfNeeded()
            return existing
        }

        let view = TerminalPaneNSView(agent: agent)
        views[agent.id] = view
        return view
    }

    /// Show terminals for multiple agents (grid mode), hiding all others.
    func showMultiple(agentIds: Set<String>) {
        visibleAgentIds = agentIds
        let now = Date()
        for id in agentIds {
            lastAccess[id] = now
        }
        for (id, view) in views {
            view.isHidden = !agentIds.contains(id)
        }
    }

    /// Show the terminal for a single agent, hiding all others.
    func show(agentId: String) {
        showMultiple(agentIds: [agentId])
    }

    /// Hide all terminal views.
    func hideAll() {
        for (_, view) in views {
            view.isHidden = true
        }
    }

    /// Check if a terminal exists for an agent.
    func hasView(for agentId: String) -> Bool {
        views[agentId] != nil
    }

    /// Evict terminal views for completed/killed/failed agents that haven't
    /// been viewed in evictionDelay seconds and are not currently visible.
    private func evictStale(visibleIds: Set<String> = []) {
        let now = Date()
        var toEvict: [String] = []

        for (id, view) in views {
            guard !visibleIds.contains(id) else { continue }
            guard !view.agent.status.isAlive else { continue }
            guard let access = lastAccess[id],
                  now.timeIntervalSince(access) > Self.evictionDelay else { continue }
            toEvict.append(id)
        }

        for id in toEvict {
            views[id]?.tearDown()
            views[id]?.removeFromSuperview()
            views.removeValue(forKey: id)
            lastAccess.removeValue(forKey: id)
        }
    }
}
