// Port of apps/purepoint-macos/purepoint-macos/Views/FilePane/FilePaneView.swift (WIN-8.5): a tab
// showing a file navigator and editor rooted at the workspace's worktree or project. Its tree and
// editor state live in the tab's FileTabSession (fileTabStore.ts), so they survive the view being
// unmounted while the tab is hidden; the registry's pruning stops their watchers when it closes.
//
// Header: the tree toggle, then the open file's icon, name and unsaved dot (or the root's name).
// The tree is FileTreeSidebarView; the content is FileEditorContent.
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import { detectEditorLanguage } from '../../model/editorLanguage';
import { editorLanguageIcon } from '../../model/editorTab';
import { pathName } from '../../model/fileNode';
import type { FileHost } from '../../services/fileHost';
import { EditorState } from '../../state/editorState';
import type { FileTabSession } from '../../state/fileTabStore';
import { FileTreeState } from '../../state/fileTreeState';
import { Icon } from '../../theme/Icon';
import { FileTreeSidebarView } from '../filetree/FileTreeSidebarView';
import { FileEditorContent } from './FileEditorContent';
import './filePane.css';

/** The session's editor and tree, created the first time the tab is shown. */
export function fileTabStates(session: FileTabSession): {
  editor: EditorState;
  fileTree: FileTreeState;
} {
  const editor = session.editor instanceof EditorState ? session.editor : new EditorState();
  const fileTree =
    session.fileTree instanceof FileTreeState ? session.fileTree : new FileTreeState();
  session.editor = editor;
  session.fileTree = fileTree;
  return { editor, fileTree };
}

export interface FilePaneViewProps {
  rootPath: string;
  /** Where the root lives; remote files are read and saved through the machine's daemon. */
  host: FileHost;
  initialPath: string | null;
  session: FileTabSession;
  isFocused: boolean;
  /** The file shown changed: the tab records it, so its title and the restored file follow. */
  onPathChange: (path: string) => void;
}

export function FilePaneView({
  rootPath,
  host,
  initialPath,
  session,
  isFocused,
  onPathChange,
}: FilePaneViewProps) {
  const [{ editor, fileTree }] = useState(() => fileTabStates(session));
  const [showTree, setShowTreeState] = useState(session.showTree);
  const file = useStore(editor.store, (s) => s.currentFile);
  const setShowTree = (shown: boolean) => {
    session.setShowTree(shown);
    setShowTreeState(shown);
  };

  // Returning to a tab finds its session as it was left: only a first appearance (or a new root)
  // loads the tree and opens the tab's file.
  useEffect(() => {
    editor.setHost(host);
    if (session.loadedRoot !== rootPath) {
      void fileTree.load(rootPath, host);
      session.setLoadedRoot(rootPath);
    }
  }, [editor, fileTree, host, rootPath, session]);

  useEffect(() => {
    if (initialPath === null || editor.currentFile?.id === initialPath) return;
    void editor.openFile(initialPath, pathName(initialPath));
  }, [editor, initialPath]);

  const fileId = file?.id ?? null;
  useEffect(() => {
    if (fileId !== null && fileId !== initialPath) onPathChange(fileId);
  }, [fileId, initialPath, onPathChange]);

  return (
    <div className="file-pane" data-view="FilePaneView" data-testid="file-pane">
      <div className="file-pane-header">
        <button
          type="button"
          className="file-tree-icon-button"
          aria-label={showTree ? 'Hide file tree' : 'Show file tree'}
          title={showTree ? 'Hide file tree' : 'Show file tree'}
          onClick={() => {
            setShowTree(!showTree);
          }}
        >
          <Icon name="sidebar.left" size={14} />
        </button>
        {file !== null ? (
          <>
            <Icon
              name={editorLanguageIcon(detectEditorLanguage(file.name))}
              size={12}
              className="file-pane-file-icon"
            />
            <span className="file-pane-file-name" data-testid="file-pane-name">
              {file.name}
            </span>
            {file.isDirty && (
              <span
                className="file-pane-dirty"
                title="Unsaved changes"
                aria-label="Unsaved changes"
                data-testid="file-pane-dirty"
              />
            )}
          </>
        ) : (
          <span className="file-pane-root-name">{pathName(rootPath)}</span>
        )}
        <span className="file-pane-spacer" />
      </div>
      <div className="file-pane-body">
        {showTree && (
          <div className="file-pane-tree" style={{ width: session.treeWidth }}>
            <FileTreeSidebarView
              fileTreeState={fileTree}
              onHide={() => {
                setShowTree(false);
              }}
              onFileSelected={(path, name) => {
                void editor.openFile(path, name);
              }}
            />
          </div>
        )}
        <FileEditorContent editor={editor} emptyText="Select a file" autoFocus={isFocused} />
      </div>
    </div>
  );
}
