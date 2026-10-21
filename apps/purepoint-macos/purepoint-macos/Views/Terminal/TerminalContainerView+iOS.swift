#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac container that hosts a cached terminal view for an agent. It shows the
/// cache's `TerminalPaneNSView` the way the Mac does; focus taps arrive through SwiftUI.
struct TerminalContainerView: UIViewRepresentable {
    let agent: AgentModel
    var isFocused: Bool = false
    var onFocus: (() -> Void)? = nil
    @Environment(TerminalViewCache.self) private var viewCache

    func makeUIView(context: Context) -> UIView {
        let container = UIView()
        container.backgroundColor = TerminalTheme.background
        show(viewCache.terminalView(for: agent), in: container)
        return container
    }

    func updateUIView(_ uiView: UIView, context: Context) {
        let termView = viewCache.terminalView(for: agent)
        guard termView.superview !== uiView || termView.isHidden else { return }
        for sub in uiView.subviews { sub.isHidden = true }
        show(termView, in: uiView)
    }

    private func show(_ termView: TerminalPaneNSView, in container: UIView) {
        if termView.superview !== container {
            termView.removeFromSuperview()
            termView.frame = container.bounds
            termView.autoresizingMask = [.flexibleWidth, .flexibleHeight]
            container.addSubview(termView)
        }
        termView.isHidden = false
        termView.onMouseDown = onFocus
        // Deferred: this runs inside a SwiftUI update, and the cache is observed by the tab bar.
        let cache = viewCache
        let agentId = agent.id
        DispatchQueue.main.async { cache.show(agentId: agentId) }
    }
}
#endif
