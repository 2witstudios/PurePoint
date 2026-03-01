import SwiftUI

struct ProjectTreeSection: View {
    @Environment(AppState.self) private var appState
    @Binding var selection: SidebarSelection?
    @State private var expandedWorktrees: Set<String> = []
    @State private var didInitialExpand = false

    var body: some View {
        List(selection: $selection) {
            if appState.isLoaded {
                DisclosureGroup(
                    isExpanded: .constant(true)
                ) {
                    // Root agents (not in any worktree)
                    ForEach(appState.rootAgents) { agent in
                        AgentRow(agent: agent)
                            .tag(SidebarSelection.agent(agent.id))
                    }

                    ForEach(appState.worktrees) { worktree in
                        DisclosureGroup(
                            isExpanded: binding(for: worktree.id)
                        ) {
                            ForEach(worktree.agents) { agent in
                                AgentRow(agent: agent)
                                    .tag(SidebarSelection.agent(agent.id))
                            }
                        } label: {
                            WorktreeRow(worktree: worktree)
                        }
                        .tag(SidebarSelection.worktree(worktree.id))
                    }
                } label: {
                    ProjectRow(name: appState.projectName)
                }
            } else {
                Text("No project open")
                    .font(PurePointTheme.smallFont)
                    .foregroundStyle(PurePointTheme.tertiaryText)
            }
        }
        .listStyle(.sidebar)
        .onChange(of: appState.worktrees) { _, newValue in
            if !didInitialExpand {
                didInitialExpand = true
                expandedWorktrees = Set(newValue.map(\.id))
            }
        }
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
}
