import SwiftUI
import AppKit
import SwiftTerm

/// NSViewRepresentable bridge that creates a ScrollableTerminal connected to a
/// tmux grouped session for an agent. Handles lazy creation and deferred start
/// to prevent the 1-column PTY bug.
struct TerminalPaneView: NSViewRepresentable {
    let agent: AgentModel
    let sessionName: String

    func makeNSView(context: Context) -> TerminalPaneNSView {
        TerminalPaneNSView(agent: agent, sessionName: sessionName)
    }

    func updateNSView(_ nsView: TerminalPaneNSView, context: Context) {
        // Agent identity doesn't change; status updates are cosmetic only
    }

    static func dismantleNSView(_ nsView: TerminalPaneNSView, coordinator: ()) {
        nsView.tearDown()
    }
}

/// The AppKit view that wraps a ScrollableTerminal and manages tmux session lifecycle.
class TerminalPaneNSView: NSView {
    let agent: AgentModel
    let sessionName: String
    private(set) var terminal: ScrollableTerminal?
    private var processStarted = false
    private var terminalInstalled = false

    init(agent: AgentModel, sessionName: String) {
        self.agent = agent
        self.sessionName = sessionName
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
        if window != nil && !terminalInstalled {
            ensureTerminal()
        }
    }

    override func layout() {
        super.layout()
        // Start tmux only after the first layout pass gives us a real frame.
        // Starting while frame is .zero causes SwiftTerm to open a 1-column PTY;
        // tmux then rewraps all scrollback to 1 column — never reflowed.
        if let tv = terminal, !processStarted, tv.bounds.width > 1 {
            processStarted = true
            startTmux()
        }
    }

    private func startTmux() {
        let target = agent.tmuxTarget

        // Parse session and window spec
        let windowSpec: String?
        if let colonIdx = target.firstIndex(of: ":") {
            let after = String(target[target.index(after: colonIdx)...])
            windowSpec = after.isEmpty ? nil : after
        } else {
            windowSpec = nil
        }

        // Random suffix to avoid session name collisions on fast re-selection
        let suffix = String((0..<4).map { _ in "abcdefghijklmnopqrstuvwxyz0123456789".randomElement()! })
        let viewSession = "\(sessionName)-view-\(agent.id)-\(suffix)"

        let cmd = TmuxCommandBuilder.groupedSessionCommand(
            tmuxTarget: target,
            viewSession: viewSession,
            windowSpec: windowSpec
        )

        terminal?.startProcess(
            executable: "/bin/zsh",
            args: ["-c", cmd],
            environment: nil,
            execName: "zsh"
        )
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        layer?.backgroundColor = TerminalTheme.background.cgColor
    }

    func tearDown() {
        terminal?.tearDown()
    }
}
