import SwiftUI

struct ProjectDetailView: View {
    let project: ProjectState
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
        .task(id: project.projectRoot) {
            diffState.loadForProject(projectRoot: project.projectRoot)
        }
        .onDisappear {
            diffState.stopWatching()
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 8) {
            Image(systemName: "folder.fill")
                .font(.system(size: 14))
                .foregroundStyle(.secondary)

            Text(project.projectName)
                .font(.system(size: 14, weight: .semibold))

            Text("\(project.worktrees.count) worktrees")
                .font(.system(size: 12))
                .foregroundStyle(.secondary)

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
            Text("Pull Requests")
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
            } else {
                prListContent
            }
        }
    }

    // MARK: - PR List

    private var prListContent: some View {
        VStack(spacing: 0) {
            if diffState.isLoadingPRs && diffState.pullRequests.isEmpty {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if diffState.pullRequests.isEmpty {
                VStack(spacing: 12) {
                    Image(systemName: "pull.request")
                        .font(.system(size: 28))
                        .foregroundStyle(.secondary)
                    Text("No open pull requests")
                        .font(.system(size: 13))
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if diffState.selectedPR == nil {
                // Show PR list for selection
                prListView
            } else {
                // Show selected PR with back button + diff
                selectedPRView
            }
        }
    }

    private var prListView: some View {
        ScrollView {
            LazyVStack(spacing: 1) {
                ForEach(diffState.pullRequests) { pr in
                    Button {
                        diffState.selectPR(pr)
                    } label: {
                        PRRowView(pr: pr)
                    }
                    .buttonStyle(.plain)
                    Divider()
                }
            }
        }
    }

    private var selectedPRView: some View {
        VStack(spacing: 0) {
            // Back bar with PR info
            HStack(spacing: 8) {
                Button {
                    diffState.selectedPR = nil
                    diffState.prDiff = nil
                } label: {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 11, weight: .semibold))
                }
                .buttonStyle(.borderless)

                if let pr = diffState.selectedPR {
                    Text("#\(pr.number)")
                        .font(.system(size: 12, weight: .semibold, design: .monospaced))
                        .foregroundStyle(.secondary)

                    Text(pr.title)
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)

                    Spacer()

                    if let url = URL(string: pr.url) {
                        Link(destination: url) {
                            Image(systemName: "arrow.up.right.square")
                                .font(.system(size: 12))
                        }
                        .help("Open in browser")
                    }
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

}
