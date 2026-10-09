import SwiftUI

struct DetailView: View {
    @Binding var selection: SidebarSelection?
    @Environment(AppState.self) private var appState
    @Environment(WorkspaceRegistry.self) private var registry

    var body: some View {
        Group {
            if let selection {
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
        // Every workspace renders as a grid. A one-pane workspace is a grid of one, which
        // is why selecting its row drops the user straight into the terminal.
        case .workspace(let id):
            if registry.workspace(id: id) != nil {
                PaneGridView(workspaceId: id)
            } else {
                placeholderView(icon: "rectangle.split.2x2", title: "Workspace not found")
            }

        case .nav(let item):
            switch item {
            case .dashboard:
                PointGuardView(selection: $selection)
            case .agents:
                AgentsHubView()
            case .schedule:
                ScheduleView()
            }

        case .worktree(let id):
            if let wt = appState.projectState(forWorktreeId: id)?.worktrees.first(where: { $0.id == id }) {
                WorktreeDetailView(worktree: wt, project: appState.projectState(forWorktreeId: id))
            } else {
                placeholderView(icon: "arrow.triangle.branch", title: "Worktree not found")
            }

        case .channel(let root):
            if let project = appState.projectState(forRoot: root) { ProjectChannelView(project: project) }
            else { placeholderView(icon: "bubble.left.and.bubble.right", title: "Project channel") }
        case .project(let root):
            if let project = appState.projectState(forRoot: root) {
                ProjectDetailView(project: project, selection: $selection)
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
