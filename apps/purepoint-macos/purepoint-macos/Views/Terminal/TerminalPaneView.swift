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
    private var attachTask: Task<Void, Never>?
    private var attachStarted = false
    private var isAttachDone = false
    private var terminalInstalled = false
    private var heartbeatTimer: Timer?
    private var installDebounce: DispatchWorkItem?

    init(agent: AgentModel) {
        self.agent = agent
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = TerminalTheme.background.cgColor
    }

    required init?(coder: NSCoder) { fatalError() }

    private func scheduleTerminalInstall() {
        installDebounce?.cancel()
        let item = DispatchWorkItem { [weak self] in
            guard let self, !self.terminalInstalled else { return }
            self.ensureTerminal()
            self.needsLayout = true
        }
        installDebounce = item
        DispatchQueue.main.asyncAfter(deadline: .now() + .milliseconds(100), execute: item)
    }

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
        tv.terminalView.hideCursor(source: tv.terminalView.getTerminal())
        terminal = tv
    }

    override func viewDidMoveToWindow() {
        super.viewDidMoveToWindow()
        if window != nil {
            needsLayout = true
            // Force terminal to redraw after being re-parented between containers.
            // Without this, closing a grid pane (2→1) leaves the surviving terminal blank
            // because the AnyView type change in PaneGridView destroys and recreates the
            // SwiftUI view hierarchy, moving this NSView to a new container.
            if let tv = terminal {
                tv.needsDisplay = true
                tv.terminalView.needsDisplay = true
            }
        }
    }

    override func layout() {
        super.layout()
        // Create terminal only after we have a real frame, preventing 0-column grids
        if window != nil && !terminalInstalled && bounds.width > 1 {
            scheduleTerminalInstall()
        }
        // Start daemon attach only after the first layout pass gives us a real frame.
        // Starting while frame is .zero causes SwiftTerm to report 1-column size.
        if let tv = terminal, tv.bounds.width > 1 {
            if !attachStarted {
                attachStarted = true
                startDaemonAttach()
                startHeartbeat()
            } else if let task = attachTask, task.isCancelled || isAttachDone {
                // Session died — restart
                startDaemonAttach()
            }
        }
    }

    private func startDaemonAttach() {
        guard let tv = terminal else { return }

        isAttachDone = false
        let session = DaemonAttachSession(agentId: agent.id, terminalView: tv.terminalView)
        tv.attachSession = session

        attachTask = Task { [weak self] in
            await session.start()
            await MainActor.run { self?.isAttachDone = true }
        }
    }

    /// Restart the attach session if it has died and the view has a valid frame.
    func reconnectIfNeeded() {
        guard isAttachDone, let tv = terminal, tv.bounds.width > 1 else { return }
        startDaemonAttach()
    }

    override var acceptsFirstResponder: Bool { true }

    override func mouseDown(with event: NSEvent) {
        if let tv = terminal?.terminalView {
            window?.makeFirstResponder(tv)
        }
        super.mouseDown(with: event)
    }

    override func viewDidChangeEffectiveAppearance() {
        super.viewDidChangeEffectiveAppearance()
        layer?.backgroundColor = TerminalTheme.background.cgColor
    }

    private func startHeartbeat() {
        guard heartbeatTimer == nil else { return }
        heartbeatTimer = Timer.scheduledTimer(withTimeInterval: 5, repeats: true) { [weak self] _ in
            self?.reconnectIfNeeded()
        }
    }

    func tearDown() {
        installDebounce?.cancel()
        installDebounce = nil
        heartbeatTimer?.invalidate()
        heartbeatTimer = nil
        attachTask?.cancel()
        attachTask = nil
        terminal?.tearDown()
    }
}
