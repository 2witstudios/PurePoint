import SwiftUI

/// SwiftUI wrapper that hosts a cached terminal view for the selected agent.
struct TerminalContainerView: NSViewRepresentable {
    let agent: AgentModel
    var isFocused: Bool = false
    var onFocus: (() -> Void)? = nil
    @Environment(TerminalViewCache.self) private var viewCache

    class Coordinator {
        var onFocus: (() -> Void)?
        weak var container: NSView?
        /// Last `isFocused` seen, so focus is only taken when it changes to true.
        /// Re-taking it on every update stole focus from the sidebar and search
        /// fields whenever any agent's status changed.
        var wasFocused = false
        private var monitor: Any?

        func stopMonitor() {
            if let monitor {
                NSEvent.removeMonitor(monitor)
                self.monitor = nil
            }
        }

        func startMonitor() {
            stopMonitor()
            monitor = NSEvent.addLocalMonitorForEvents(matching: .leftMouseDown) { [weak self] event in
                guard let self, let container = self.container else { return event }
                guard event.window === container.window, !container.isHidden else { return event }
                let point = container.convert(event.locationInWindow, from: nil)
                if container.bounds.contains(point) {
                    self.onFocus?()
                }
                return event
            }
        }

        deinit {
            stopMonitor()
        }
    }

    static func dismantleNSView(_ nsView: NSView, coordinator: Coordinator) {
        coordinator.stopMonitor()
        coordinator.container = nil
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeNSView(context: Context) -> NSView {
        let container = NSView()
        container.wantsLayer = true
        container.layer?.backgroundColor = TerminalTheme.background.cgColor

        let termView = viewCache.terminalView(for: agent)
        termView.isHidden = false
        termView.pinToEdges(of: container)

        context.coordinator.container = container
        context.coordinator.onFocus = onFocus
        context.coordinator.startMonitor()

        // A tab coming back on screen has now been seen. Deferred: this runs inside a
        // SwiftUI update, and the cache is observed by the tab bar.
        let cache = viewCache
        let agentId = agent.id
        DispatchQueue.main.async { cache.show(agentId: agentId) }

        return container
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        let termView = viewCache.terminalView(for: agent)
        context.coordinator.onFocus = onFocus
        let becameFocused = isFocused && !context.coordinator.wasFocused
        context.coordinator.wasFocused = isFocused

        // Already showing the correct agent — just take focus if it just moved here
        if termView.superview === nsView && !termView.isHidden {
            if becameFocused {
                makeTerminalFirstResponder(termView, in: nsView)
            }
            return
        }

        // Hide all current subviews
        for sub in nsView.subviews {
            sub.isHidden = true
        }

        // Add if not already a child, then show
        if termView.superview !== nsView {
            termView.pinToEdges(of: nsView)
        }

        termView.isHidden = false
        let cache = viewCache
        let agentId = agent.id
        DispatchQueue.main.async { cache.show(agentId: agentId) }

        // Always focus terminal when switching to a new agent
        makeTerminalFirstResponder(termView, in: nsView)
    }

    private func makeTerminalFirstResponder(_ paneView: TerminalPaneNSView, in container: NSView) {
        DispatchQueue.main.async {
            guard paneView.superview === container, !paneView.isHidden else { return }
            paneView.focusTerminal()
        }
    }
}
