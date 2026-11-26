// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/WorktreeDetailView.swift (WIN-8.2).
//
// A worktree: its header (name, branch, file tree / Files / Channel / changes / Refresh), the
// file tree on the left, the review changes or the open file in the middle, and the project
// channel on the right. DiffState loads for the worktree and is stopped when the screen goes.
//
// The file tree is FileTreeSidebarView (WIN-8.4), listed while shown; the editor and its banners
// (current file breadcrumb, external change, save error) are EditorState and FileEditorContent
// (WIN-8.5).
import { useMemo, useState } from 'react';
import type { WorktreeModel } from '../../model/workspaceModel.ts';
import type { DiffState } from '../../state/diffState.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { ChangesContent } from './ChangesContent.tsx';
import { ProjectChannelView } from '../channel/ProjectChannelView.tsx';
import { projectFileHost } from '../../services/fileHost.ts';
import { FileTreeSidebarView } from '../filetree/FileTreeSidebarView.tsx';
import { useFileTreeState } from '../filetree/useFileTreeState.ts';
import { useStore } from 'zustand';
import { detectEditorLanguage } from '../../model/editorLanguage.ts';
import { editorLanguageIcon } from '../../model/editorTab.ts';
import { FileEditorContent, useEditorState } from '../filePane/FileEditorContent.tsx';
import { useDiffState } from './useDiffState.ts';
import './detail.css';

export interface WorktreeDetailViewProps {
  worktree: WorktreeModel;
  project: ProjectState | null;
  /** Tests pass a DiffState over a scripted git service. */
  createDiffState?: () => DiffState;
}

export function WorktreeDetailView({
  worktree,
  project,
  createDiffState,
}: WorktreeDetailViewProps) {
  const diffState = useDiffState(
    {
      path: worktree.path,
      branch: worktree.branch,
      baseBranch: worktree.baseBranch,
      machineId: project?.machineId ?? null,
    },
    createDiffState,
  );
  const [showChannel, setShowChannel] = useState(false);
  const [showFileTree, setShowFileTree] = useState(false);
  const host = useMemo(() => projectFileHost(project), [project]);
  const fileTree = useFileTreeState(worktree.path, host, showFileTree);
  const editor = useEditorState(worktree.path, host);
  // Swift `EditorState.showChanges` (starts true; opening a file clears it).
  const showChanges = useStore(editor.store, (s) => s.showChanges);
  const file = useStore(editor.store, (s) => s.currentFile);
  const setShowChanges = (shown: boolean | ((shown: boolean) => boolean)) => {
    editor.setShowChanges(typeof shown === 'function' ? shown(showChanges) : shown);
  };

  return (
    <div className="detail-screen" data-view="WorktreeDetailView">
      <div className="detail-header">
        {!showFileTree && (
          <button
            type="button"
            className="detail-icon-button"
            aria-label="Show file tree"
            title="Show file tree"
            onClick={() => {
              setShowFileTree(true);
              setShowChanges(false);
            }}
          >
            <Icon name="sidebar.left" size={14} />
          </button>
        )}
        <Icon name="arrow.triangle.branch" size={14} className="detail-header-icon" />
        <span className="detail-title">{worktree.name}</span>
        <span className="detail-branch-chip">{worktree.branch}</span>
        {file !== null && !showChanges && (
          <span className="detail-breadcrumb" data-testid="detail-breadcrumb">
            <Icon name="chevron.right" size={9} />
            <Icon name={editorLanguageIcon(detectEditorLanguage(file.name))} size={11} />
            <span>{file.name}</span>
            {file.isDirty && <span className="detail-dirty-dot" aria-label="Unsaved changes" />}
          </span>
        )}
        <span className="detail-spacer" />
        <button
          type="button"
          className="detail-button"
          onClick={() => {
            const next = !showFileTree;
            setShowFileTree(next);
            setShowChanges(!next);
          }}
        >
          <Icon name="doc.text" size={13} />
          Files
        </button>
        {project !== null && (
          <button
            type="button"
            className="detail-button"
            aria-pressed={showChannel}
            onClick={() => {
              setShowChannel((shown) => !shown);
            }}
          >
            <Icon name="bubble.left.and.bubble.right" size={13} />
            Channel
          </button>
        )}
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
            void fileTree.refresh();
          }}
        >
          <Icon name="arrow.clockwise" size={13} />
        </button>
      </div>
      <div className="detail-body">
        {showFileTree && (
          <div className="detail-file-tree">
            <FileTreeSidebarView
              fileTreeState={fileTree}
              onHide={() => {
                setShowFileTree(false);
              }}
              onFileSelected={(path, name) => {
                void editor.openFile(path, name);
              }}
            />
          </div>
        )}
        <div className="detail-content">
          {showChanges ? (
            <ChangesContent state={diffState} />
          ) : (
            <FileEditorContent editor={editor} emptyText="Select a file to edit" />
          )}
        </div>
        {showChannel && project !== null && (
          <div className="detail-channel">
            <ProjectChannelView key={project.projectId} project={project} compact />
          </div>
        )}
      </div>
    </div>
  );
}
