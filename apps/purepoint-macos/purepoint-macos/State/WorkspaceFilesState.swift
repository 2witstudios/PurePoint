import Foundation
import Observation

/// Review state belongs to one root. Changing workspaces creates a new instance,
/// so an in-flight read can never populate a different worktree's sidebar.
@Observable
@MainActor
final class WorkspaceFilesState {
    enum Mode: String, CaseIterable {
        case changes = "Changes"
        case files = "Files"
    }

    let rootPath: String
    var mode: Mode = .changes
    var files: [FileDiff] = []
    var expandedFiles: Set<String> = []
    var expandedFolders: Set<String> = []
    var expandedPreviews: Set<String> = []
    var diffs: [String: FileDiff] = [:]
    var loadingFiles: Set<String> = []
    var fileErrors: [String: String] = [:]
    var error: String?
    var isLoading = true
    let fileTree = FileTreeState()
    private var treeLoaded = false

    init(rootPath: String) {
        self.rootPath = rootPath
    }

    /// A .git watcher alone misses normal saves (and linked worktrees' .git is a
    /// static file). Poll while mounted, including expanded previews, instead.
    func observe() async {
        defer { fileTree.stopWatching() }
        while !Task.isCancelled {
            await refresh()
            do { try await Task.sleep(for: .seconds(2)) } catch { return }
        }
    }

    func refresh() async {
        do {
            let updated = try await GitService.shared.fetchWorkingTreeChanges(worktreePath: rootPath)
            guard !Task.isCancelled else { return }
            files = updated
            let paths = Set(updated.map(\.filename))
            expandedFiles.formIntersection(paths)
            diffs = diffs.filter { paths.contains($0.key) }
            fileErrors = fileErrors.filter { paths.contains($0.key) }
            error = nil
            isLoading = false
            for file in updated where expandedFiles.contains(file.filename) {
                await loadDiff(file)
                if Task.isCancelled { return }
            }
        } catch {
            guard !Task.isCancelled else { return }
            self.error = error.localizedDescription
            isLoading = false
        }
        if mode == .files {
            loadTreeIfNeeded()
            fileTree.refresh()
        }
    }

    func loadTreeIfNeeded() {
        guard !treeLoaded else { return }
        treeLoaded = true
        fileTree.load(worktreePath: rootPath)
    }

    func toggleFile(_ file: FileDiff) {
        if expandedFiles.contains(file.filename) {
            expandedFiles.remove(file.filename)
            diffs[file.filename] = nil
            fileErrors[file.filename] = nil
        } else {
            expandedFiles.insert(file.filename)
        }
    }

    func toggleFolder(_ node: FileTreeNode) {
        if expandedFolders.contains(node.relativePath) {
            expandedFolders.remove(node.relativePath)
            fileTree.collapseNode(node)
        } else {
            expandedFolders.insert(node.relativePath)
            fileTree.expandNode(node)
        }
    }

    func togglePreview(_ path: String) {
        if expandedPreviews.contains(path) { expandedPreviews.remove(path) } else { expandedPreviews.insert(path) }
    }

    func loadDiff(_ file: FileDiff) async {
        guard !loadingFiles.contains(file.filename) else { return }
        loadingFiles.insert(file.filename)
        defer { loadingFiles.remove(file.filename) }
        do {
            let diff = try await GitService.shared.fetchWorkingTreeFileDiff(worktreePath: rootPath, file: file)
            guard !Task.isCancelled, expandedFiles.contains(file.filename) else { return }
            diffs[file.filename] = diff
            fileErrors[file.filename] = nil
        } catch {
            guard !Task.isCancelled else { return }
            fileErrors[file.filename] = error.localizedDescription
        }
    }
}
