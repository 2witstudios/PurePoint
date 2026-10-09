import Foundation

struct SpawnTarget {
    let root: Bool
    let worktree: String?
}

enum SpawnTargetResolver {
    static func resolve(
        isWorktree: Bool,
        selection: SidebarSelection?,
        worktreeIdForWorkspace: (String) -> String?
    ) -> SpawnTarget {
        if isWorktree {
            return SpawnTarget(root: false, worktree: nil)
        }

        switch selection {
        case .worktree(let id):
            return SpawnTarget(root: false, worktree: id)
        case .workspace(let id):
            // A new agent joins the worktree its originating workspace lives in.
            if let wtId = worktreeIdForWorkspace(id) {
                return SpawnTarget(root: false, worktree: wtId)
            }
            return SpawnTarget(root: true, worktree: nil)
        case nil, .nav, .project, .channel:
            return SpawnTarget(root: true, worktree: nil)
        }
    }
}
