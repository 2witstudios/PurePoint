import AppKit
import Foundation

/// Caches terminal NSViews by agent ID. Uses hide/show pattern — never
/// creates/destroys views on switch. LRU eviction removes completed agent
/// views after 30s idle.
@Observable
final class TerminalViewCache {
    @ObservationIgnored private var views: [String: TerminalPaneNSView] = [:]
    @ObservationIgnored private var lastAccess: [String: Date] = [:]
    @ObservationIgnored private var visibleAgentId: String?
    @ObservationIgnored private var evictionTimer: Timer?
    private static let evictionDelay: TimeInterval = 30

    init() {
        evictionTimer = Timer.scheduledTimer(withTimeInterval: 20, repeats: true) { [weak self] _ in
            self?.evictStale(visibleId: self?.visibleAgentId)
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
            return existing
        }

        let view = TerminalPaneNSView(agent: agent)
        views[agent.id] = view
        return view
    }

    /// Show the terminal for a given agent, hiding all others.
    func show(agentId: String) {
        visibleAgentId = agentId
        lastAccess[agentId] = Date()
        for (id, view) in views {
            view.isHidden = (id != agentId)
        }
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

    /// Remove views for agents no longer in the manifest.
    func clearStale(activeIds: Set<String>) {
        let staleIds = Set(views.keys).subtracting(activeIds)
        for id in staleIds {
            views[id]?.tearDown()
            views[id]?.removeFromSuperview()
            views.removeValue(forKey: id)
            lastAccess.removeValue(forKey: id)
        }
    }

    /// Evict terminal views for completed/killed/failed agents that haven't
    /// been viewed in evictionDelay seconds and are not currently visible.
    private func evictStale(visibleId: String? = nil) {
        let now = Date()
        var toEvict: [String] = []

        for (id, view) in views {
            guard id != visibleId else { continue }
            guard view.agent.status.isTerminal else { continue }
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
