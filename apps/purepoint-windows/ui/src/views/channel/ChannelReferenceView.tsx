// Port of apps/purepoint-macos/purepoint-macos/Views/Channel/ChannelReferenceView.swift (WIN-8.10).
//
// A sheet with a referenced commit's or pull request's changes, read in the sender's worktree
// (the project root when that worktree is gone).
import { useEffect, useId, useState } from 'react';
import type { ChannelMessage, ChannelReference } from '../../model/channelModel.ts';
import {
  fetchCommitDiff,
  fetchPRDiff,
  gitErrorMessage,
  type DiffData,
} from '../../services/git.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { DiffListView } from '../diff/DiffListView.tsx';

export interface ChannelReferenceSelection {
  readonly message: ChannelMessage;
  readonly reference: ChannelReference;
}

export interface ReferenceDiffFetchers {
  commit: (root: string, sha: string, machineId: string | null) => Promise<DiffData>;
  pr: (cwd: string, prNumber: number, machineId: string | null) => Promise<DiffData>;
}

const defaultFetchers: ReferenceDiffFetchers = {
  commit: async (root, sha, machineId) => ({
    files: await fetchCommitDiff(root, sha, false, machineId),
  }),
  pr: (cwd, prNumber, machineId) => fetchPRDiff(cwd, prNumber, false, machineId),
};

export function ChannelReferenceView({
  project,
  selection,
  onDismiss,
  fetchers = defaultFetchers,
}: {
  project: ProjectState;
  selection: ChannelReferenceSelection;
  onDismiss: () => void;
  fetchers?: ReferenceDiffFetchers;
}) {
  const titleId = useId();
  const [diff, setDiff] = useState<DiffData | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    // A function, so the checks after each await are not narrowed away.
    const live = () => !cancelled;
    const { message, reference } = selection;
    const path =
      project.worktrees.find((worktree) => worktree.id === message.author.worktree_id)?.path ??
      project.projectRoot;
    void (async () => {
      try {
        if (reference.kind === 'commit') {
          const value = await fetchers.commit(path, reference.value, project.machineId);
          if (live()) setDiff(value);
        } else if (/^-?\d+$/.test(reference.value)) {
          const value = await fetchers.pr(path, Number(reference.value), project.machineId);
          if (live()) setDiff(value);
        } else if (live()) {
          setError('Invalid pull request reference');
        }
      } catch (failure) {
        if (live()) setError(gitErrorMessage(failure));
      }
      if (live()) setLoading(false);
    })();
    return () => {
      cancelled = true;
    };
  }, [project, selection, fetchers]);

  return (
    <div className="channel-sheet-backdrop">
      <div
        className="channel-sheet channel-reference-sheet"
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            onDismiss();
          }
        }}
      >
        <div className="channel-reference-header">
          <h2 className="channel-sheet-title" id={titleId}>
            {selection.reference.label ?? selection.reference.value}
          </h2>
          <span className="channel-spacer" />
          <button type="button" className="channel-button" autoFocus onClick={onDismiss}>
            Done
          </button>
        </div>
        <div className="channel-reference-body">
          <DiffListView
            diff={diff}
            isLoading={loading}
            emptyMessage="No file changes"
            error={error}
          />
        </div>
      </div>
    </div>
  );
}
