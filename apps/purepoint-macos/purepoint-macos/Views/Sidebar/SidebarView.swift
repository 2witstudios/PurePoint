import SwiftUI

struct SidebarView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState
    @State private var expandedProjects: Set<String> = []
    @State private var expandedWorktrees: Set<String> = []

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
                    ForEach(appState.projects) { project in
                        DisclosureGroup(
                            isExpanded: projectBinding(for: project.projectRoot)
                        ) {
                            ForEach(visibleRootAgents(in: project)) { agent in
                                AgentRow(agent: agent, isGridOwner: agent.id == gridState.ownerAgentId)
                                    .tag(SidebarSelection.agent(agent.id))
                            }

                            ForEach(project.worktrees) { worktree in
                                DisclosureGroup(
                                    isExpanded: worktreeBinding(for: worktree.id)
                                ) {
                                    ForEach(visibleAgents(in: worktree, project: project)) { agent in
                                        AgentRow(agent: agent, isGridOwner: agent.id == gridState.ownerAgentId)
                                            .tag(SidebarSelection.agent(agent.id))
                                    }
                                } label: {
                                    WorktreeRow(
                                        worktree: worktree,
                                        onAddAgent: { showCommandPalette(for: project, selection: .worktree(worktree.id)) },
                                        onAddTerminal: {
                                            project.createAgent(variant: .terminal, prompt: nil, selection: .worktree(worktree.id))
                                        }
                                    )
                                }
                                .tag(SidebarSelection.worktree(worktree.id))
                            }
                        } label: {
                            ProjectRow(name: project.projectName) {
                                showCommandPalette(for: project, selection: nil, includeWorktree: true)
                            }
                        }
                        .tag(SidebarSelection.project(project.projectRoot))
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
        .onChange(of: appState.projects.map(\.projectRoot)) { oldRoots, newRoots in
            let oldSet = Set(oldRoots)
            let newSet = Set(newRoots)
            expandedProjects.formUnion(newSet.subtracting(oldSet))
            expandedProjects.formIntersection(newSet)
        }
        .onChange(of: allWorktreeIds) { oldIds, newIds in
            let oldSet = Set(oldIds)
            let newSet = Set(newIds)
            expandedWorktrees.formUnion(newSet.subtracting(oldSet))
            expandedWorktrees.formIntersection(newSet)
        }
    }

    private var allWorktreeIds: [String] {
        appState.projects.flatMap { $0.worktrees.map(\.id) }
    }

    /// Root agents visible in the sidebar (excludes grid children for the grid's project).
    private func visibleRootAgents(in project: ProjectState) -> [AgentModel] {
        guard project.projectRoot == gridState.projectRoot else {
            return project.rootAgents
        }
        let hidden = gridState.childAgentIds
        return project.rootAgents.filter { !hidden.contains($0.id) }
    }

    /// Worktree agents visible in the sidebar (excludes grid children for the grid's project).
    private func visibleAgents(in worktree: WorktreeModel, project: ProjectState) -> [AgentModel] {
        guard project.projectRoot == gridState.projectRoot else {
            return worktree.agents
        }
        let hidden = gridState.childAgentIds
        return worktree.agents.filter { !hidden.contains($0.id) }
    }

    private func projectBinding(for root: String) -> Binding<Bool> {
        Binding(
            get: { expandedProjects.contains(root) },
            set: { isExpanded in
                if isExpanded {
                    expandedProjects.insert(root)
                } else {
                    expandedProjects.remove(root)
                }
            }
        )
    }

    private func worktreeBinding(for id: String) -> Binding<Bool> {
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

    private func showCommandPalette(for project: ProjectState, selection: SidebarSelection?, includeWorktree: Bool = false) {
        let variants = includeWorktree ? AgentVariant.variantsWithWorktree : AgentVariant.allVariants
        CommandPalettePanel.show(relativeTo: NSApp.keyWindow, variants: variants) { variant, prompt, name in
            project.createAgent(variant: variant, prompt: prompt, name: name, selection: selection)
        }
    }
}
