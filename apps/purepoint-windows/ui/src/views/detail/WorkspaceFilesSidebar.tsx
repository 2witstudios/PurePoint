// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/WorkspaceFilesSidebar.swift (WIN-8.2).
//
// The workspace's right-hand sidebar: Changes (the working tree's changed files, each expanding
// in place to its diff, with a pinned header that collapses it), Files and Point Guard, plus the
// button that hides the sidebar. Polls the working tree while mounted (WorkspaceFilesState).
//
// Files (WIN-8.4) is the root's tree inline: a folder expands in place, a file expands to a
// read-only preview of its text (`InlineFilePreview`), polled while it is open.
//
// Difference from Swift, until its leaf lands: Point Guard's compact chat is PiPointGuardChatView
// (WIN-9.x) and shows a placeholder.
import { useEffect, useRef, useState } from 'react';
import { useStore } from 'zustand';
import type { FileNode } from '../../model/fileNode.ts';
import { fileHostErrorMessage, type FileHost } from '../../services/fileHost.ts';
import type { FileDiff, Hunk } from '../../services/git.ts';
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
import { InlineCodeView } from '../diff/InlineCodeView.tsx';
import { CompactPointGuardChat } from '../chat/PiPointGuardChatView.tsx';
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
            <InlineCodeView hunks={loaded.hunks} filename={file.filename} />
          ))}
      </section>
    );
  });
}

/** Swift `InlineFilePreview`: the file's text as one context hunk, re-read when it changes. */
function InlineFilePreview({ path, name, host }: { path: string; name: string; host: FileHost }) {
  const [hunks, setHunks] = useState<Hunk[] | null>(null);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    const run = { cancelled: false };
    const isCancelled = () => run.cancelled;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let loaded = false;
    let lastModified: number | null = null;
    const tick = async () => {
      // A remote check is a daemon round trip: skip it while the app is in the background, as
      // the other remote pollers do (the first read always runs).
      const checkNow = !loaded || host.isLocal || document.hasFocus();
      try {
        const modified = checkNow ? await host.stat(path) : lastModified;
        if (isCancelled()) return;
        if (checkNow && (!loaded || modified !== lastModified)) {
          const file = await host.read(path, 1_000_000);
          if (isCancelled()) return;
          setMessage(file.isBinary ? 'Binary file' : file.content === '' ? 'Empty file' : null);
          const lines = file.content.split('\n');
          if (lines.at(-1) === '') lines.pop();
          setHunks([
            {
              header: '',
              lines: lines.map((content, index) => ({
                type: 'context',
                content,
                oldLineNo: null,
                newLineNo: index + 1,
              })),
            },
          ]);
          loaded = true;
          lastModified = modified;
        }
      } catch (error) {
        if (isCancelled()) return;
        setMessage(fileHostErrorMessage(error));
        loaded = true;
      }
      if (!isCancelled()) timer = setTimeout(() => void tick(), host.isLocal ? 2000 : 5000);
    };
    void tick();
    return () => {
      run.cancelled = true;
      if (timer !== null) clearTimeout(timer);
    };
  }, [path, host]);

  if (message !== null) return <Message text={message} />;
  if (hunks === null) return <Message text="Loading…" />;
  return <InlineCodeView hunks={hunks} filename={name} />;
}

/** Swift `InlineFileTreeRow`: a folder expands to its children, a file to its preview. */
function InlineFileTreeRow({
  node,
  state,
  depth,
}: {
  node: FileNode;
  state: WorkspaceFilesState;
  depth: number;
}) {
  const expanded = useStore(state.store, (s) =>
    node.isDirectory
      ? s.expandedFolders.has(node.relativePath)
      : s.expandedPreviews.has(node.relativePath),
  );
  return (
    <div>
      <button
        type="button"
        className="workspace-tree-row"
        title={node.relativePath}
        aria-label={`${node.relativePath}, ${expanded ? 'Collapse' : 'Expand'}`}
        aria-expanded={expanded}
        style={{ paddingLeft: 10 + depth * 12 }}
        onClick={() => {
          if (node.isDirectory) void state.toggleFolder(node);
          else state.togglePreview(node.relativePath);
        }}
      >
        <Icon name={expanded ? 'chevron.down' : 'chevron.right'} size={8} />
        {node.isDirectory && <Icon name="folder" size={11} className="workspace-tree-folder" />}
        <span className="workspace-file-name">{node.name}</span>
      </button>
      {expanded &&
        (node.isDirectory ? (
          node.children.map((child) => (
            <InlineFileTreeRow
              key={child.relativePath}
              node={child}
              state={state}
              depth={depth + 1}
            />
          ))
        ) : (
          <InlineFilePreview path={node.absolutePath} name={node.name} host={state.fileHost} />
        ))}
    </div>
  );
}

function Files({ state }: { state: WorkspaceFilesState }) {
  const rootNodes = useStore(state.fileTree.store, (s) => s.rootNodes);
  return (
    <>
      {rootNodes.map((node) => (
        <InlineFileTreeRow key={node.relativePath} node={node} state={state} depth={0} />
      ))}
      {rootNodes.length === 0 && <Message text="No files" />}
    </>
  );
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
        <CompactPointGuardChat />
      ) : (
        <div className="workspace-files-scroll">
          {mode === 'files' ? <Files state={state} /> : <Changes state={state} />}
        </div>
      )}
    </div>
  );
}
