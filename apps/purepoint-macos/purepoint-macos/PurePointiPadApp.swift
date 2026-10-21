#if os(iOS)
import SwiftUI

/// The iPad app: the Mac app's shell over remote machines only. It shares every state type
/// with the Mac App, minus Sparkle updates and the Point Guard host (IPAD-001, IPAD-011).
@main
struct PurePointiPadApp: App {
    @State private var appState = AppState()
    @State private var settingsState = SettingsState()
    @State private var viewCache = TerminalViewCache()
    @State private var registry = WorkspaceRegistry()
    @State private var keyBindingState = KeyBindingState()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .environment(settingsState)
                .environment(viewCache)
                .environment(registry)
                .environment(keyBindingState)
                .preferredColorScheme(settingsState.appearance.colorScheme)
                .onAppear {
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
                    appState.loadMachines()
                }
        }
    }
}
#endif
