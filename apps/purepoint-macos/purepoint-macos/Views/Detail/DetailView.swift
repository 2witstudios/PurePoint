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
                TerminalContainerView(agent: agent)
            } else {
                placeholderView(icon: "cpu", title: "Agent not found")
            }

        case .nav(let item):
            placeholderView(icon: item.icon, title: item.title)

        case .worktree(let id):
            let wt = appState.worktrees.first { $0.id == id }
            placeholderView(icon: "arrow.triangle.branch", title: wt?.branch ?? "Worktree")

        case .terminal(let id):
            placeholderView(icon: "terminal", title: id)

        case .project:
            placeholderView(icon: "folder.fill", title: appState.projectName)
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
