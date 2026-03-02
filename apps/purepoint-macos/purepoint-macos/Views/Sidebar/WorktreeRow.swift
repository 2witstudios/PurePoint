import SwiftUI

struct WorktreeRow: View {
    let worktree: WorktreeModel
    var onAddAgent: () -> Void
    var onAddTerminal: () -> Void
    @Environment(AppState.self) private var appState
    @State private var showKillConfirmation = false

    var body: some View {
        Label {
            HStack {
                Text(worktree.branch)
                    .font(PurePointTheme.treeFont)

                Spacer()

                Menu {
                    Button("New Agent") { onAddAgent() }
                    Button("New Terminal") { onAddTerminal() }
                } label: {
                    Image(systemName: "plus.circle")
                        .font(.system(size: 11))
                        .foregroundStyle(.secondary)
                }
                .menuStyle(.borderlessButton)
                .fixedSize()

                if worktree.agents.count > 0 {
                    Text("\(worktree.agents.count)")
                        .font(PurePointTheme.smallFont)
                        .foregroundStyle(.secondary)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 1)
                        .background(.quaternary, in: Capsule())
                }
            }
        } icon: {
            Image(systemName: "arrow.triangle.branch")
                .foregroundStyle(.secondary)
        }
        .contextMenu {
            Button("Kill All Agents", role: .destructive) {
                showKillConfirmation = true
            }
        }
        .confirmationDialog(
            "Kill all agents in \"\(worktree.branch)\"?",
            isPresented: $showKillConfirmation,
            titleVisibility: .visible
        ) {
            Button("Kill All", role: .destructive) {
                appState.killWorktreeAgents(worktree.id)
            }
        }
    }
}
