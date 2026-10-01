import AppKit
import Foundation

/// Caches terminal NSViews by agent ID. Uses hide/show pattern — never
/// creates/destroys views on switch. LRU eviction removes completed agent
/// views after 30s idle.
@Observable
final class TerminalViewCache {
    @ObservationIgnored private var views: [String: TerminalPaneNSView] = [:]
    @ObservationIgnored private var lastAccess: [String: Date] = [:]
    @ObservationIgnored private var evictionTimer: Timer?
    /// Current status of an agent, or nil if the app no longer knows it.
    /// Set by the app; a view's own `agent` is a stale creation-time snapshot.
    @ObservationIgnored var agentStatus: ((String) -> AgentStatus?)?
    private static let evictionDelay: TimeInterval = 30

    init() {
        evictionTimer = Timer.scheduledTimer(withTimeInterval: 20, repeats: true) { [weak self] _ in
            self?.evictStale()
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
        let agentId = agent.id
        view.isAgentAlive = { [weak self] in self?.isAlive(agentId) ?? false }
        views[agent.id] = view
        return view
    }

    /// Record that an agent's terminal is on screen. Visibility itself is
    /// owned by each pane's container, so sibling grid panes stay shown.
    func show(agentId: String) {
        lastAccess[agentId] = Date()
    }

    /// Check if a terminal exists for an agent.
    func hasView(for agentId: String) -> Bool {
        views[agentId] != nil
    }

    /// Remove a specific agent's terminal view immediately.
    func remove(agentId: String) {
        views[agentId]?.tearDown()
        views[agentId]?.removeFromSuperview()
        views.removeValue(forKey: agentId)
        lastAccess.removeValue(forKey: agentId)
    }

    /// Evict terminal views for completed/killed/failed agents that haven't
    /// been viewed in evictionDelay seconds and are not currently visible.
    private func isAlive(_ agentId: String) -> Bool {
        agentStatus?(agentId)?.isAlive ?? false
    }

    private func evictStale() {
        let now = Date()
        var toEvict: [String] = []

        for (id, view) in views {
            // Agent confirmed gone by daemon — evict immediately
            if view.isAgentGone {
                toEvict.append(id)
                continue
            }
            // Still on screen in some pane
            if view.window != nil && !view.isHidden { continue }
            guard !isAlive(id) else { continue }
            guard let access = lastAccess[id],
                now.timeIntervalSince(access) > Self.evictionDelay
            else { continue }
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
