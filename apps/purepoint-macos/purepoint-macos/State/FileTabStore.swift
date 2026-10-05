import Foundation
import Observation

/// Everything a file tab shows besides the file path: navigator, editor (including unsaved
/// edits), and view toggles. It lives here rather than in the view's `@State` because only a
/// pane's active tab is rendered — switching tabs, or moving a tab to another pane, unmounts
/// the view, and must not throw away an edit in progress.
@Observable
@MainActor
final class FileTabSession {
    let fileTree = FileTreeState()
    let editor = EditorState()
    var showTree: Bool
    var showPreview = true
    var treeWidth: CGFloat = 200
    /// The root the navigator was loaded from, so returning to the tab does not rescan.
    var loadedRoot: String?

    init(showTree: Bool) {
        self.showTree = showTree
    }

    var hasUnsavedChanges: Bool { editor.currentFile?.isDirty ?? false }

    func stopWatching() {
        fileTree.stopWatching()
        editor.stopWatching()
    }
}

/// File tab sessions by tab. Tabs only move within a workspace and surface IDs are unique
/// there, so (workspace, surface) names a tab for its whole life. `WorkspaceRegistry` prunes
/// the store whenever its layout changes, which is how a closed tab's session (and its file
/// watchers) goes away.
@MainActor
final class FileTabStore {
    struct Key: Hashable {
        let workspaceId: String
        let surfaceId: Int
    }

    private var sessions: [Key: FileTabSession] = [:]

    func session(workspaceId: String, surfaceId: Int, initialPath: String?) -> FileTabSession {
        let key = Key(workspaceId: workspaceId, surfaceId: surfaceId)
        if let existing = sessions[key] { return existing }
        let session = FileTabSession(showTree: initialPath == nil)
        sessions[key] = session
        return session
    }

    func existingSession(workspaceId: String, surfaceId: Int) -> FileTabSession? {
        sessions[Key(workspaceId: workspaceId, surfaceId: surfaceId)]
    }

    /// Drop every session whose tab no longer exists.
    func retain(only live: Set<Key>) {
        for key in sessions.keys where !live.contains(key) {
            sessions.removeValue(forKey: key)?.stopWatching()
        }
    }
}
