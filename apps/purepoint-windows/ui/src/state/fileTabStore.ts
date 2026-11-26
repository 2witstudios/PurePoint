// Port of apps/purepoint-macos/purepoint-macos/State/FileTabStore.swift (WIN-6.2).
//
// Everything a file tab shows besides the file path: navigator, editor (including unsaved edits)
// and view toggles. It lives here rather than in component state because only a pane's active tab
// is rendered: switching tabs, or moving a tab to another pane, unmounts the view, and must not
// throw away an edit in progress.

/** The editor state a file tab keeps (Swift `EditorState`; its port attaches here). */
export interface FileTabEditor {
  readonly currentFile: { readonly name: string; readonly isDirty: boolean } | null;
  stopWatching(): void;
}

/** The navigator state a file tab keeps (Swift `FileTreeState`; its port attaches here). */
export interface FileTabFileTree {
  stopWatching(): void;
}

const NO_EDITOR: FileTabEditor = { currentFile: null, stopWatching: () => undefined };
const NO_FILE_TREE: FileTabFileTree = { stopWatching: () => undefined };

export class FileTabSession {
  /** Set by the file pane once it creates its editor; a fresh session has none open. */
  editor: FileTabEditor = NO_EDITOR;
  fileTree: FileTabFileTree = NO_FILE_TREE;
  showTree: boolean;
  showPreview = true;
  treeWidth = 200;
  /** The root the navigator was loaded from, so returning to the tab does not rescan. */
  loadedRoot: string | null = null;

  constructor(showTree: boolean) {
    this.showTree = showTree;
  }

  setShowTree(showTree: boolean): void {
    this.showTree = showTree;
  }

  setShowPreview(showPreview: boolean): void {
    this.showPreview = showPreview;
  }

  setLoadedRoot(root: string): void {
    this.loadedRoot = root;
  }

  get hasUnsavedChanges(): boolean {
    return this.editor.currentFile?.isDirty ?? false;
  }

  stopWatching(): void {
    this.fileTree.stopWatching();
    this.editor.stopWatching();
  }
}

/** Names a tab for its whole life: tabs only move within a workspace, and surface IDs are unique there. */
export interface FileTabKey {
  readonly workspaceId: string;
  readonly surfaceId: number;
}

export function fileTabKeyString(key: FileTabKey): string {
  return `${String(key.surfaceId)}\u0000${key.workspaceId}`;
}

/**
 * File tab sessions by tab. `WorkspaceRegistry` prunes the store whenever its layout changes,
 * which is how a closed tab's session (and its file watchers) goes away.
 */
export class FileTabStore {
  private readonly sessions = new Map<string, FileTabSession>();

  session(workspaceId: string, surfaceId: number, initialPath: string | null): FileTabSession {
    const key = fileTabKeyString({ workspaceId, surfaceId });
    const existing = this.sessions.get(key);
    if (existing !== undefined) return existing;
    const session = new FileTabSession(initialPath === null);
    this.sessions.set(key, session);
    return session;
  }

  existingSession(workspaceId: string, surfaceId: number): FileTabSession | null {
    return this.sessions.get(fileTabKeyString({ workspaceId, surfaceId })) ?? null;
  }

  /** Drop every session whose tab no longer exists. */
  retainOnly(live: Iterable<FileTabKey>): void {
    const keep = new Set([...live].map(fileTabKeyString));
    for (const [key, session] of [...this.sessions]) {
      if (keep.has(key)) continue;
      this.sessions.delete(key);
      session.stopWatching();
    }
  }
}
