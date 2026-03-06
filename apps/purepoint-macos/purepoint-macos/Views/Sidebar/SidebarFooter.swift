import SwiftUI

struct SidebarFooter: View {
    @Environment(AppState.self) private var appState
    let selection: SidebarSelection?

    var body: some View {
        VStack(spacing: 0) {
            Divider()

            HStack {
                Button {
                    appState.showSettings = true
                } label: {
                    Image(systemName: "gear")
                        .foregroundStyle(.secondary)
                }
                .buttonStyle(.plain)

                Text("Settings")
                    .font(PurePointTheme.smallFont)
                    .foregroundStyle(.secondary)

                Spacer()

                Button {
                    showCommandPalette()
                } label: {
                    HStack(spacing: 3) {
                        Image(systemName: "plus")
                        Text("Add")
                            .font(PurePointTheme.smallFont)
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
