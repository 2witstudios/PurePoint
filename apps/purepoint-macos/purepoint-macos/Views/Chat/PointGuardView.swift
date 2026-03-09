import SwiftTerm
import SwiftUI

struct PointGuardView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @State private var sessionList = SessionListState()
    @State private var showSidebar = true
    @State private var shellAgentId: String?
    @State private var selectedSessionId: String?
    @State private var shellError: String?

    var body: some View {
        HSplitView {
            if showSidebar {
                ConversationSidebarView(
                    sessionList: sessionList,
                    showSidebar: $showSidebar,
                    selectedSessionId: selectedSessionId,
                    onSelect: { session in
                        selectedSessionId = session.sessionId
                        Task {
                            await respawnShell(cwd: session.projectPath, then: "claude --resume \(session.sessionId)")
                        }
                    },
                    onNewConversation: {
                        selectedSessionId = nil
                        Task { await respawnShell(cwd: NSHomeDirectory()) }
                    }
                )
                .frame(minWidth: 180, idealWidth: 200, maxWidth: 280)
            }

            ZStack {
                PointGuardTerminalView(agentId: shellAgentId)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                if shellAgentId == nil {
                    VStack(spacing: 12) {
                        if let shellError {
                            Text(shellError)
                                .foregroundStyle(.secondary)
                                .font(.system(size: 13))
                            Button("Retry") {
                                Task { await spawnShell() }
                            }
                            .buttonStyle(.bordered)
                        } else {
                            ProgressView()
                                .controlSize(.small)
                            Text("Starting terminal...")
                                .foregroundStyle(.secondary)
                                .font(.system(size: 13))
                        }
                    }
                }
            }
        }
        .toolbar {
            if !showSidebar {
                ToolbarItem(placement: .navigation) {
                    Button {
                        withAnimation(.easeInOut(duration: 0.2)) {
                            showSidebar = true
                        }
                    } label: {
                        Image(systemName: "sidebar.left")
                    }
                    .help("Show conversations (Command+Shift+S)")
                }
            }
        }
        .task {
            await spawnShell()
            await sessionList.refreshSessions()
        }
        .onReceive(NotificationCenter.default.publisher(for: .toggleChatSidebar)) { _ in
            showSidebar.toggle()
        }
    }

    private func spawnShell() async {
        await respawnShell(cwd: NSHomeDirectory())
    }

    /// Kill the current shell, spawn a new one at `cwd`, optionally run a command.
    private func respawnShell(cwd: String, then command: String? = nil) async {
        shellError = nil

        // Tell existing shell to exit: Ctrl-C anything running, then `exit`
        if let oldId = shellAgentId {
            shellAgentId = nil
            let client = DaemonClient()
            _ = try? await client.send(.input(agentId: oldId, data: Data([0x03])))  // Ctrl-C
            try? await Task.sleep(for: .milliseconds(50))
            _ = try? await client.send(.input(agentId: oldId, data: Data("exit\n".utf8)))
        }

        do {
            try await DaemonLifecycle.ensureDaemon()
            let client = DaemonClient()
            let response = try await client.send(.spawnShell(cwd: cwd))
            if case .spawnResult(_, let agentId, _) = response {
                shellAgentId = agentId
                if let command {
                    // Brief delay for shell to initialize before sending command
                    try await Task.sleep(for: .milliseconds(300))
                    _ = try? await client.send(.input(agentId: agentId, data: Data("\(command)\n".utf8)))
                }
            } else if case .error(_, let message) = response {
                shellError = message
            }
        } catch {
            shellError = error.localizedDescription
        }
    }
}

/// NSViewRepresentable that hosts a ScrollableTerminal connected to a shell agent.
struct PointGuardTerminalView: NSViewRepresentable {
    let agentId: String?

    func makeNSView(context: Context) -> PointGuardTerminalNSView {
        PointGuardTerminalNSView()
    }

    func updateNSView(_ nsView: PointGuardTerminalNSView, context: Context) {
        if let agentId, nsView.currentAgentId != agentId {
            nsView.attach(agentId: agentId)
        }
    }

    static func dismantleNSView(_ nsView: PointGuardTerminalNSView, coordinator: ()) {
        nsView.tearDown()
    }
}

class PointGuardTerminalNSView: NSView {
    private(set) var currentAgentId: String?
    private var terminal: ScrollableTerminal?
    private var attachTask: Task<Void, Never>?

    override init(frame: NSRect) {
        super.init(frame: frame)
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
        terminal = tv
    }

    override func layout() {
        super.layout()
        if window != nil && terminal == nil && bounds.width > 1 {
            ensureTerminal()
        }
        // Start attach once terminal has a real frame
        if let agentId = currentAgentId, let tv = terminal, tv.bounds.width > 1, attachTask == nil {
            startAttach(agentId: agentId)
        }
    }

    func attach(agentId: String) {
        currentAgentId = agentId
        guard let tv = terminal, tv.bounds.width > 1 else {
            // Terminal not ready yet; layout() will start attach
            return
        }
        startAttach(agentId: agentId)
    }

    private func startAttach(agentId: String) {
        attachTask?.cancel()
        if let old = terminal?.attachSession {
            Task { await old.stop() }
        }

        guard let tv = terminal else { return }

        // Clear terminal buffer so old shell output doesn't linger
        tv.terminalView.getTerminal().resetToInitialState()

        let session = DaemonAttachSession(agentId: agentId, terminalView: tv.terminalView)
        tv.attachSession = session

        attachTask = Task {
            await session.start()
        }

        // Focus the terminal
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.window?.makeFirstResponder(tv.terminalView)
        }
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

    func tearDown() {
        attachTask?.cancel()
        attachTask = nil
        terminal?.tearDown()
    }
}
