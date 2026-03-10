import Foundation

struct SpawnTarget {
    let root: Bool
    let worktree: String?
}

enum SpawnTargetResolver {
    static func resolve(
        isWorktree: Bool,
        selection: SidebarSelection?,
        worktreeIdForAgent: (String) -> String?
    ) -> SpawnTarget {
        if isWorktree {
            return SpawnTarget(root: false, worktree: nil)
        }

        switch selection {
        case .worktree(let id):
            return SpawnTarget(root: false, worktree: id)
        case .agent(let id):
            if let wtId = worktreeIdForAgent(id) {
                return SpawnTarget(root: false, worktree: wtId)
            }
            return SpawnTarget(root: true, worktree: nil)
        case .terminal(let id):
            if let wtId = worktreeIdForAgent(id) {
                return SpawnTarget(root: false, worktree: wtId)
            }
            return SpawnTarget(root: true, worktree: nil)
        case nil, .nav, .project:
            return SpawnTarget(root: true, worktree: nil)
        }
    }
}
