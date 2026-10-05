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
    var onMouseDown: (() -> Void)?
    /// Live status lookup. `agent` is a snapshot from creation, so its status
    /// is stale; set by `TerminalViewCache`.
    var isAgentAlive: () -> Bool = { true }
    /// Called when output arrives while this terminal is not on screen — a background tab.
    /// Set by `TerminalViewCache`.
    var onBackgroundOutput: (() -> Void)?
    /// Every attach replays the daemon's buffer; output before this moment is that replay,
    /// not new activity, so it must not flag the tab as having unseen output.
    private var replaySettlesAt = Date.distantFuture
    private(set) var terminal: ScrollableTerminal?
    private var attachTask: Task<Void, Never>?
    private var attachStarted = false
    private var isAttachDone = false
    /// The daemon closed the last stream because the agent exited.
    private var didStreamEnd = false
    private(set) var isAgentGone = false
    private var heartbeatTimer: Timer?
    private var spinner: NSProgressIndicator?
    private var spinnerShownAt = Date()

    init(agent: AgentModel) {
        self.agent = agent
        super.init(frame: .zero)
        wantsLayer = true
        layer?.backgroundColor = TerminalTheme.background.cgColor
    }

    required init?(coder: NSCoder) { fatalError() }

    private func ensureTerminal() {
        guard terminal == nil else { return }

        let tv = ScrollableTerminal(frame: bounds)
        tv.wantsLayer = true
        tv.layer?.masksToBounds = true
        tv.pinToEdges(of: self)
        tv.terminalView.hideCursor(source: tv.terminalView.getTerminal())
        terminal = tv

        // Show a small spinner while waiting for first output
        let indicator = NSProgressIndicator()
        indicator.style = .spinning
        indicator.controlSize = .small
        indicator.translatesAutoresizingMaskIntoConstraints = false
        indicator.startAnimation(nil)
        addSubview(indicator)
        NSLayoutConstraint.activate([
            indicator.centerXAnchor.constraint(equalTo: centerXAnchor),
            indicator.centerYAnchor.constraint(equalTo: centerYAnchor),
        ])
        spinner = indicator
        spinnerShownAt = Date()
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
        if window != nil && terminal == nil && bounds.width > 1 {
            ensureTerminal()
        }
        // Start daemon attach only after the first layout pass gives us a real frame.
        // Starting while frame is .zero causes SwiftTerm to report 1-column size.
        if let tv = terminal, tv.bounds.width > 1 {
            if !attachStarted {
                attachStarted = true
                startDaemonAttach()
                startHeartbeat()
                // New panes should be immediately interactive.
                DispatchQueue.main.async { [weak self] in
                    guard let self, !InlineRenameFocus.isActive else { return }
                    self.window?.makeFirstResponder(tv.terminalView)
                }
            } else if let task = attachTask, task.isCancelled || isAttachDone, canReattach {
                // Session died — restart
                startDaemonAttach()
            }
        }
    }

    private func noteOutput() {
        guard Date() >= replaySettlesAt, isHidden || window == nil else { return }
        onBackgroundOutput?()
    }

    private func startDaemonAttach() {
        guard let tv = terminal else { return }

        // Clean up previous session before creating a new one
        attachTask?.cancel()
        let oldSession = tv.attachSession
        if oldSession != nil {
            Task { await oldSession?.stop() }
        }

        isAttachDone = false
        didStreamEnd = false
        let session = DaemonAttachSession(
            agentId: agent.id,
            terminalView: tv.terminalView,
            // Every attach replays the daemon's whole buffer; only the first
            // one lands in an empty terminal.
            resetBeforeReplay: oldSession != nil,
            onFirstOutput: { [weak self] in self?.removeSpinner() },
            onOutput: { [weak self] in self?.noteOutput() }
        )
        replaySettlesAt = Date().addingTimeInterval(2)
        tv.attachSession = session

        attachTask = Task { [weak self] in
            await session.start()
            let agentGone = await session.isAgentGone
            let streamEnded = await session.didStreamEnd
            await MainActor.run {
                self?.isAttachDone = true
                self?.didStreamEnd = streamEnded
                if agentGone {
                    self?.isAgentGone = true
                    self?.heartbeatTimer?.invalidate()
                    self?.heartbeatTimer = nil
                }
            }
        }
    }

    /// Restart the attach session if it has died and the view has a valid frame.
    func reconnectIfNeeded() {
        guard !isAgentGone else { return }
        guard isAttachDone, canReattach, let tv = terminal, tv.bounds.width > 1 else { return }
        startDaemonAttach()
    }

    /// After the agent exits, the terminal already holds all of its output.
    /// Reattach only if it is running again (e.g. resumed).
    private var canReattach: Bool {
        !didStreamEnd || isAgentAlive()
    }

    override var acceptsFirstResponder: Bool { true }

    func focusTerminal() {
        guard !InlineRenameFocus.isActive, let tv = terminal?.terminalView else { return }
        window?.makeFirstResponder(tv)
    }

    override func mouseDown(with event: NSEvent) {
        onMouseDown?()
        focusTerminal()
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

    private func removeSpinner() {
        guard let spinner else { return }
        let elapsed = Date().timeIntervalSince(spinnerShownAt)
        if elapsed >= 0.5 {
            spinner.stopAnimation(nil)
            spinner.removeFromSuperview()
            self.spinner = nil
        } else {
            DispatchQueue.main.asyncAfter(deadline: .now() + (0.5 - elapsed)) { [weak self] in
                self?.spinner?.stopAnimation(nil)
                self?.spinner?.removeFromSuperview()
                self?.spinner = nil
            }
        }
    }

    func tearDown() {
        removeSpinner()
        heartbeatTimer?.invalidate()
        heartbeatTimer = nil
        attachTask?.cancel()
        attachTask = nil
        terminal?.tearDown()
    }
}
