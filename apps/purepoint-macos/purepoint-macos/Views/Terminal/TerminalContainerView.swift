import SwiftUI

/// SwiftUI wrapper that hosts a cached terminal view for the selected agent.
struct TerminalContainerView: NSViewRepresentable {
    let agent: AgentModel
    var isFocused: Bool = false
    @Environment(TerminalViewCache.self) private var viewCache

    func makeNSView(context: Context) -> NSView {
        let container = NSView()
        container.wantsLayer = true
        container.layer?.backgroundColor = TerminalTheme.background.cgColor

        let termView = viewCache.terminalView(for: agent)
        termView.isHidden = false
        termView.pinToEdges(of: container)

        return container
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        let termView = viewCache.terminalView(for: agent)

        // Already showing the correct agent — just ensure focus
        if termView.superview === nsView && !termView.isHidden {
            if isFocused {
                makeTerminalFirstResponder()
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
        makeTerminalFirstResponder()
    }

    private func makeTerminalFirstResponder() {
        let paneView = viewCache.terminalView(for: agent)
        DispatchQueue.main.async {
            paneView.focusTerminal()
        }
    }
}
