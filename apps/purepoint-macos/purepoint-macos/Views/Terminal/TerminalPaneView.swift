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
    private(set) var terminal: ScrollableTerminal?
    private var attachTask: Task<Void, Never>?
    private var attachStarted = false
    private var isAttachDone = false
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
                    guard let self else { return }
                    self.window?.makeFirstResponder(tv.terminalView)
                }
            } else if let task = attachTask, task.isCancelled || isAttachDone {
                // Session died — restart
                startDaemonAttach()
            }
        }
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
        let session = DaemonAttachSession(
            agentId: agent.id,
            terminalView: tv.terminalView,
            onFirstOutput: { [weak self] in self?.removeSpinner() }
        )
        tv.attachSession = session

        attachTask = Task { [weak self] in
            await session.start()
            let agentGone = await session.isAgentGone
            await MainActor.run {
                self?.isAttachDone = true
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
        guard isAttachDone, let tv = terminal, tv.bounds.width > 1 else { return }
        startDaemonAttach()
    }

    override var acceptsFirstResponder: Bool { true }

    func focusTerminal() {
        guard let tv = terminal?.terminalView else { return }
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
