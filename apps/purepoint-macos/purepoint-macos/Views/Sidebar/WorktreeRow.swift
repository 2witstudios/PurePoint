import SwiftUI

struct WorktreeRow: View {
    let worktree: WorktreeModel

    var body: some View {
        Label {
            HStack {
                Text(worktree.branch)
                    .font(PurePointTheme.treeFont)

                Spacer()

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
    }
}
