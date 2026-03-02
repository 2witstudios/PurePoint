import SwiftUI
import AppKit
import SwiftTerm

/// NSViewRepresentable bridge that creates a ScrollableTerminal connected to
/// the daemon via IPC for an agent. Handles lazy creation and deferred start
/// to prevent the 1-column PTY bug.
struct TerminalPaneView: NSViewRepresentable {
    let agent: AgentModel

    func makeNSView(context: Context) -> TerminalPaneNSView {
        TerminalPaneNSView(agent: agent)
    }

    func updateNSView(_ nsView: TerminalPaneNSView, context: Context) {
        // Agent identity doesn't change; status updates are cosmetic only
    }

    static func dismantleNSView(_ nsView: TerminalPaneNSView, coordinator: ()) {
        nsView.tearDown()
    }
}

/// The AppKit view that wraps a ScrollableTerminal and manages daemon attach lifecycle.
class TerminalPaneNSView: NSView {
    let agent: AgentModel
    private(set) var terminal: ScrollableTerminal?
    private var attachStarted = false
    private var terminalInstalled = false

    init(agent: AgentModel) {
        self.agent = agent
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = TerminalTheme.background.cgColor
    }

    required init?(coder: NSCoder) { fatalError() }

    private func ensureTerminal() {
        guard !terminalInstalled else { return }
        terminalInstalled = true

        let tv = ScrollableTerminal(frame: bounds)
        tv.wantsLayer = true
        tv.layer?.masksToBounds = true
        tv.translatesAutoresizingMaskIntoConstraints = false
        addSubview(tv)

        NSLayoutConstraint.activate([
            tv.topAnchor.constraint(equalTo: topAnchor),
            tv.leadingAnchor.constraint(equalTo: leadingAnchor),
            tv.trailingAnchor.constraint(equalTo: trailingAnchor),
            tv.bottomAnchor.constraint(equalTo: bottomAnchor),
        ])
        terminal = tv
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        // Terminal creation deferred to layout() to ensure non-zero bounds
    }

    override func layout() {
        super.layout()
        // Create terminal only after we have a real frame, preventing 0-column grids
        if window != nil && !terminalInstalled && bounds.width > 1 {
            ensureTerminal()
        }
        // Start daemon attach only after the first layout pass gives us a real frame.
        // Starting while frame is .zero causes SwiftTerm to report 1-column size.
        if let tv = terminal, !attachStarted, tv.bounds.width > 1 {
            attachStarted = true
            startDaemonAttach()
        }
    }

    private func startDaemonAttach() {
        guard let tv = terminal else { return }

        let session = DaemonAttachSession(agentId: agent.id, terminalView: tv.terminalView)
        tv.attachSession = session

        Task {
            await session.start()
        }
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        layer?.backgroundColor = TerminalTheme.background.cgColor
    }

    func tearDown() {
        terminal?.tearDown()
    }
}
