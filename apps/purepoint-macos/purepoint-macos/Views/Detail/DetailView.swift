import SwiftUI

struct DetailView: View {
    let selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState

    var body: some View {
        Group {
            if gridState.isActive {
                PaneGridView()
            } else if let selection {
                selectedContent(selection)
            } else {
                placeholderContent
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private var placeholderContent: some View {
        VStack(spacing: 12) {
            Image(systemName: "sidebar.left")
                .font(.system(size: 40))
                .foregroundStyle(.tertiary)
            Text("Select an item")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func selectedContent(_ selection: SidebarSelection) -> some View {
        switch selection {
        case .agent(let id):
            if let agent = appState.agent(byId: id) {
                ZStack(alignment: .topTrailing) {
                    TerminalContainerView(agent: agent)
                    SinglePaneSplitOverlay(agentId: id)
                }
            } else {
                placeholderView(icon: "cpu", title: "Agent not found")
            }

        case .nav(let item):
            placeholderView(icon: item.icon, title: item.title)

        case .worktree(let id):
            if let wt = appState.projectState(forWorktreeId: id)?.worktrees.first(where: { $0.id == id }) {
                WorktreeDetailView(worktree: wt)
            } else {
                placeholderView(icon: "arrow.triangle.branch", title: "Worktree not found")
            }

        case .terminal(let id):
            placeholderView(icon: "terminal", title: id)

        case .project(let root):
            if let project = appState.projectState(forRoot: root) {
                ProjectDetailView(project: project)
            } else {
                placeholderView(icon: "folder.fill", title: "Project")
            }
        }
    }

    private func placeholderView(icon: String, title: String) -> some View {
        VStack(spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 40))
                .foregroundStyle(.secondary)
            Text(title)
                .font(.title3)
                .foregroundStyle(.primary)
        }
    }
}

/// Split overlay for single-pane agent view — enters grid mode on click.
private struct SinglePaneSplitOverlay: View {
    let agentId: String
    @State private var isHovered = false
    @Environment(AppState.self) private var appState
    @Environment(GridState.self) private var gridState

    var body: some View {
        ZStack(alignment: .topTrailing) {
            Color.clear

            if isHovered {
                HStack(spacing: 4) {
                    overlayButton(icon: "rectangle.split.2x1", tooltip: "Split Right") {
                        enterGrid(axis: .vertical)
                    }
                    overlayButton(icon: "rectangle.split.1x2", tooltip: "Split Below") {
                        enterGrid(axis: .horizontal)
                    }
                }
                .padding(6)
                .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 6))
                .padding(8)
                .transition(.opacity.animation(.easeInOut(duration: 0.2)))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Rectangle())
        .onHover { hovering in
            withAnimation(.easeInOut(duration: 0.2).delay(hovering ? 0 : 0.3)) {
                isHovered = hovering
            }
        }
    }

    private func enterGrid(axis: PaneSplitNode.Axis) {
        if let project = appState.projectState(forAgentId: agentId) {
            gridState.projectRoot = project.projectRoot
        }
        gridState.enterGridMode(agentId: agentId, axis: axis)
        gridState.pendingPaletteLeafId = gridState.focusedLeafId
    }

    private func overlayButton(icon: String, tooltip: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon)
                .font(.system(size: 11))
                .frame(width: 20, height: 20)
        }
        .buttonStyle(.plain)
        .help(tooltip)
    }
}
