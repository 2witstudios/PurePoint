import SwiftUI

/// Attached to the workspace's content, rather than the elevated global navigation.
/// A file's body expands in place in the same scroll view as its filename.
struct WorkspaceFilesSidebar: View {
    @Bindable var state: WorkspaceFilesState

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 14) {
                ForEach(WorkspaceFilesState.Mode.allCases, id: \.self) { mode in
                    Button {
                        state.mode = mode
                        if mode == .files { state.loadTreeIfNeeded() }
                    } label: {
                        HStack(spacing: 4) {
                            Text(mode.rawValue)
                            if mode == .changes, !state.files.isEmpty {
                                Text("\(state.files.count)").foregroundStyle(.tertiary)
                            }
                        }
                        .font(.system(size: 11))
                        .foregroundStyle(state.mode == mode ? .primary : .secondary)
                    }
                    .buttonStyle(.plain)
                    .accessibilityAddTraits(state.mode == mode ? [.isSelected] : [])
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10)
            .frame(height: PaneTabBar.height)
            Divider()
            ScrollViewReader { proxy in
                ScrollView {
                    LazyVStack(spacing: 0, pinnedViews: [.sectionHeaders]) {
                        if state.mode == .changes {
                            changes
                        } else {
                            ForEach(state.fileTree.rootNodes, id: \.relativePath) { node in
                                InlineFileTreeRow(node: node, state: state, depth: 0)
                            }
                            if state.fileTree.rootNodes.isEmpty { message("No files") }
                        }
                    }
                }
                .onChange(of: state.expandedFiles) { previous, current in
                    if let collapsed = previous.subtracting(current).sorted().first {
                        proxy.scrollTo(collapsed, anchor: .top)
                    }
                }
            }
        }
        .background(Color(nsColor: Theme.cardBackground))
        .task { await state.observe() }
    }

    @ViewBuilder
    private var changes: some View {
        if state.isLoading {
            ProgressView().controlSize(.small).padding(16)
        } else if let error = state.error {
            message(error)
        } else if state.files.isEmpty {
            message("No changes")
        } else {
            ForEach(state.files) { file in
                let expanded = state.expandedFiles.contains(file.filename)
                // Keep the header separate from the potentially very tall native
                // diff view. Pin it while scrolling its body so collapse stays reachable.
                Section {
                    if expanded {
                        diffBody(file)
                            .task { await state.loadDiff(file) }
                    }
                    Divider()
                } header: {
                    Button {
                        state.toggleFile(file)
                    } label: {
                        HStack(spacing: 7) {
                            chevron(expanded)
                            Text((file.filename as NSString).lastPathComponent)
                                .font(.system(size: 11))
                                .lineLimit(1)
                                .truncationMode(.middle)
                            Spacer(minLength: 4)
                            stats(state.diffs[file.filename] ?? file)
                        }
                        .padding(.horizontal, 10)
                        .frame(height: 30)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .help(file.filename)
                    .accessibilityLabel("\(file.filename), \(expanded ? "Collapse diff" : "Expand diff")")
                    .background(Color(nsColor: Theme.cardBackground))
                    .id(file.filename)
                }
            }
        }
    }

    @ViewBuilder
    private func diffBody(_ file: FileDiff) -> some View {
        if let error = state.fileErrors[file.filename] {
            message(error)
        } else if let diff = state.diffs[file.filename] {
            if diff.hunks.isEmpty {
                message("No text diff (empty, binary, or metadata-only change).")
            } else {
                InlineCodeView(hunks: diff.hunks, language: EditorLanguage.detect(from: file.filename))
            }
        } else {
            ProgressView().controlSize(.mini).padding(10)
        }
    }

    private func stats(_ file: FileDiff) -> some View {
        HStack(spacing: 5) {
            if file.added > 0 {
                Text("+\(file.added)").foregroundStyle(Color(nsColor: Theme.additionText))
            }
            if file.removed > 0 {
                Text("−\(file.removed)").foregroundStyle(Color(nsColor: Theme.deletionText))
            }
            if file.added == 0 && file.removed == 0 {
                Text(file.statusCode == "??" ? "New" : file.statusCode)
                    .foregroundStyle(.secondary)
            }
        }
        .font(.system(size: 10, design: .monospaced))
    }

    private func message(_ text: String) -> some View {
        Text(text).font(.system(size: 11)).foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, alignment: .leading).padding(12)
    }
}

private func chevron(_ expanded: Bool) -> some View {
    Image(systemName: expanded ? "chevron.down" : "chevron.right")
        .font(.system(size: 8)).foregroundStyle(.tertiary).frame(width: 8)
}

private struct InlineFileTreeRow: View {
    let node: FileTreeNode
    let state: WorkspaceFilesState
    let depth: Int

    private var expanded: Bool {
        node.isDirectory
            ? state.expandedFolders.contains(node.relativePath) : state.expandedPreviews.contains(node.relativePath)
    }

    var body: some View {
        VStack(spacing: 0) {
            Button {
                if node.isDirectory { state.toggleFolder(node) } else { state.togglePreview(node.relativePath) }
            } label: {
                HStack(spacing: 7) {
                    chevron(expanded)
                    if node.isDirectory {
                        Image(systemName: "folder").font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                    Text(node.name).font(.system(size: 11)).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 0)
                }
                .padding(.leading, CGFloat(10 + depth * 12)).padding(.trailing, 10)
                .frame(height: 26).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .help(node.relativePath)
            .accessibilityLabel("\(node.relativePath), \(expanded ? "Collapse" : "Expand")")
            if expanded {
                if node.isDirectory {
                    ForEach(node.children, id: \.relativePath) { child in
                        AnyView(InlineFileTreeRow(node: child, state: state, depth: depth + 1))
                    }
                } else {
                    InlineFilePreview(path: node.absolutePath, name: node.name)
                }
            }
        }
    }
}

private struct InlineFilePreview: View {
    let path: String
    let name: String
    @State private var hunks: [Hunk]?
    @State private var message: String?

    var body: some View {
        Group {
            if let message {
                Text(message).font(.system(size: 11)).foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading).padding(12)
            } else if let hunks {
                InlineCodeView(hunks: hunks, language: EditorLanguage.detect(from: name))
            } else {
                ProgressView().controlSize(.mini).padding(10)
            }
        }
        .task(id: path) {
            var lastModified: Date?
            while !Task.isCancelled {
                let modified = FileIOService.fileModificationDate(at: path)
                if hunks == nil || modified != lastModified {
                    do {
                        let file = try await FileIOService.readFile(at: path, limit: 1_000_000)
                        guard !Task.isCancelled else { return }
                        message = file.isBinary ? "Binary file" : file.content.isEmpty ? "Empty file" : nil
                        var lines = file.content.components(separatedBy: "\n")
                        if lines.last == "" { lines.removeLast() }
                        hunks = [
                            Hunk(
                                header: "",
                                lines: lines.enumerated().map {
                                    DiffLine(
                                        type: .context, content: $0.element, oldLineNo: nil, newLineNo: $0.offset + 1)
                                })
                        ]
                        lastModified = modified
                    } catch {
                        guard !Task.isCancelled else { return }
                        message = error.localizedDescription
                    }
                }
                do { try await Task.sleep(for: .seconds(2)) } catch { return }
            }
        }
    }
}
