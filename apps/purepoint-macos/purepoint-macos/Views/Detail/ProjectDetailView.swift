import SwiftUI

struct ProjectDetailView: View {
    let project: ProjectState
    @State private var diffState = DiffState()
    @State private var fileTreeState = FileTreeState()
    @State private var editorState = EditorState()
    @State private var showFileTree = true
    @State private var sidebarRatio: CGFloat = 0.22
    @State private var saveError: String?

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            DraggableSplit(
                axis: .vertical,
                ratio: showFileTree ? sidebarRatio : 0,
                onRatioChanged: { sidebarRatio = $0 }
            ) {
                FileTreeSidebarView(
                    fileTreeState: fileTreeState,
                    showFileTree: $showFileTree,
                    onFileSelected: { path, name in
                        editorState.openFile(path: path, name: name)
                    }
                )
            } second: {
                VStack(spacing: 0) {
                    if let file = editorState.currentFile, !editorState.showChanges {
                        fileBreadcrumb(file)
                    }
                    Divider()
                    editorContent
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay(alignment: .top) {
            externalChangeBanner
        }
        .task(id: project.projectRoot) {
            editorState.stopWatching()
            editorState = EditorState()
            diffState.loadForProject(projectRoot: project.projectRoot)
            fileTreeState.load(worktreePath: project.projectRoot)
        }
        .onDisappear {
            diffState.stopWatching()
            fileTreeState.stopWatching()
            editorState.stopWatching()
        }
    }

    // MARK: - Header

    private var header: some View {
        HStack(spacing: 8) {
            if !showFileTree {
                Button {
                    withAnimation(.easeInOut(duration: 0.2)) {
                        showFileTree = true
                    }
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .buttonStyle(.plain)
                .help("Show file tree")
            }

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
                editorState.showChanges.toggle()
            } label: {
                Image(systemName: "doc.badge.plus")
                    .font(.system(size: 12))
                    .foregroundStyle(editorState.showChanges ? .primary : .secondary)
            }
            .buttonStyle(.borderless)
            .help("Toggle changes view")

            Button {
                diffState.refresh()
                fileTreeState.refresh()
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

    // MARK: - File Breadcrumb

    private func fileBreadcrumb(_ file: EditorTab) -> some View {
        HStack(spacing: 6) {
            Image(systemName: file.language.icon)
                .font(.system(size: 10))
                .foregroundStyle(.secondary)
            Text(file.name)
                .font(.system(size: 11))
                .lineLimit(1)
            if file.isDirty {
                Circle()
                    .fill(Color.primary.opacity(0.4))
                    .frame(width: 6, height: 6)
            }
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 4)
        .background(Color(nsColor: Theme.cardHeaderBackground))
    }

    // MARK: - Editor Content

    @ViewBuilder
    private var editorContent: some View {
        if editorState.showChanges {
            changesContent
        } else if let file = editorState.currentFile {
            if file.isBinary {
                binaryPlaceholder(file)
            } else {
                EditorContentRepresentable(
                    content: file.content,
                    language: file.language,
                    isBinary: false,
                    isEditable: true,
                    onContentChanged: { newContent in
                        editorState.updateContent(content: newContent)
                    },
                    onSave: {
                        Task {
                            do {
                                try await editorState.saveFile()
                                saveError = nil
                            } catch {
                                saveError = "Failed to save \(file.name): \(error.localizedDescription)"
                            }
                        }
                    }
                )
            }
        } else {
            editorPlaceholder
        }
    }

    // MARK: - Changes Content

    private var changesContent: some View {
        VStack(spacing: 0) {
            diffTabBar
            Divider()
            diffContent
        }
    }

    private var diffTabBar: some View {
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

    @ViewBuilder
    private var diffContent: some View {
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
                prListView
            } else {
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

    // MARK: - Placeholders

    private var editorPlaceholder: some View {
        VStack(spacing: 12) {
            Image(systemName: "doc.text")
                .font(.system(size: 28))
                .foregroundStyle(.tertiary)
            Text("Select a file to edit")
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func binaryPlaceholder(_ tab: EditorTab) -> some View {
        VStack(spacing: 12) {
            Image(systemName: "doc.fill")
                .font(.system(size: 28))
                .foregroundStyle(.tertiary)
            Text("Binary file")
                .font(.system(size: 13))
                .foregroundStyle(.secondary)
            Text(tab.name)
                .font(.system(size: 11, design: .monospaced))
                .foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: - Banners

    @ViewBuilder
    private var externalChangeBanner: some View {
        if editorState.externalChangeAlert != nil, let file = editorState.currentFile {
            bannerView(
                icon: "exclamationmark.triangle.fill",
                iconColor: .yellow,
                message: "\(file.name) changed on disk."
            ) {
                Button("Reload") {
                    editorState.reloadFile()
                }
                .buttonStyle(.bordered)
                .controlSize(.small)
                Button("Dismiss") {
                    editorState.dismissExternalChange()
                }
                .buttonStyle(.plain)
                .font(.system(size: 11))
            }
        }

        if let error = saveError {
            bannerView(
                icon: "xmark.circle.fill",
                iconColor: .red,
                message: error
            ) {
                Button("Dismiss") {
                    saveError = nil
                }
                .buttonStyle(.plain)
                .font(.system(size: 11))
            }
        }
    }

    private func bannerView<Actions: View>(
        icon: String,
        iconColor: Color,
        message: String,
        @ViewBuilder actions: () -> Actions
    ) -> some View {
        HStack {
            Image(systemName: icon)
                .foregroundStyle(iconColor)
            Text(message)
                .font(.system(size: 12))
            actions()
        }
        .padding(8)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 8))
        .padding(8)
    }
}
