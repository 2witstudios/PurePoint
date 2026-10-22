#if os(macOS)
import Sparkle
import SwiftUI

@main
struct purepoint_macosApp: App {
    @NSApplicationDelegateAdaptor(PointGuardApplicationDelegate.self) private var applicationDelegate
    @State private var appState = AppState()
    @State private var settingsState = SettingsState()
    @State private var viewCache = TerminalViewCache()
    @State private var registry = WorkspaceRegistry()
    @State private var keyBindingState = KeyBindingState()
    @State private var hotkeyMonitor = HotkeyMonitor()
    @StateObject private var updaterViewModel = CheckForUpdatesViewModel()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .environment(settingsState)
                .environment(viewCache)
                .environment(registry)
                .environment(keyBindingState)
                .environmentObject(updaterViewModel)
                .preferredColorScheme(settingsState.appearance.colorScheme)
                .frame(
                    minWidth: PurePointTheme.windowMinWidth,
                    minHeight: PurePointTheme.windowMinHeight
                )
                .onAppear {
                    // "Tab" means a pane tab here. macOS window tabs would add a second,
                    // unrelated tab bar and View-menu items, and compete for ⌘T.
                    NSWindow.allowsAutomaticWindowTabbing = false
                    appState.registry = registry
                    viewCache.agentStatus = { [weak appState] agentId in
                        appState?.agent(byId: agentId)?.status
                    }
                    viewCache.clientForAgent = { [weak appState] agentId in
                        appState?.client(forAgentId: agentId)
                    }
                    registry.onCloseAgent = { [viewCache] projectId, agentId in
                        viewCache.remove(agentId: agentId)
                        appState.projectState(forId: projectId)?.removeAndKillAgent(agentId)
                    }
                    hotkeyMonitor.keyBindingState = keyBindingState
                    hotkeyMonitor.start()
                    CLIInstaller.installIfNeeded()
                    openProjectFromArguments()
                    appState.startLocalProjectOrderRefresh()
                    appState.loadMachines()
                    appState.pointGuardService.start(chat: appState.pointGuardChat)
                    applicationDelegate.pointGuardService = appState.pointGuardService
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                    appState.shutdownWithSuspend()
                }
        }
        .defaultSize(
            width: PurePointTheme.windowDefaultWidth,
            height: PurePointTheme.windowDefaultHeight
        )
        // No title bar band: the sidebar and the pane tab strip both start at the top edge.
        .windowStyle(.hiddenTitleBar)
        .commands {
            AppCommands(
                appState: appState,
                settingsState: settingsState,
                registry: registry,
                keyBindingState: keyBindingState,
                updaterViewModel: updaterViewModel
            )
        }
    }

    private func openProjectFromArguments() {
        // Check command-line arguments for --project-root
        let args = ProcessInfo.processInfo.arguments
        for (i, arg) in args.enumerated() {
            if arg == "--project-root", i + 1 < args.count {
                let path = args[i + 1]
                appState.restoreProjects(projectRoot: path)
                return
            }
        }

        // Restore saved projects from UserDefaults
        appState.restoreProjects()

        // Restore the workspace that was on screen, once reconcile has run for each project.
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 500_000_000)
            appState.restoreActiveWorkspace()
        }
    }
}
#endif
