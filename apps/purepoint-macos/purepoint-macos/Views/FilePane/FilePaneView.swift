import SwiftUI

/// A pane showing a file navigator and editor (code, or rendered markdown) rooted at the
/// workspace's worktree or project. Each pane owns its own tree and editor state.
struct FilePaneView: View {
    let workspaceId: String
    let leafId: Int
    let rootPath: String
    let initialPath: String?
    let onFocus: () -> Void

    @State private var fileTree = FileTreeState()
    @State private var editor = EditorState()
    @State private var showTree: Bool
    @State private var showPreview = true
    @State private var saveError: String?
    @State private var treeWidth: CGFloat = 200

    init(
        workspaceId: String, leafId: Int, rootPath: String, initialPath: String?,
        onFocus: @escaping () -> Void
    ) {
        self.workspaceId = workspaceId
        self.leafId = leafId
        self.rootPath = rootPath
        self.initialPath = initialPath
        self.onFocus = onFocus
        _showTree = State(initialValue: initialPath == nil)
    }

    private var isMarkdown: Bool { editor.currentFile?.language == .markdown }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            HStack(spacing: 0) {
                if showTree {
                    FileTreeSidebarView(
                        fileTreeState: fileTree, showFileTree: $showTree,
                        onFileSelected: { path, name in editor.openFile(path: path, name: name) }
                    )
                    .frame(width: treeWidth)
                    Divider()
                }
                content
            }
        }
        .background(Color(nsColor: Theme.cardBackground))
        .simultaneousGesture(TapGesture().onEnded { onFocus() })
        .onAppear {
            fileTree.load(worktreePath: rootPath)
            openInitial()
        }
        .onChange(of: initialPath) { _, _ in openInitial() }
        .onDisappear {
            fileTree.stopWatching()
            editor.stopWatching()
        }
    }

    private func openInitial() {
        guard let initialPath else { return }
        editor.openFile(path: initialPath, name: (initialPath as NSString).lastPathComponent)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) { showTree.toggle() }
            } label: {
                Image(systemName: "sidebar.left")
            }
            .buttonStyle(.plain)
            .help(showTree ? "Hide file tree" : "Show file tree")

            if let file = editor.currentFile {
                Image(systemName: file.language.icon).foregroundStyle(.secondary)
                Text(file.name).font(.system(size: 12, weight: .medium)).lineLimit(1)
                if file.isDirty {
                    Circle().fill(Color.orange).frame(width: 6, height: 6).help("Unsaved changes")
                }
            } else {
                Text((rootPath as NSString).lastPathComponent)
                    .font(.system(size: 12)).foregroundStyle(.secondary).lineLimit(1)
            }

            Spacer()

            if isMarkdown {
                Picker("", selection: $showPreview) {
                    Text("Code").tag(false)
                    Text("Preview").tag(true)
                }
                .pickerStyle(.segmented)
                .labelsHidden()
                .frame(width: 130)
            }
        }
        .padding(.horizontal, 10)
        .frame(height: 30)
    }

    @ViewBuilder
    private var content: some View {
        VStack(spacing: 0) {
            if let saveError {
                banner(saveError, color: .red) { self.saveError = nil }
            }
            if editor.externalChangeAlert != nil {
                HStack {
                    Text("File changed on disk.").font(.caption)
                    Spacer()
                    Button("Reload") { editor.reloadFile() }
                    Button("Keep mine") { editor.dismissExternalChange() }
                }
                .padding(.horizontal, 10).padding(.vertical, 4)
                .background(Color.yellow.opacity(0.2))
            }
            if let file = editor.currentFile {
                if file.isBinary {
                    placeholder("Binary file — can't be shown", icon: "doc.fill")
                } else if isMarkdown && showPreview {
                    MarkdownPreviewView(
                        markdown: file.content,
                        baseDirectory: (file.id as NSString).deletingLastPathComponent,
                        onOpenFile: { path in editor.openFile(path: path, name: (path as NSString).lastPathComponent) }
                    )
                } else {
                    EditorContentRepresentable(
                        content: file.content,
                        language: file.language,
                        isBinary: false,
                        isEditable: true,
                        onContentChanged: { editor.updateContent(content: $0) },
                        onSave: save
                    )
                }
            } else {
                placeholder("Select a file", icon: "doc.text")
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func save() {
        Task {
            do {
                try await editor.saveFile()
                saveError = nil
            } catch {
                saveError = "Save failed: \(error.localizedDescription)"
            }
        }
    }

    private func placeholder(_ text: String, icon: String) -> some View {
        VStack(spacing: 8) {
            Image(systemName: icon).font(.system(size: 28)).foregroundStyle(.quaternary)
            Text(text).font(.callout).foregroundStyle(.tertiary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    private func banner(_ text: String, color: Color, dismiss: @escaping () -> Void) -> some View {
        HStack {
            Text(text).font(.caption)
            Spacer()
            Button("Dismiss", action: dismiss)
        }
        .padding(.horizontal, 10).padding(.vertical, 4)
        .background(color.opacity(0.2))
    }
}
