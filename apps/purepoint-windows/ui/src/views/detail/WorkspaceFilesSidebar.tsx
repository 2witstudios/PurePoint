// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/WorkspaceFilesSidebar.swift (WIN-8.2).
//
// The workspace's right-hand sidebar: Changes (the working tree's changed files, each expanding
// in place to its diff, with a pinned header that collapses it), Files and Point Guard, plus the
// button that hides the sidebar. Polls the working tree while mounted (WorkspaceFilesState).
//
// Differences from Swift, until their leaves land: the Files mode's inline tree and previews
// need FileTreeState (WIN-8.4) and Point Guard's compact chat is PiPointGuardChatView (WIN-9.x);
// both show a placeholder.
import { useEffect, useRef } from 'react';
import { useStore } from 'zustand';
import type { FileDiff } from '../../services/git.ts';
import {
  WORKSPACE_FILES_MODE_TITLES,
  WORKSPACE_FILES_MODES,
  type WorkspaceFilesState,
} from '../../state/workspaceFilesState.ts';
import {
  useSidebarVisibilityStore,
  WORKSPACE_SIDEBAR_KEY,
} from '../../state/sidebarVisibilityStore.ts';
import { Icon } from '../../theme/Icon.tsx';
import { InlineDiffBody } from './InlineDiffBody.tsx';
import { ScreenPlaceholder } from './placeholders.tsx';
import './detail.css';

function lastPathComponent(path: string): string {
  return path.split('/').pop() ?? path;
}

/** Swift `WorkspaceSidebarToggle`: shows or hides the workspace sidebar. */
export function WorkspaceSidebarToggle() {
  const shown = useSidebarVisibilityStore((state) => state.workspaceSidebarShown);
  const label = shown ? 'Hide Workspace Sidebar' : 'Show Workspace Sidebar';
  return (
    <button
      type="button"
      className="detail-icon-button"
      aria-label={label}
      title={label}
      onClick={() => {
        useSidebarVisibilityStore.getState().toggle(WORKSPACE_SIDEBAR_KEY);
      }}
    >
      <Icon name="sidebar.right" size={14} />
    </button>
  );
}

/** Swift `stats(_:)`: +added −removed, or the status (`New` for untracked) when neither. */
export function DiffStats({ file }: { file: FileDiff }) {
  return (
    <span className="diff-stats">
      {file.added > 0 && <span className="diff-stat-added">{`+${String(file.added)}`}</span>}
      {file.removed > 0 && <span className="diff-stat-removed">{`−${String(file.removed)}`}</span>}
      {file.added === 0 && file.removed === 0 && (
        <span className="diff-stat-status">
          {file.statusCode === '??' ? 'New' : file.statusCode}
        </span>
      )}
    </span>
  );
}

function Message({ text }: { text: string }) {
  return <div className="workspace-files-message">{text}</div>;
}

function Changes({ state }: { state: WorkspaceFilesState }) {
  const files = useStore(state.store, (s) => s.files);
  const isLoading = useStore(state.store, (s) => s.isLoading);
  const error = useStore(state.store, (s) => s.error);
  const expandedFiles = useStore(state.store, (s) => s.expandedFiles);
  const diffs = useStore(state.store, (s) => s.diffs);
  const fileErrors = useStore(state.store, (s) => s.fileErrors);
  const headers = useRef(new Map<string, HTMLElement>());
  const previous = useRef(expandedFiles);

  // Swift scrolls a just-collapsed file's header back to the top of the list.
  useEffect(() => {
    const collapsed = [...previous.current].filter((name) => !expandedFiles.has(name)).sort()[0];
    previous.current = expandedFiles;
    if (collapsed !== undefined) headers.current.get(collapsed)?.scrollIntoView({ block: 'start' });
  }, [expandedFiles]);

  if (isLoading) return <Message text="Loading changes…" />;
  if (error !== null) return <Message text={error} />;
  if (files.length === 0) return <Message text="No changes" />;
  return files.map((file) => {
    const expanded = expandedFiles.has(file.filename);
    const loaded = diffs[file.filename];
    const fileError = fileErrors[file.filename];
    return (
      <section key={file.filename} className="workspace-file-section">
        <button
          type="button"
          className="workspace-file-header"
          title={file.filename}
          aria-label={`${file.filename}, ${expanded ? 'Collapse diff' : 'Expand diff'}`}
          aria-expanded={expanded}
          ref={(element) => {
            if (element === null) headers.current.delete(file.filename);
            else headers.current.set(file.filename, element);
          }}
          onClick={() => {
            state.toggleFile(file);
            if (!expanded) void state.loadDiff(file);
          }}
        >
          <Icon name={expanded ? 'chevron.down' : 'chevron.right'} size={8} />
          <span className="workspace-file-name">{lastPathComponent(file.filename)}</span>
          <DiffStats file={loaded ?? file} />
        </button>
        {expanded &&
          (fileError !== undefined ? (
            <Message text={fileError} />
          ) : loaded === undefined ? (
            <Message text="Loading…" />
          ) : loaded.hunks.length === 0 ? (
            <Message text="No text diff (empty, binary, or metadata-only change)." />
          ) : (
            <InlineDiffBody hunks={loaded.hunks} filename={file.filename} />
          ))}
      </section>
    );
  });
}

export function WorkspaceFilesSidebar({ state }: { state: WorkspaceFilesState }) {
  const mode = useStore(state.store, (s) => s.mode);
  const changeCount = useStore(state.store, (s) => s.files.length);

  useEffect(() => state.observe(), [state]);

  return (
    <div className="workspace-files-sidebar-body" data-view="WorkspaceFilesSidebar">
      <div className="workspace-files-modes" role="tablist" aria-label="Workspace sidebar">
        {WORKSPACE_FILES_MODES.map((candidate) => (
          <button
            key={candidate}
            type="button"
            role="tab"
            aria-selected={mode === candidate}
            onClick={() => {
              state.setMode(candidate);
            }}
          >
            {WORKSPACE_FILES_MODE_TITLES[candidate]}
            {candidate === 'changes' && changeCount > 0 && (
              <span className="workspace-files-count">{changeCount}</span>
            )}
          </button>
        ))}
        <span className="detail-spacer" />
        <WorkspaceSidebarToggle />
      </div>
      {mode === 'pointGuard' ? (
        <ScreenPlaceholder
          icon="bubble.left.and.bubble.right"
          title="Point Guard"
          view="PiPointGuardChatView"
        />
      ) : mode === 'files' ? (
        <ScreenPlaceholder icon="folder" title="Files" view="InlineFileTreeRow" />
      ) : (
        <div className="workspace-files-scroll">
          <Changes state={state} />
        </div>
      )}
    </div>
  );
}
