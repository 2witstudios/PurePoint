import SwiftUI

/// A tab showing a file navigator and editor (code, or rendered markdown) rooted at the
/// workspace's worktree or project. Its tree and editor state live in a `FileTabSession`
/// owned by `FileTabStore`, so they survive the view being unmounted when the tab is hidden.
struct FilePaneView: View {
    let rootPath: String
    let initialPath: String?
    @Bindable var session: FileTabSession
    let onFocus: () -> Void
    /// The file shown changed — the tab records it, so its title and the restored file follow.
    let onPathChange: (String) -> Void

    @State private var saveError: String?

    private var fileTree: FileTreeState { session.fileTree }
    private var editor: EditorState { session.editor }
    private var isMarkdown: Bool { editor.currentFile?.language == .markdown }

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            HStack(spacing: 0) {
                if session.showTree {
                    FileTreeSidebarView(
                        fileTreeState: fileTree, showFileTree: $session.showTree,
                        onFileSelected: { path, name in editor.openFile(path: path, name: name) }
                    )
                    .frame(width: session.treeWidth)
                    Divider()
                }
                content
            }
        }
        .background(Color(nsColor: Theme.cardBackground))
        .simultaneousGesture(TapGesture().onEnded { onFocus() })
        .onAppear {
            // Returning to a tab finds its session as it was left: only a first appearance
            // (or a new root) loads the tree and opens the tab's file.
            if session.loadedRoot != rootPath {
                fileTree.load(worktreePath: rootPath)
                session.loadedRoot = rootPath
            }
            if editor.currentFile == nil { openInitial() }
        }
        .onChange(of: initialPath) { _, _ in openInitial() }
        .onChange(of: editor.currentFile?.id) { _, path in
            if let path, path != initialPath { onPathChange(path) }
        }
        // Watchers keep running while the tab is hidden; the store stops them when it closes.
    }

    private func openInitial() {
        guard let initialPath, editor.currentFile?.id != initialPath else { return }
        editor.openFile(path: initialPath, name: (initialPath as NSString).lastPathComponent)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Button {
                withAnimation(.easeInOut(duration: 0.2)) { session.showTree.toggle() }
            } label: {
                Image(systemName: "sidebar.left")
            }
            .buttonStyle(.plain)
            .help(session.showTree ? "Hide file tree" : "Show file tree")

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
                Picker("", selection: $session.showPreview) {
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
                } else if isMarkdown && session.showPreview {
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
