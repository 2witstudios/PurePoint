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
        termView.translatesAutoresizingMaskIntoConstraints = false
        termView.isHidden = false
        container.addSubview(termView)

        NSLayoutConstraint.activate([
            termView.topAnchor.constraint(equalTo: container.topAnchor),
            termView.leadingAnchor.constraint(equalTo: container.leadingAnchor),
            termView.trailingAnchor.constraint(equalTo: container.trailingAnchor),
            termView.bottomAnchor.constraint(equalTo: container.bottomAnchor),
        ])

        return container
    }

    func updateNSView(_ nsView: NSView, context: Context) {
        let termView = viewCache.terminalView(for: agent)

        // Already showing the correct agent — nothing to do (besides focus check)
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
            termView.translatesAutoresizingMaskIntoConstraints = false
            nsView.addSubview(termView)
            NSLayoutConstraint.activate([
                termView.topAnchor.constraint(equalTo: nsView.topAnchor),
                termView.leadingAnchor.constraint(equalTo: nsView.leadingAnchor),
                termView.trailingAnchor.constraint(equalTo: nsView.trailingAnchor),
                termView.bottomAnchor.constraint(equalTo: nsView.bottomAnchor),
            ])
        }

        termView.isHidden = false
        viewCache.show(agentId: agent.id)

        if isFocused {
            makeTerminalFirstResponder(in: nsView)
        }
    }

    private func makeTerminalFirstResponder(in nsView: NSView) {
        DispatchQueue.main.async {
            guard let window = nsView.window else { return }
            // Find the TerminalPaneNSView and make its terminal the first responder
            for sub in nsView.subviews where !sub.isHidden {
                if let termPaneView = sub as? TerminalPaneNSView,
                   let tv = termPaneView.terminal?.terminalView {
                    window.makeFirstResponder(tv)
                    return
                }
            }
        }
    }
}
