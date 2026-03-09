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

        return container
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        let termView = viewCache.terminalView(for: agent)
        context.coordinator.onFocus = onFocus

        // Already showing the correct agent — just ensure focus
        if termView.superview === nsView && !termView.isHidden {
            if isFocused {
                makeTerminalFirstResponder(in: nsView)
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
        viewCache.show(agentId: agent.id)

        // Always focus terminal when switching to a new agent
        makeTerminalFirstResponder(in: nsView)
    }

    private func makeTerminalFirstResponder(in container: NSView) {
        let paneView = viewCache.terminalView(for: agent)
        DispatchQueue.main.async {
            guard paneView.superview === container, !paneView.isHidden else { return }
            paneView.focusTerminal()
        }
    }
}
