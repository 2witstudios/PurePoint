import SwiftUI

struct WorktreeDetailView: View {
    let worktree: WorktreeModel
    var project: ProjectState?
    @State private var showChannel = false
    @State private var diffState = DiffState()
    @State private var fileTreeState = FileTreeState()
    @State private var editorState = EditorState()
    @State private var showFileTree = false
    @State private var sidebarRatio: CGFloat = 0.22
    @State private var saveError: String?

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            HStack(spacing: 0) {
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
                    Divider()
                    editorContent
                }
            }
            if showChannel, let project {
                Divider()
                ProjectChannelView(project: project, compact: true).frame(minWidth: 280, idealWidth: 340, maxWidth: 400)
            }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .overlay(alignment: .top) {
            externalChangeBanner
        }
        .task(id: worktree.id) {
            editorState.stopWatching()
            editorState = EditorState()
            diffState.loadForWorktree(worktree)
            fileTreeState.load(worktreePath: worktree.path)
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
                        editorState.showChanges = false
                    }
                } label: {
                    Image(systemName: "sidebar.left")
                }
                .buttonStyle(.plain)
                .help("Show file tree")
            }

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

            if let file = editorState.currentFile, !editorState.showChanges {
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(.tertiary)
                Image(systemName: file.language.icon)
                    .font(.system(size: 11))
                    .foregroundStyle(.secondary)
                Text(file.name)
                    .font(.system(size: 12))
                    .lineLimit(1)
                if file.isDirty {
                    Circle()
                        .fill(Color.primary.opacity(0.4))
                        .frame(width: 6, height: 6)
                }
            }

            Spacer()
            Button { showFileTree.toggle(); editorState.showChanges = !showFileTree } label: { Label("Files", systemImage: "doc.text") }.buttonStyle(.borderless)
            if project != nil { Button { showChannel.toggle() } label: { Label("Channel", systemImage: "bubble.left.and.bubble.right") }.buttonStyle(.borderless) }

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

    private var changesContent: some View { ReviewChangesView(state: diffState) }

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

    // MARK: - Binding Helpers

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
