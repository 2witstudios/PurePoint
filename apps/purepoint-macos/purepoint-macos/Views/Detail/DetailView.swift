import SwiftUI

struct DetailView: View {
    let selection: SidebarSelection?

    var body: some View {
        Group {
            if let selection {
                selectedContent(selection)
            } else {
                placeholderContent
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(PurePointTheme.contentBackground)
    }

    private var placeholderContent: some View {
        VStack(spacing: 12) {
            Image(systemName: "sidebar.left")
                .font(.system(size: 40))
                .foregroundStyle(PurePointTheme.tertiaryText)
            Text("Select an item")
                .font(.title3)
                .foregroundStyle(PurePointTheme.secondaryText)
        }
    }

    @ViewBuilder
    private func selectedContent(_ selection: SidebarSelection) -> some View {
        VStack(spacing: 12) {
            Image(systemName: icon(for: selection))
                .font(.system(size: 40))
                .foregroundStyle(PurePointTheme.secondaryText)
            Text(title(for: selection))
                .font(.title3)
                .foregroundStyle(PurePointTheme.primaryText)
        }
    }

    private func icon(for selection: SidebarSelection) -> String {
        switch selection {
        case .nav(let item):      item.icon
        case .project:            "folder.fill"
        case .worktree:           "arrow.triangle.branch"
        case .agent:              "cpu"
        case .terminal:           "terminal"
        }
    }

    private func title(for selection: SidebarSelection) -> String {
        switch selection {
        case .nav(let item):      item.title
        case .project(let id):    MockData.projects.first { $0.id == id }?.name ?? "Project"
        case .worktree(let id):   MockData.projects.flatMap(\.worktrees).first { $0.id == id }?.branch ?? "Worktree"
        case .agent(let id):      MockData.projects.flatMap(\.worktrees).flatMap(\.agents).first { $0.id == id }?.name ?? "Agent"
        case .terminal(let id):   MockData.projects.flatMap(\.worktrees).flatMap(\.terminals).first { $0.id == id }?.name ?? "Terminal"
        }
    }
}

#Preview("Empty") {
    DetailView(selection: nil)
        .frame(width: 500, height: 400)
        .preferredColorScheme(.dark)
}

#Preview("Selected") {
    DetailView(selection: .nav(.dashboard))
        .frame(width: 500, height: 400)
        .preferredColorScheme(.dark)
}
