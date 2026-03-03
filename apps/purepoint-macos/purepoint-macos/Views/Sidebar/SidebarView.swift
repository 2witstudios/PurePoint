import SwiftUI

struct SidebarView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState
    @State private var expandedWorktrees: Set<String> = []
    @State private var didInitialExpand = false

    var body: some View {
        List(selection: $selection) {
            Section {
                ForEach(SidebarNavItem.allCases) { item in
                    Label(item.title, systemImage: item.icon)
                        .font(PurePointTheme.navFont)
                        .tag(SidebarSelection.nav(item))
                }
            }

            Section {
                if appState.isLoaded {
                    DisclosureGroup(
                        isExpanded: .constant(true)
                    ) {
                        ForEach(visibleRootAgents) { agent in
                            AgentRow(agent: agent, isGridOwner: agent.id == gridState.ownerAgentId)
                                .tag(SidebarSelection.agent(agent.id))
                        }

                        ForEach(appState.worktrees) { worktree in
                            DisclosureGroup(
                                isExpanded: binding(for: worktree.id)
                            ) {
                                ForEach(visibleAgents(in: worktree)) { agent in
                                    AgentRow(agent: agent, isGridOwner: agent.id == gridState.ownerAgentId)
                                        .tag(SidebarSelection.agent(agent.id))
                                }
                            } label: {
                                WorktreeRow(
                                    worktree: worktree,
                                    onAddAgent: { showCommandPalette(for: .worktree(worktree.id)) },
                                    onAddTerminal: {
                                        appState.createAgent(variant: .terminal, prompt: nil, selection: .worktree(worktree.id))
                                    }
                                )
                            }
                            .tag(SidebarSelection.worktree(worktree.id))
                        }
                    } label: {
                        ProjectRow(name: appState.projectName) {
                            showCommandPalette(for: .project(appState.projectName))
                        }
                    }
                } else {
                    Text("No project open")
                        .font(PurePointTheme.smallFont)
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .listStyle(.sidebar)
        .safeAreaInset(edge: .bottom) {
            SidebarFooter(selection: selection)
        }
        .onChange(of: appState.worktrees) { _, newValue in
            let currentIds = Set(newValue.map(\.id))
            if !didInitialExpand {
                didInitialExpand = true
                expandedWorktrees = currentIds
            } else {
                expandedWorktrees.formIntersection(currentIds)
            }
        }
    }

    /// Root agents visible in the sidebar (excludes grid children).
    private var visibleRootAgents: [AgentModel] {
        let hidden = gridState.childAgentIds
        return appState.rootAgents.filter { !hidden.contains($0.id) }
    }

    /// Worktree agents visible in the sidebar (excludes grid children).
    private func visibleAgents(in worktree: WorktreeModel) -> [AgentModel] {
        let hidden = gridState.childAgentIds
        return worktree.agents.filter { !hidden.contains($0.id) }
    }

    private func binding(for id: String) -> Binding<Bool> {
        Binding(
            get: { expandedWorktrees.contains(id) },
            set: { isExpanded in
                if isExpanded {
                    expandedWorktrees.insert(id)
                } else {
                    expandedWorktrees.remove(id)
                }
            }
        )
    }

    private func showCommandPalette(for selection: SidebarSelection) {
        let state = appState
        CommandPalettePanel.show(relativeTo: NSApp.keyWindow) { variant, prompt in
            state.createAgent(variant: variant, prompt: prompt, selection: selection)
        }
    }
}
