import SwiftUI

struct SidebarFooter: View {
    @Environment(SettingsState.self) private var settingsState
    @Environment(AppState.self) private var appState
    let selection: SidebarSelection?
    @State private var showAddMachine = false

    var body: some View {
        VStack(spacing: 0) {
            Divider()

            HStack(spacing: 12) {
                Button {
                    appState.showSettings = true
                } label: {
                    Image(systemName: "gear")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Button {
                    showAddMachine = true
                } label: {
                    Image(systemName: "desktopcomputer")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
                .help("Add Machine\u{2026}")
                .accessibilityLabel("Add Machine")

                Spacer()

                Button {
                    showCommandPalette()
                } label: {
                    HStack(spacing: 3) {
                        Image(systemName: "plus")
                        Text("New")
                            .font(PurePointTheme.smallFont)
                        Text("⌘N")
                            .font(PurePointTheme.smallFont)
                            .foregroundStyle(.tertiary)
                    }
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Button {
                    openProjectWithPicker()
                } label: {
                    HStack(spacing: 3) {
                        Text("Open")
                            .font(PurePointTheme.smallFont)
                        Text("⌘O")
                            .font(PurePointTheme.smallFont)
                            .foregroundStyle(.tertiary)
                    }
                    .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, PurePointTheme.padding + 4)
            .frame(height: PurePointTheme.footerHeight)
        }
        .sheet(isPresented: $showAddMachine) {
            #if os(iOS) && DEBUG
            AddMachineSheet(flow: debugAddMachine.flow)
            #else
            AddMachineSheet()
            #endif
        }
        #if os(iOS) && DEBUG
        // `-PUDebugAddMachine` shows the sheet it is submitting (Debug launch arguments).
        .onChange(of: debugAddMachine.flow == nil, initial: true) { _, isNil in
            if !isNil { showAddMachine = true }
        }
        // `-PUDebugOpen add-machine` shows it empty, as this footer's button does.
        .onChange(of: debugAddMachine.showEmpty, initial: true) { _, show in
            if show {
                debugAddMachine.showEmpty = false
                showAddMachine = true
            }
        }
        #endif
    }

    #if os(iOS) && DEBUG
    private var debugAddMachine: DebugAddMachinePresenter { .shared }
    #endif

    private var activeProject: ProjectState? {
        switch selection {
        case .workspace(let id):
            return appState.registry?.projectRoot(forWorkspace: id).flatMap { appState.projectState(forId: $0) }
        case .worktree(let id):
            return appState.projectState(forWorktreeId: id)
        case .project(let projectId), .channel(let projectId):
            return appState.projectState(forId: projectId)
        default:
            return appState.projects.first
        }
    }

    private func openProjectWithPicker() {
        #if os(macOS)
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a project directory"

        if panel.runModal() == .OK, let url = panel.url {
            appState.openProject(url.path)
        }
        #else
        // No local folders on iPad: the remote folder picker registers this hook (IPAD-7.1).
        AppLifecycleHooks.openProject?()
        #endif
    }

    private func showCommandPalette() {
        if selection == .nav(.dashboard) || appState.registry?.activeWorkspace?.isPointGuard == true {
            appState.newPointGuardTab()
            return
        }
        guard let project = activeProject else { return }
        let sel = selection
        let hub = appState.agentsHubState
        let window = NSApp.keyWindow
        CommandPalettePanel.requestOpen {
            await hub.loadAll(projectRoots: appState.libraryProjectRoots)
            let items = CommandPaletteItem.buildItems(
                builtInVariants: AgentVariant.variantsWithWorktree,
                agents: hub.agents,
                swarms: hub.swarms,
                preferredOrder: settingsState.commandPaletteOrder
            )

            CommandPalettePanel.show(
                relativeTo: window, items: items, usePointGuardNames: settingsState.usePointGuardNames
            ) { result in
                project.handlePaletteResult(result, selection: sel, hub: hub)
            }
        }
    }
}

#Preview {
    SidebarFooter(selection: nil)
        .frame(width: 240)
        .environment(AppState())
}
