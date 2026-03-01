import SwiftUI

struct ProjectTreeSection: View {
    let projects: [MockProject]
    @Binding var selection: SidebarSelection?
    @State private var expandedProjects: Set<String>
    @State private var expandedWorktrees: Set<String>

    init(projects: [MockProject], selection: Binding<SidebarSelection?>) {
        self.projects = projects
        self._selection = selection
        // Start with all projects and worktrees expanded
        self._expandedProjects = State(initialValue: Set(projects.map(\.id)))
        self._expandedWorktrees = State(initialValue: Set(projects.flatMap(\.worktrees).map(\.id)))
    }

    var body: some View {
        List(selection: $selection) {
            ForEach(projects) { project in
                DisclosureGroup(
                    isExpanded: binding(for: project.id, in: $expandedProjects)
                ) {
                    ForEach(project.worktrees) { worktree in
                        DisclosureGroup(
                            isExpanded: binding(for: worktree.id, in: $expandedWorktrees)
                        ) {
                            ForEach(worktree.agents) { agent in
                                AgentRow(agent: agent)
                                    .tag(SidebarSelection.agent(agent.id))
                            }
                            ForEach(worktree.terminals) { terminal in
                                TerminalRow(terminal: terminal)
                                    .tag(SidebarSelection.terminal(terminal.id))
                            }
                        } label: {
                            WorktreeRow(worktree: worktree)
                        }
                        .tag(SidebarSelection.worktree(worktree.id))
                    }
                } label: {
                    ProjectRow(project: project)
                }
                .tag(SidebarSelection.project(project.id))
            }
        }
        .listStyle(.sidebar)
    }

    private func binding(for id: String, in set: Binding<Set<String>>) -> Binding<Bool> {
        Binding(
            get: { set.wrappedValue.contains(id) },
            set: { isExpanded in
                if isExpanded {
                    set.wrappedValue.insert(id)
                } else {
                    set.wrappedValue.remove(id)
                }
            }
        )
    }
}

#Preview {
    ProjectTreeSection(
        projects: MockData.projects,
        selection: .constant(nil)
    )
    .frame(width: 240, height: 300)
    .preferredColorScheme(.dark)
}
