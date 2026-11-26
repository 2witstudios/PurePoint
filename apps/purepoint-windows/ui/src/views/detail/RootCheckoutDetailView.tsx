// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/RootCheckoutDetailView.swift (WIN-8.2).
//
// The project's root checkout, opened from the project overview: its header (file tree toggle,
// name, worktree count, changes toggle, Refresh), the file tree (shown by default) and the
// review changes or the open file. DiffState loads for the project root.
//
// Differences from Swift, as in WorktreeDetailView: the file tree is WIN-8.4 and the editor and
// its banners WIN-8.5; until then the editor shows its empty state.
import { useState } from 'react';
import { useStore } from 'zustand';
import type { DiffState } from '../../state/diffState.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { ChangesContent } from './ChangesContent.tsx';
import { EditorPlaceholder, FileTreeSidebarPlaceholder } from './placeholders.tsx';
import { useDiffState } from './useDiffState.ts';
import './detail.css';

export interface RootCheckoutDetailViewProps {
  project: ProjectState;
  createDiffState?: () => DiffState;
}

export function RootCheckoutDetailView({ project, createDiffState }: RootCheckoutDetailViewProps) {
  const worktreeCount = useStore(project.store, (state) => state.worktrees.length);
  const diffState = useDiffState(
    { path: project.projectRoot, branch: null, baseBranch: null, machineId: project.machineId },
    createDiffState,
  );
  const [showFileTree, setShowFileTree] = useState(true);
  const [showChanges, setShowChanges] = useState(true);

  return (
    <div className="detail-screen" data-view="RootCheckoutDetailView">
      <div className="detail-header">
        {!showFileTree && (
          <button
            type="button"
            className="detail-icon-button"
            aria-label="Show file tree"
            title="Show file tree"
            onClick={() => {
              setShowFileTree(true);
            }}
          >
            <Icon name="sidebar.left" size={14} />
          </button>
        )}
        <Icon name="folder.fill" size={14} className="detail-header-icon" />
        <span className="detail-title">{project.projectName}</span>
        <span className="detail-subtitle">{`${String(worktreeCount)} worktrees`}</span>
        <span className="detail-spacer" />
        <button
          type="button"
          className="detail-icon-button"
          aria-label="Toggle changes view"
          title="Toggle changes view"
          aria-pressed={showChanges}
          onClick={() => {
            setShowChanges((shown) => !shown);
          }}
        >
          <Icon name="doc.badge.plus" size={13} />
        </button>
        <button
          type="button"
          className="detail-icon-button"
          aria-label="Refresh"
          title="Refresh"
          onClick={() => {
            diffState.refresh();
          }}
        >
          <Icon name="arrow.clockwise" size={13} />
        </button>
      </div>
      <div className="detail-body">
        {showFileTree && (
          <FileTreeSidebarPlaceholder
            onHide={() => {
              setShowFileTree(false);
            }}
          />
        )}
        <div className="detail-content">
          {showChanges ? <ChangesContent state={diffState} /> : <EditorPlaceholder />}
        </div>
      </div>
    </div>
  );
}
