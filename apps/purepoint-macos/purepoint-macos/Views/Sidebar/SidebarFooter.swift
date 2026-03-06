import SwiftUI

struct SidebarFooter: View {
    @Environment(AppState.self) private var appState
    let selection: SidebarSelection?

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
    }

    private var activeProject: ProjectState? {
        switch selection {
        case .agent(let id):
            return appState.projectState(forAgentId: id)
        case .worktree(let id):
            return appState.projectState(forWorktreeId: id)
        case .project(let root):
            return appState.projectState(forRoot: root)
        default:
            return appState.projects.first
        }
    }

    private func openProjectWithPicker() {
        let panel = NSOpenPanel()
        panel.canChooseDirectories = true
        panel.canChooseFiles = false
        panel.allowsMultipleSelection = false
        panel.message = "Choose a project directory"

        if panel.runModal() == .OK, let url = panel.url {
            appState.openProject(url.path)
        }
    }

    private func showCommandPalette() {
        guard let project = activeProject else { return }
        let sel = selection
        let hub = appState.agentsHubState
        let items = CommandPaletteItem.buildItems(
            builtInVariants: AgentVariant.variantsWithWorktree,
            agents: hub.agents,
            swarms: hub.swarms
        )
        Task { await hub.loadAll(projectRoot: project.projectRoot) }

        CommandPalettePanel.show(relativeTo: NSApp.keyWindow, items: items) { result in
            project.handlePaletteResult(result, selection: sel, hub: hub)
        }
    }
}

#Preview {
    SidebarFooter(selection: nil)
        .frame(width: 240)
        .environment(AppState())
}
