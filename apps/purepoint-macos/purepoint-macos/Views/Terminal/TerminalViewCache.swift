#if os(macOS)
import AppKit
#else
import UIKit
#endif
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
    /// The daemon owning an agent; remote agents attach through their machine's forward.
    @ObservationIgnored var clientForAgent: ((String) -> DaemonClient?)?
    /// Agents whose terminal printed output while it was off screen (a background tab),
    /// since it was last shown. Drives the tab bar's unseen-output dot.
    private(set) var unseenOutput: Set<String> = []
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
        if let client = clientForAgent?(agentId) { view.client = client }
        view.isAgentAlive = { [weak self] in self?.isAlive(agentId) ?? false }
        view.onBackgroundOutput = { [weak self] in self?.markUnseenOutput(agentId) }
        views[agent.id] = view
        return view
    }

    /// Record that an agent's terminal is on screen. Visibility itself is
    /// owned by each pane's container, so sibling grid panes stay shown.
    func show(agentId: String) {
        lastAccess[agentId] = Date()
        if unseenOutput.contains(agentId) { unseenOutput.remove(agentId) }
    }

    /// Writes only on the first chunk after the terminal was last seen, so a busy
    /// background agent does not invalidate observers on every chunk.
    private func markUnseenOutput(_ agentId: String) {
        if !unseenOutput.contains(agentId) { unseenOutput.insert(agentId) }
    }

    /// Check if a terminal exists for an agent.
    func hasView(for agentId: String) -> Bool {
        views[agentId] != nil
    }

    #if os(iOS)
    /// The terminal Focus Content moves to in `window`: the focused pane's, else the first
    /// terminal on screen in layout order. The Mac finds it by walking the window's `NSView`
    /// tree; here the workspace says which agents are on screen and the cache holds their views.
    func terminalToFocus(in workspace: Workspace?, window: UIWindow) -> TerminalPaneNSView? {
        guard let workspace else { return nil }
        for agentId in Self.focusOrder(of: workspace) {
            guard let view = views[agentId], view.terminal != nil, view.window === window,
                view.isShownInHierarchy
            else { continue }
            return view
        }
        return nil
    }

    /// The workspace's agents with the focused pane's first, then the rest in layout order.
    static func focusOrder(of workspace: Workspace) -> [String] {
        let focused = workspace.focusedAgentId
        return (focused.map { [$0] } ?? []) + workspace.agentIds.filter { $0 != focused }
    }
    #endif

    /// Remove a specific agent's terminal view immediately.
    func remove(agentId: String) {
        views[agentId]?.tearDown()
        views[agentId]?.removeFromSuperview()
        views.removeValue(forKey: agentId)
        lastAccess.removeValue(forKey: agentId)
        unseenOutput.remove(agentId)
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

#if os(iOS)
private extension UIView {
    /// Neither this view nor any ancestor is hidden, as the Mac's walk skips hidden subtrees.
    var isShownInHierarchy: Bool {
        var view: UIView? = self
        while let current = view {
            if current.isHidden { return false }
            view = current.superview
        }
        return true
    }
}
#endif
