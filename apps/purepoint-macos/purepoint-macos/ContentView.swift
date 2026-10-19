import Combine
import SwiftUI

struct ContentView: View {
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry
    @Environment(TerminalViewCache.self) private var viewCache
    @State private var selection: SidebarSelection? = .nav(.dashboard)
    @State private var sidebarVisible = true
    @State private var sidebarOutlineView: NSOutlineView?
    /// A focus request that arrived while the sidebar was hidden, waiting for its outline view to join a window.
    @State private var sidebarFocusPending = false

    /// Clears the traffic-light buttons, which sit over the top of the sidebar.
    private let trafficLightInset: CGFloat = 28
    /// Clears the traffic-light buttons when the sidebar is hidden and the detail starts at the left edge.
    private let hiddenSidebarLeadingInset: CGFloat = 72

    var body: some View {
        @Bindable var appState = appState

        // A flush sidebar column, not a floating split-view panel. Both columns run the full
        // window height, so each pane's tab strip reaches the top edge.
        HStack(spacing: 0) {
            if sidebarVisible {
                SidebarView(
                    selection: $selection,
                    onOutlineViewReady: { outlineView in
                        DispatchQueue.main.async {
                            sidebarOutlineView = outlineView
                            if sidebarFocusPending {
                                sidebarFocusPending = false
                                outlineView.window?.makeFirstResponder(outlineView)
                            }
                        }
                    }
                )
                .padding(.top, trafficLightInset)
                .frame(width: PurePointTheme.sidebarIdealWidth)
                .frame(maxHeight: .infinity)
                .background(Color(nsColor: Theme.sidebarBackground))

                Rectangle()
                    .fill(Color.primary.opacity(0.06))
                    .frame(width: 1)
            }

            DetailView(selection: $selection)
                .padding(.leading, sidebarVisible ? 0 : hiddenSidebarLeadingInset)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        }
        // The configurator makes the content view span the title bar; this drops the inset that leaves.
        .ignoresSafeArea(.container, edges: .top)
        .background(WindowChromeConfigurator())
        .navigationTitle("")
        .task {
            while !Task.isCancelled {
                appState.updateAgentSignals()
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
            }
        }
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
        .onReceive(appState.pointGuardService.$activeCwd.removeDuplicates()) { cwd in
            let root = cwd.isEmpty ? nil : URL(fileURLWithPath: cwd).standardizedFileURL.path
            guard root != appState.pointGuardRoot else { return }
            appState.updatePointGuardRoot(cwd)
            if selection == .nav(.dashboard) {
                appState.activatePointGuardWorkspace()
            }
        }
        .onChange(of: appState.pendingSignalDestination) { _, _ in
            selectSignalDestination()
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
            if newValue == .nav(.dashboard) {
                appState.activatePointGuardWorkspace()
                return
            }
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
        selection =
            registry.workspace(id: workspaceId)?.isPointGuard == true ? .nav(.dashboard) : .workspace(workspaceId)
    }

    private func selectSignalDestination() {
        guard let destination = appState.pendingSignalDestination else { return }
        // Consume once. Ambiguous or expired destinations never fall back to global ID lookup.
        appState.pendingSignalDestination = nil
        guard let project = appState.projectState(forRoot: destination.projectID),
            let agent = project.agent(byId: destination.agentID),
            destination.matches(agent.signal), agent.status.isAlive, !agent.suspended, !project.isUnavailable,
            let target = AgentSignalRouting.target(for: destination, among: appState.signalRoutingTargets)
        else { return }
        appState.activeProjectRoot = destination.projectID
        registry.selectTab(workspaceId: target.workspaceID, surfaceId: target.surfaceID)
        registry.setFocus(workspaceId: target.workspaceID, leafId: target.leafID)
        selection = registry.workspace(id: target.workspaceID)?.isPointGuard == true
            ? .nav(.dashboard) : .workspace(target.workspaceID)
    }

    /// Resolve a just-spawned agent to the workspace that now holds it.
    private func selectPendingAgentIfReady() {
        guard let agentId = appState.pendingSelectAgentId,
            let workspaceId = registry.workspaceId(forAgent: agentId)
        else { return }

        appState.pendingSelectAgentId = nil
        appState.pendingFocusAgentId = agentId
        selection =
            registry.workspace(id: workspaceId)?.isPointGuard == true ? .nav(.dashboard) : .workspace(workspaceId)
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            appState.pendingFocusAgentId = nil
        }
    }

    // MARK: - Hotkey Dispatch

    private func handleHotkeyAction(_ action: HotkeyAction) {
        switch action {
        case .focusSidebar:
            if sidebarVisible {
                DispatchQueue.main.async {
                    if let outlineView = sidebarOutlineView {
                        outlineView.window?.makeFirstResponder(outlineView)
                    }
                }
            } else {
                // The outline view is created when the sidebar reappears; focus it once it has a window.
                sidebarFocusPending = true
                sidebarVisible = true
            }

        case .focusContent:
            // Find the terminal in the current view and focus it
            DispatchQueue.main.async {
                guard let window = NSApp.keyWindow else { return }
                focusTerminalInWindow(window)
            }

        case .toggleSidebar:
            withAnimation {
                sidebarVisible.toggle()
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
                TabCloseConfirmation.confirm(
                    closing: [surface], in: workspace.id, appState: appState, registry: registry)
            else { break }
            registry.closeTab(workspaceId: workspace.id, surfaceId: surface.id)

        case .toggleChatSidebar:
            NotificationCenter.default.post(name: .toggleChatSidebar, object: nil)

        default:
            break
        }
    }

    private func focusTerminalInWindow(_ window: NSWindow) {
        // Walk the view hierarchy to find a visible terminal pane.
        func findTerminalPane(in view: NSView) -> TerminalPaneNSView? {
            if let pane = view as? TerminalPaneNSView,
                pane.terminal != nil
            {
                return pane
            }
            for sub in view.subviews where !sub.isHidden {
                if let found = findTerminalPane(in: sub) { return found }
            }
            return nil
        }

        guard let contentView = window.contentView,
            let pane = findTerminalPane(in: contentView)
        else { return }
        pane.focusTerminal()
    }
}
