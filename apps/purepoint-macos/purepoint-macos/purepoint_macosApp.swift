import SwiftUI

@main
struct purepoint_macosApp: App {
    @State private var appState = AppState()
    @State private var viewCache = TerminalViewCache()

    var body: some Scene {
        WindowGroup {
            ContentView()
                .environment(appState)
                .environment(viewCache)
                .frame(
                    minWidth: PurePointTheme.windowMinWidth,
                    minHeight: PurePointTheme.windowMinHeight
                )
                .onAppear {
                    openProjectFromArguments()
                }
                .onReceive(NotificationCenter.default.publisher(for: NSApplication.willTerminateNotification)) { _ in
                    appState.shutdown()
                }
        }
        .defaultSize(
            width: PurePointTheme.windowDefaultWidth,
            height: PurePointTheme.windowDefaultHeight
        )
        .windowToolbarStyle(.unified(showsTitle: true))
        .commands {
            CommandGroup(after: .newItem) {
                Button("Open Project...") {
                    openProjectWithPicker()
                }
                .keyboardShortcut("o")
            }
        }
    }

    private func openProjectFromArguments() {
        // Check command-line arguments for --project-root
        let args = ProcessInfo.processInfo.arguments
        for (i, arg) in args.enumerated() {
            if arg == "--project-root", i + 1 < args.count {
                let path = args[i + 1]
                UserDefaults.standard.set(path, forKey: "PurePointLastProject")
                appState.openProject(path)
                return
            }
        }

        // Try last opened project from UserDefaults
        if let lastProject = UserDefaults.standard.string(forKey: "PurePointLastProject"),
           FileManager.default.fileExists(atPath: lastProject) {
            appState.openProject(lastProject)
        }
    }

    private func openProjectWithPicker() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a project directory"

        if panel.runModal() == .OK, let url = panel.url {
            let path = url.path
            UserDefaults.standard.set(path, forKey: "PurePointLastProject")
            appState.openProject(path)
        }
    }
}
