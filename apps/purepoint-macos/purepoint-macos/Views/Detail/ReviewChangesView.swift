import SwiftUI

struct ReviewChangesView: View {
    @Bindable var state: DiffState
    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Picker("Review", selection: $state.activeTab) {
                    Text("Branch Changes").tag(DiffTab.branch)
                    Text("Uncommitted").tag(DiffTab.unstaged)
                    Text("Commits").tag(DiffTab.commits)
                    Text("PR Diffs").tag(DiffTab.prDiffs)
                }.pickerStyle(.segmented).frame(maxWidth: 520)
                Spacer(minLength: 8)
                if state.activeTab == .branch || state.activeTab == .commits {
                    Menu {
                        ForEach(state.availableBases, id: \.self) { base in Button(base) { state.setComparisonBase(base) } }
                    } label: { Text("vs \(state.comparisonBase)").font(.system(size: 11, design: .monospaced)) }.menuStyle(.borderlessButton).fixedSize()
                }
            }.padding(.horizontal, 16).padding(.vertical, 12)
            Divider()
            content.id(state.reviewIdentity.uuidString + state.activeTab.rawValue + state.comparisonBase + (state.selectedCommit?.sha ?? "") + String(state.selectedPR?.number ?? 0))
        }
    }
    @ViewBuilder private var content: some View {
        switch state.activeTab {
        case .branch:
            patches(state.branchDiff, loading: state.isInitiallyLoadingBranch, empty: "No changes since \(state.comparisonBase)", error: state.branchError)
        case .unstaged:
            DiffListView(diff: state.isInitiallyLoadingUnstaged ? nil : DiffData(files: []),
                         isLoading: state.isInitiallyLoadingUnstaged, emptyMessage: "Working tree is clean", error: state.localError,
                         onRetry: { state.refresh() }, groups: [
                            DiffFileGroup(title: "Staged", files: state.stagedDiff),
                            DiffFileGroup(title: "Unstaged", files: state.unstagedDiff),
                            DiffFileGroup(title: "Untracked", files: state.untrackedDiff)
                         ], loadPage: { file, page in try await state.loadPage(file, after: page) })
                .id("local")
        case .commits:
            if let error = state.branchError { errorView(error) }
            else if let commit = state.selectedCommit {
                VStack(spacing: 0) {
                    HStack { Button { state.selectedCommit = nil } label: { Image(systemName: "chevron.left") }; Text(commit.subject).font(.system(size: 13, weight: .medium)); Spacer(); Text(String(commit.sha.prefix(8))).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary) }.padding(14)
                    Divider()
                    patches(state.commitDiff, loading: state.isInitiallyLoadingCommit, empty: "This commit has no file changes", error: state.commitError)
                }
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(state.commits) { commit in
                            Button { state.selectCommit(commit) } label: {
                                HStack(spacing: 14) {
                                    Image(systemName: "point.3.connected.trianglepath.dotted").foregroundStyle(.secondary)
                                    VStack(alignment: .leading, spacing: 5) { Text(commit.subject).font(.system(size: 13, weight: .medium)); Text("\(commit.author) · \(commit.date)").font(.system(size: 11)).foregroundStyle(.secondary) }
                                    Spacer(); Text(String(commit.sha.prefix(8))).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary)
                                }.padding(16).contentShape(Rectangle())
                            }.buttonStyle(.plain)
                            Divider()
                        }
                        if state.commits.isEmpty { Text(state.isInitiallyLoadingBranch ? "Loading commits…" : "No commits beyond \(state.comparisonBase)").foregroundStyle(.secondary).padding(30) }
                    }
                }
            }
        case .prDiffs:
            VStack(spacing: 0) {
            if state.isPRDiffStale {
                HStack {
                    Text(state.isLoadingPRDiff ? "Showing the previous PR snapshot. Refreshing changes…" : "Showing the previous PR snapshot. Refresh to load current changes.").font(.system(size: 12))
                    Spacer()
                    Button("Retry") { if let pr = state.selectedPR { state.selectPR(pr) } }
                }.foregroundStyle(.secondary).padding(12)
                Divider()
            }
            if let error = state.prError, !state.pullRequests.isEmpty || state.prDiff != nil {
                HStack { Image(systemName: "exclamationmark.triangle"); Text(error).font(.system(size: 12)).textSelection(.enabled); Spacer(); Button("Retry") { state.refresh() } }.foregroundStyle(.secondary).padding(12).background(Color.orange.opacity(0.08))
                Divider()
            }
            if let error = state.prError, state.pullRequests.isEmpty && state.prDiff == nil { errorView(error) }
            else if !state.ghAvailable && state.pullRequests.isEmpty && state.prDiff == nil { GHUnavailableView() }
            else if let pr = state.selectedPR {
                VStack(spacing: 0) {
                    HStack { Button { state.selectedPR = nil; state.prDiff = nil } label: { Image(systemName: "chevron.left") }; Text("#\(pr.number) \(pr.title)").font(.system(size: 13, weight: .medium)); Spacer(); if let url = URL(string: pr.url) { Link(destination: url) { Image(systemName: "arrow.up.right.square") } } }.padding(14)
                    Divider()
                    DiffListView(diff: state.prDiff, isLoading: state.isLoadingPRDiff, emptyMessage: "No changes in PR", error: state.prDiff == nil ? state.prError : nil, onRetry: { state.refresh() }, loadPage: { file, page in try await state.loadPage(file, after: page) }).id("pr-\(pr.number)")
                }
            } else if state.isLoadingPRs && state.pullRequests.isEmpty { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
            else if state.pullRequests.isEmpty { Text("No open pull requests").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity) }
            else { ScrollView { LazyVStack(spacing: 0) { ForEach(state.pullRequests) { pr in Button { state.selectPR(pr) } label: { PRRowView(pr: pr) }.buttonStyle(.plain); Divider() } } } }
            }
        }
    }
    private func patches(_ files: [FileDiff], loading: Bool, empty: String, error: String?) -> some View {
        DiffListView(diff: loading && files.isEmpty ? nil : DiffData(files: files), isLoading: loading, emptyMessage: empty, error: error, onRetry: { state.refresh() }, loadPage: { file, page in try await state.loadPage(file, after: page) })
    }
    private func errorView(_ message: String) -> some View { patches([], loading: false, empty: "", error: message) }
}
