// Port of apps/purepoint-macos/purepoint-macos/State/SpawnTargetResolver.swift (WIN-6.6).
//
// `SidebarSelection` itself ports with Models/SidebarItem.swift (WIN-6.5); this takes the
// structurally equivalent selection so it does not depend on that leaf's file.

/** Where a new agent spawns: the project root, or a worktree (`worktree: null` = a new one). */
export interface SpawnTarget {
  readonly root: boolean;
  readonly worktree: string | null;
}

export type SpawnSelection =
  | { readonly kind: 'nav'; readonly item: string }
  | { readonly kind: 'workspace'; readonly id: string }
  | { readonly kind: 'worktree'; readonly id: string }
  | { readonly kind: 'channel'; readonly id: string }
  | { readonly kind: 'project'; readonly id: string };

export interface SpawnTargetInput {
  /** The palette item creates a new worktree. */
  readonly isWorktree: boolean;
  readonly selection: SpawnSelection | null;
  /** The worktree a workspace lives in, or null for a workspace at the project root. */
  readonly worktreeIdForWorkspace: (workspaceId: string) => string | null | undefined;
}

export function resolveSpawnTarget(input: SpawnTargetInput): SpawnTarget {
  if (input.isWorktree) return { root: false, worktree: null };
  const selection = input.selection;
  switch (selection?.kind) {
    case 'worktree':
      return { root: false, worktree: selection.id };
    case 'workspace': {
      // A new agent joins the worktree its originating workspace lives in.
      const worktree = input.worktreeIdForWorkspace(selection.id);
      if (worktree != null) return { root: false, worktree };
      return { root: true, worktree: null };
    }
    default:
      return { root: true, worktree: null };
  }
}
