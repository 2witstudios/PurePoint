#if os(iOS)
import SwiftUI
import UIKit

/// iOS twin of the Mac terminal pane representable.
struct TerminalPaneView: UIViewRepresentable {
    let agent: AgentModel

    func makeUIView(context: Context) -> TerminalPaneNSView {
        TerminalPaneNSView(agent: agent)
    }

    func updateUIView(_ uiView: TerminalPaneNSView, context: Context) {}

    static func dismantleUIView(_ uiView: TerminalPaneNSView, coordinator: ()) {
        uiView.tearDown()
    }
}

/// Placeholder twin of the Mac terminal pane, with the same properties and lifecycle calls
/// `TerminalViewCache` and the shell use. It never attaches to the daemon; it shows a label
/// until the SwiftTerm terminal lands (IPAD-6.1).
class TerminalPaneNSView: UIView {
    let agent: AgentModel
    var onMouseDown: (() -> Void)?
    /// Live status lookup. `agent` is a snapshot from creation; set by `TerminalViewCache`.
    var isAgentAlive: () -> Bool = { true }
    /// Called when output arrives while this terminal is not on screen; set by `TerminalViewCache`.
    var onBackgroundOutput: (() -> Void)?
    /// The agent's daemon. Set by `TerminalViewCache`; on iPad every agent is remote.
    var client = DaemonClient() {
        didSet {
            terminal?.acceptsFileDrops = false
            terminal?.usesRemoteHost = true
        }
    }
    private(set) var terminal: ScrollableTerminal?
    private(set) var isAgentGone = false

    init(agent: AgentModel) {
        self.agent = agent
        super.init(frame: .zero)
        backgroundColor = TerminalTheme.background
        NotYetOnIPad.addLabel("TerminalPaneNSView", to: self)
    }

    required init?(coder: NSCoder) { fatalError() }

    func reconnectIfNeeded() {}

    override var canBecomeFirstResponder: Bool { true }

    func focusTerminal() {
        guard !InlineRenameFocus.isActive else { return }
        becomeFirstResponder()
    }

    func tearDown() {
        terminal?.tearDown()
    }
}
#endif
