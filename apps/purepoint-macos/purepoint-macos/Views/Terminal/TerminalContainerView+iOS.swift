#if os(iOS)
import SwiftUI
import UIKit

/// SwiftUI wrapper that hosts a cached terminal view for the selected agent.
struct TerminalContainerView: UIViewRepresentable {
    let agent: AgentModel
    var isFocused: Bool = false
    var onFocus: (() -> Void)? = nil
    @Environment(TerminalViewCache.self) private var viewCache

    class Coordinator {
        var onFocus: (() -> Void)?
        weak var container: UIView?
        /// Last `isFocused` seen, so focus is only taken when it changes to true.
        /// Re-taking it on every update stole focus from the sidebar and search
        /// fields whenever any agent's status changed.
        var wasFocused = false
        private var recognizer: TouchDownRecognizer?

        func stopMonitor() {
            if let recognizer {
                recognizer.view?.removeGestureRecognizer(recognizer)
                self.recognizer = nil
            }
        }

        /// A touch or click anywhere in the pane focuses it, without taking the gesture
        /// from the terminal: the iOS form of the Mac's mouse-down monitor.
        func startMonitor() {
            stopMonitor()
            guard let container else { return }
            let recognizer = TouchDownRecognizer { [weak self] _, _ in
                guard let self, let container = self.container, !container.isHidden else { return }
                self.onFocus?()
            }
            container.addGestureRecognizer(recognizer)
            self.recognizer = recognizer
        }
    }

    static func dismantleUIView(_ uiView: UIView, coordinator: Coordinator) {
        coordinator.stopMonitor()
        coordinator.container = nil
    }

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeUIView(context: Context) -> UIView {
        let container = UIView()
        container.backgroundColor = TerminalTheme.background

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

    func updateUIView(_ uiView: UIView, context: Context) {
        let termView = viewCache.terminalView(for: agent)
        context.coordinator.onFocus = onFocus
        let becameFocused = isFocused && !context.coordinator.wasFocused
        context.coordinator.wasFocused = isFocused

        // Already showing the correct agent — just take focus if it just moved here
        if termView.superview === uiView && !termView.isHidden {
            if becameFocused {
                makeTerminalFirstResponder(termView, in: uiView)
            }
            return
        }

        // Hide all current subviews
        for sub in uiView.subviews {
            sub.isHidden = true
        }

        // Add if not already a child, then show
        if termView.superview !== uiView {
            termView.pinToEdges(of: uiView)
        }

        termView.isHidden = false
        let cache = viewCache
        let agentId = agent.id
        DispatchQueue.main.async { cache.show(agentId: agentId) }

        // Always focus terminal when switching to a new agent
        makeTerminalFirstResponder(termView, in: uiView)
    }

    private func makeTerminalFirstResponder(_ paneView: TerminalPaneNSView, in container: UIView) {
        DispatchQueue.main.async {
            guard paneView.superview === container, !paneView.isHidden else { return }
            paneView.focusTerminal()
        }
    }
}
#endif
