import SwiftUI

struct WorktreeDetailView: View {
    let worktree: WorktreeModel
    @State private var diffState = DiffState()

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            tabBar
            Divider()
            content
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .task(id: worktree.id) {
            diffState.loadForWorktree(worktree)
        }
        .onDisappear {
            diffState.stopWatching()
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 8) {
            Image(systemName: "arrow.triangle.branch")
                .font(.system(size: 14))
                .foregroundStyle(.secondary)

            Text(worktree.name)
                .font(.system(size: 14, weight: .semibold))

            Text(worktree.branch)
                .font(.system(size: 12, design: .monospaced))
                .foregroundStyle(.secondary)
                .padding(.horizontal, 6)
                .padding(.vertical, 2)
                .background(Color.secondary.opacity(0.1))
                .clipShape(RoundedRectangle(cornerRadius: 4))

            Spacer()

            Button {
                diffState.refresh()
            } label: {
                Image(systemName: "arrow.clockwise")
                    .font(.system(size: 12))
            }
            .buttonStyle(.borderless)
            .help("Refresh")
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
    }

    // MARK: - Tab Bar

    private var tabBar: some View {
        Picker("", selection: $diffState.activeTab) {
            Text("Unstaged Changes")
                .tag(DiffTab.unstaged)
            Text("PR Diffs")
                .tag(DiffTab.prDiffs)
        }
        .pickerStyle(.segmented)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
    }

    // MARK: - Content

    @ViewBuilder
    private var content: some View {
        switch diffState.activeTab {
        case .unstaged:
            DiffListView(
                diff: diffState.unstagedDiff,
                isLoading: diffState.isLoadingUnstaged,
                emptyMessage: "No unstaged changes",
                error: diffState.error,
                onRetry: { diffState.refresh() }
            )

        case .prDiffs:
            if !diffState.ghAvailable {
                GHUnavailableView()
            } else if diffState.isLoadingPRs && diffState.pullRequests.isEmpty {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if diffState.pullRequests.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "pull.request")
                        .font(.system(size: 28))
                        .foregroundStyle(.secondary)
                    Text("No open pull requests for this branch")
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                prContent
            }
        }
    }

    // MARK: - PR Content

    private var prContent: some View {
        VStack(spacing: 0) {
            // PR selector
            HStack {
                Picker("Pull Request", selection: prBinding) {
                    ForEach(diffState.pullRequests) { pr in
                        Text("#\(pr.number) \(pr.title)")
                            .tag(pr.number)
                    }
                }
                .labelsHidden()

                if let pr = diffState.selectedPR, let url = URL(string: pr.url) {
                    Link(destination: url) {
                        Image(systemName: "arrow.up.right.square")
                            .font(.system(size: 12))
                    }
                    .help("Open in browser")
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 8)

            Divider()

            DiffListView(
                diff: diffState.prDiff,
                isLoading: diffState.isLoadingPRDiff,
                emptyMessage: "No changes in PR",
                error: nil
            )
        }
    }

    private var prBinding: Binding<Int> {
        Binding(
            get: { diffState.selectedPR?.number ?? 0 },
            set: { number in
                if let pr = diffState.pullRequests.first(where: { $0.number == number }) {
                    diffState.selectPR(pr)
                }
            }
        )
    }

}
