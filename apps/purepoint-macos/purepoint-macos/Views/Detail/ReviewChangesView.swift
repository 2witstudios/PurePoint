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
            content
        }
    }
    @ViewBuilder private var content: some View {
        switch state.activeTab {
        case .branch:
            patches(state.branchDiff, loading: state.isLoadingBranch, empty: "No changes since \(state.comparisonBase)", error: state.branchError)
        case .unstaged:
            if state.isLoadingUnstaged && state.stagedDiff.isEmpty && state.unstagedDiff.isEmpty && state.untrackedDiff.isEmpty { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
            else if state.stagedDiff.isEmpty && state.unstagedDiff.isEmpty && state.untrackedDiff.isEmpty { patches([], loading: false, empty: "Working tree is clean", error: state.localError) }
            else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        if let error = state.localError { Text(error).font(.system(size: 12)).foregroundStyle(.orange).textSelection(.enabled); Button("Retry") { state.refresh() } }
                        section("Staged", files: state.stagedDiff)
                        section("Unstaged", files: state.unstagedDiff)
                        section("Untracked", files: state.untrackedDiff)
                    }.padding(16)
                }
            }
        case .commits:
            if let error = state.branchError { errorView(error) }
            else if let commit = state.selectedCommit {
                VStack(spacing: 0) {
                    HStack { Button { state.selectedCommit = nil } label: { Image(systemName: "chevron.left") }; Text(commit.subject).font(.system(size: 13, weight: .medium)); Spacer(); Text(String(commit.sha.prefix(8))).font(.system(size: 11, design: .monospaced)).foregroundStyle(.secondary) }.padding(14)
                    Divider()
                    patches(state.commitDiff, loading: state.isLoadingCommit, empty: "This commit has no file changes", error: nil)
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
                        if state.commits.isEmpty { Text(state.isLoadingBranch ? "Loading commits…" : "No commits beyond \(state.comparisonBase)").foregroundStyle(.secondary).padding(30) }
                    }
                }
            }
        case .prDiffs:
            VStack(spacing: 0) {
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
                    DiffListView(diff: state.prDiff, isLoading: state.isLoadingPRDiff, emptyMessage: "No changes in PR", error: state.prDiff == nil ? state.prError : nil, onRetry: { state.refresh() })
                }
            } else if state.isLoadingPRs && state.pullRequests.isEmpty { ProgressView().frame(maxWidth: .infinity, maxHeight: .infinity) }
            else if state.pullRequests.isEmpty { Text("No open pull requests").foregroundStyle(.secondary).frame(maxWidth: .infinity, maxHeight: .infinity) }
            else { ScrollView { LazyVStack(spacing: 0) { ForEach(state.pullRequests) { pr in Button { state.selectPR(pr) } label: { PRRowView(pr: pr) }.buttonStyle(.plain); Divider() } } } }
            }
        }
    }
    private func patches(_ files: [FileDiff], loading: Bool, empty: String, error: String?) -> some View {
        DiffListView(diff: loading && files.isEmpty ? nil : DiffData(files: files), isLoading: loading, emptyMessage: empty, error: error, onRetry: { state.refresh() })
    }
    @ViewBuilder private func section(_ title: String, files: [FileDiff]) -> some View {
        if !files.isEmpty { Text("\(title) · \(files.count)").font(.system(size: 12, weight: .semibold)).foregroundStyle(.secondary).padding(.top, 8); ForEach(files) { DiffCardView(fileDiff: $0) } }
    }
    private func errorView(_ message: String) -> some View { patches([], loading: false, empty: "", error: message) }
}
