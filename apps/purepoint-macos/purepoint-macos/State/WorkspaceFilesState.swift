#if os(macOS)
import AppKit
#else
import UIKit
#endif
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
        case pointGuard = "Point Guard"
    }

    let rootPath: String
    /// Where the root lives; a remote root's git and files go through its machine's daemon.
    let host: any ProjectHost
    private let git: GitService
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

    /// Which files this state shows: the host (this Mac or a machine) and the root on it.
    var identity: String { Self.identity(rootPath: rootPath, host: host) }

    nonisolated static func identity(rootPath: String, host: any ProjectHost) -> String {
        host.identity + "\u{0}" + rootPath
    }

    init(rootPath: String, host: any ProjectHost = LocalProjectHost.shared) {
        self.rootPath = rootPath
        self.host = host
        git = host.isLocal ? GitService.shared : GitService(host: host)
    }

    /// A .git watcher alone misses normal saves (and linked worktrees' .git is a
    /// static file). Poll while mounted, including expanded previews, instead.
    func observe() async {
        defer { fileTree.stopWatching() }
        while !Task.isCancelled {
            // Remote polling is a daemon round trip over SSH: skip it while the app is in the
            // background (like DiffState), and poll less often.
            if host.isLocal || NSApplication.shared.isActive { await refresh() }
            do { try await Task.sleep(for: .seconds(host.isLocal ? 2 : 5)) } catch { return }
        }
    }

    func refresh() async {
        do {
            let updated = try await git.fetchWorkingTreeChanges(worktreePath: rootPath)
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
        fileTree.load(worktreePath: rootPath, host: host)
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
            let diff = try await git.fetchWorkingTreeFileDiff(worktreePath: rootPath, file: file)
            guard !Task.isCancelled, expandedFiles.contains(file.filename) else { return }
            diffs[file.filename] = diff
            fileErrors[file.filename] = nil
        } catch {
            guard !Task.isCancelled else { return }
            fileErrors[file.filename] = error.localizedDescription
        }
    }
}
