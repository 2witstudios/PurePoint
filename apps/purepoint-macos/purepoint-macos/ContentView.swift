import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry
    @Environment(TerminalViewCache.self) private var viewCache
    @State private var selection: SidebarSelection? = .nav(.dashboard)
    @State private var columnVisibility: NavigationSplitViewVisibility = .automatic
    @State private var sidebarOutlineView: NSOutlineView?

    var body: some View {
        @Bindable var appState = appState

        NavigationSplitView(columnVisibility: $columnVisibility) {
            SidebarView(
                selection: $selection,
                onOutlineViewReady: { outlineView in
                    DispatchQueue.main.async {
                        sidebarOutlineView = outlineView
                    }
                }
            )
            .navigationSplitViewColumnWidth(
                min: PurePointTheme.sidebarMinWidth,
                ideal: PurePointTheme.sidebarIdealWidth,
                max: PurePointTheme.sidebarMaxWidth
            )
        } detail: {
            DetailView(selection: $selection)
        }
        .navigationTitle("")
        .overlay(alignment: .top) {
            if let error = appState.daemonError {
                DaemonErrorBanner(message: error) {
                    appState.daemonError = nil
                }
            }
        }
        .animation(.easeInOut(duration: 0.25), value: appState.daemonError)
        .overlay {
            if appState.showSettings {
                Color.black.opacity(0.3)
                    .ignoresSafeArea()
                    .onTapGesture { appState.showSettings = false }

                SettingsView()
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .shadow(color: .black.opacity(0.3), radius: 20, y: 10)
                    .onExitCommand { appState.showSettings = false }
                    .transition(.opacity.combined(with: .scale(scale: 0.95)))
            }
        }
        .animation(.easeInOut(duration: 0.2), value: appState.showSettings)
        .onReceive(NotificationCenter.default.publisher(for: .hotkeyAction)) { notification in
            guard let action = notification.userInfo?["action"] as? HotkeyAction else { return }
            handleHotkeyAction(action)
        }
        .onChange(of: appState.pendingSelectAgentId) { _, agentId in
            guard agentId != nil else { return }
            selectPendingAgentIfReady()
        }
        .onChange(of: registry.workspacesByProject) { _, _ in
            // Closing a workspace's last tab removes the workspace; fall back to the dashboard
            // rather than leaving a dangling selection, whichever path closed it.
            if case .workspace(let id) = selection, registry.workspace(id: id) == nil {
                selection = .nav(.dashboard)
            }
            // A spawn's workspace only exists once the manifest lands and reconcile runs.
            selectPendingAgentIfReady()
            selectPendingWorkspaceIfReady()
        }
        .onChange(of: appState.pendingSelectWorkspaceId) { _, workspaceId in
            guard workspaceId != nil else { return }
            selectPendingWorkspaceIfReady()
        }
        .onChange(of: appState.pendingSelectWorktreeId) { _, worktreeId in
            guard let worktreeId else { return }
            appState.pendingSelectWorktreeId = nil
            selection = .worktree(worktreeId)
        }
        .onChange(of: selection) { _, newValue in
            appState.updateActiveProject(for: newValue)

            // Selecting a workspace shows its grid and focuses a pane. A single-pane
            // workspace is a grid of one, so one click lands the user in the terminal.
            guard case .workspace(let workspaceId) = newValue else {
                registry.activeWorkspaceId = nil
                return
            }
            registry.activate(workspaceId: workspaceId)
        }
    }

    /// Restore the last session's workspace as soon as reconcile has produced it.
    private func selectPendingWorkspaceIfReady() {
        guard let workspaceId = appState.resolvePendingWorkspaceSelection() else { return }
        selection = .workspace(workspaceId)
    }

    /// Resolve a just-spawned agent to the workspace that now holds it.
    private func selectPendingAgentIfReady() {
        guard let agentId = appState.pendingSelectAgentId,
            let workspaceId = registry.workspaceId(forAgent: agentId)
        else { return }

        appState.pendingSelectAgentId = nil
        appState.pendingFocusAgentId = agentId
        selection = .workspace(workspaceId)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            appState.pendingFocusAgentId = nil
        }
    }

    // MARK: - Hotkey Dispatch

    private func handleHotkeyAction(_ action: HotkeyAction) {
        switch action {
        case .focusSidebar:
            columnVisibility = .all
            DispatchQueue.main.async {
                if let outlineView = sidebarOutlineView {
                    outlineView.window?.makeFirstResponder(outlineView)
                }
            }

        case .focusContent:
            // Find the terminal in the current view and focus it
            DispatchQueue.main.async {
                guard let window = NSApp.keyWindow else { return }
                focusTerminalInWindow(window)
            }

        case .toggleSidebar:
            withAnimation {
                columnVisibility = columnVisibility == .detailOnly ? .all : .detailOnly
            }

        case .navDashboard:
            selection = .nav(.dashboard)

        case .navAgents:
            selection = .nav(.agents)

        case .navSchedule:
            selection = .nav(.schedule)

        case .closeAgent:
            // ⌘W closes the focused tab; its pane goes with its last tab, and the workspace
            // with its last pane.
            // (A workspace left with nothing open is removed; the selection then falls back
            // to the dashboard via the workspacesByProject observer.)
            guard let workspace = registry.activeWorkspace, let surface = workspace.focusedSurface,
                TabCloseConfirmation.confirm(closing: [surface], in: workspace.id, appState: appState, registry: registry)
            else { break }
            registry.closeTab(workspaceId: workspace.id, surfaceId: surface.id)

        case .toggleChatSidebar:
            NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)

        default:
            break
        }
    }

    private func focusTerminalInWindow(_ window: NSWindow) {
        // Walk the view hierarchy to find a visible TerminalView
        func findTerminalView(in view: NSView) -> NSView? {
            if let pane = view as? TerminalPaneNSView,
                let tv = pane.terminal?.terminalView
            {
                return tv
            }
            for sub in view.subviews where !sub.isHidden {
                if let found = findTerminalView(in: sub) { return found }
            }
            return nil
        }

        guard let contentView = window.contentView,
            let tv = findTerminalView(in: contentView)
        else { return }
        guard !InlineRenameFocus.isActive else { return }
        window.makeFirstResponder(tv)
    }
}
