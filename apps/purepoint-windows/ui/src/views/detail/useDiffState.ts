// One DiffState per detail screen (Swift `@State private var diffState = DiffState()`), loaded
// for the screen's worktree or root and stopped when the screen goes away (`.onDisappear`).
import { useEffect, useState } from 'react';
import { DiffState, gitServiceFor } from '../../state/diffState.ts';

export interface DiffTarget {
  /** The worktree path, or the project root. */
  readonly path: string;
  /** The worktree's branch; null for the root checkout. */
  readonly branch: string | null;
  readonly baseBranch: string | null;
  readonly machineId: string | null;
}

export function useDiffState(
  target: DiffTarget,
  create: () => DiffState = () => new DiffState(),
): DiffState {
  const [diffState] = useState(create);
  const { path, branch, baseBranch, machineId } = target;
  useEffect(() => {
    diffState.use(gitServiceFor(machineId));
    if (branch === null) diffState.loadForProject(path);
    else diffState.loadForWorktree({ path, branch, baseBranch });
    return () => {
      diffState.stopWatching();
    };
  }, [diffState, path, branch, baseBranch, machineId]);
  return diffState;
}
