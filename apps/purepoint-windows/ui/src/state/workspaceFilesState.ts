// Port of apps/purepoint-macos/purepoint-macos/State/WorkspaceFilesState.swift: the workspace
// sidebar's state. Changes (WIN-8.2/8.3): the working tree's changed files, polled while the
// sidebar is mounted, and the diffs of the expanded ones. Files (WIN-8.4): the root's FileTreeState
// (loaded on first use), its expanded folders and the files whose inline preview is open.
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in `store`; one instance per root, so
// a read in flight can never fill a different worktree's sidebar (Swift makes a new instance on a
// root change; `WorkspaceFilesLayout` does the same here).

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { FileNode } from '../model/fileNode';
import { daemonFileHost, localFileHost, type FileHost } from '../services/fileHost';
import { machineDaemonClient } from '../services/machines';
import {
  fetchWorkingTreeChanges,
  fetchWorkingTreeFileDiff,
  gitErrorMessage,
  type FileDiff,
} from '../services/git';
import { FileTreeState } from './fileTreeState';

export const WORKSPACE_FILES_MODES = ['changes', 'files', 'pointGuard'] as const;
export type WorkspaceFilesMode = (typeof WORKSPACE_FILES_MODES)[number];

/** Swift `Mode.rawValue`: what the mode bar shows. */
export const WORKSPACE_FILES_MODE_TITLES: Readonly<Record<WorkspaceFilesMode, string>> = {
  changes: 'Changes',
  files: 'Files',
  pointGuard: 'Point Guard',
};

/** The git calls the sidebar makes; tests script them. */
export interface WorkspaceFilesGit {
  fetchWorkingTreeChanges(root: string): Promise<FileDiff[]>;
  fetchWorkingTreeFileDiff(root: string, file: FileDiff): Promise<FileDiff>;
}

export function workspaceFilesGit(machineId: string | null): WorkspaceFilesGit {
  return {
    fetchWorkingTreeChanges: (root) => fetchWorkingTreeChanges(root, machineId),
    fetchWorkingTreeFileDiff: (root, file) => fetchWorkingTreeFileDiff(root, file, machineId),
  };
}

export interface WorkspaceFilesFields {
  readonly mode: WorkspaceFilesMode;
  readonly files: readonly FileDiff[];
  readonly expandedFiles: ReadonlySet<string>;
  readonly diffs: Readonly<Record<string, FileDiff>>;
  readonly fileErrors: Readonly<Record<string, string>>;
  readonly error: string | null;
  readonly isLoading: boolean;
  /** Relative paths of the Files tree's expanded folders. */
  readonly expandedFolders: ReadonlySet<string>;
  /** Relative paths of the files whose inline preview is open. */
  readonly expandedPreviews: ReadonlySet<string>;
}

export interface WorkspaceFilesOptions {
  readonly machineId?: string | null;
  readonly git?: WorkspaceFilesGit;
  /** Where the Files tree lists from (the machine's daemon for a remote root). */
  readonly fileHost?: FileHost;
  /** Swift polls a remote root only while the app is active. */
  readonly isAppActive?: () => boolean;
}

/** Swift `WorkspaceFilesState.identity(rootPath:host:)`: a local and a remote root can share a path. */
export function workspaceFilesIdentity(rootPath: string, machineId: string | null): string {
  return `${machineId === null ? 'local' : `machine:${machineId}`}\0${rootPath}`;
}

function without<T>(record: Readonly<Record<string, T>>, key: string): Record<string, T> {
  const copy = { ...record };
  // eslint-disable-next-line @typescript-eslint/no-dynamic-delete -- a keyed cache, as in Swift
  delete copy[key];
  return copy;
}

function only<T>(record: Readonly<Record<string, T>>, keys: ReadonlySet<string>) {
  return Object.fromEntries(Object.entries(record).filter(([key]) => keys.has(key)));
}

export class WorkspaceFilesState {
  readonly rootPath: string;
  readonly machineId: string | null;
  readonly store: StoreApi<WorkspaceFilesFields>;
  private readonly git: WorkspaceFilesGit;
  private readonly isAppActive: () => boolean;
  private readonly loadingFiles = new Set<string>();
  private observing: object | null = null;
  /** The Files mode's tree, loaded the first time the mode is shown. */
  readonly fileTree = new FileTreeState();
  readonly fileHost: FileHost;
  private treeLoaded = false;

  constructor(rootPath: string, options: WorkspaceFilesOptions = {}) {
    this.rootPath = rootPath;
    this.machineId = options.machineId ?? null;
    this.git = options.git ?? workspaceFilesGit(this.machineId);
    this.fileHost =
      options.fileHost ??
      (this.machineId === null
        ? localFileHost
        : daemonFileHost(this.machineId, machineDaemonClient(this.machineId)));
    this.isAppActive =
      options.isAppActive ?? (() => typeof document === 'undefined' || document.hasFocus());
    this.store = createStore<WorkspaceFilesFields>()(() => ({
      mode: 'changes',
      files: [],
      expandedFiles: new Set(),
      diffs: {},
      fileErrors: {},
      error: null,
      isLoading: true,
      expandedFolders: new Set(),
      expandedPreviews: new Set(),
    }));
  }

  get identity(): string {
    return workspaceFilesIdentity(this.rootPath, this.machineId);
  }

  get isLocal(): boolean {
    return this.machineId === null;
  }

  private get s(): WorkspaceFilesFields {
    return this.store.getState();
  }

  setMode(mode: WorkspaceFilesMode): void {
    this.store.setState({ mode });
    if (mode === 'files') void this.loadTreeIfNeeded();
  }

  /**
   * A .git watcher alone misses normal saves (and a linked worktree's .git is a file), so poll
   * while mounted: every 2 s, or 5 s for a remote root (skipped while the app is inactive).
   * Returns the stop function.
   */
  observe(): () => void {
    const token = {};
    this.observing = token;
    let timer: ReturnType<typeof setTimeout> | null = null;
    const tick = async () => {
      if (this.isLocal || this.isAppActive()) await this.refresh(token);
      if (this.observing !== token) return;
      timer = setTimeout(() => void tick(), this.isLocal ? 2000 : 5000);
    };
    void tick();
    return () => {
      if (this.observing === token) this.observing = null;
      if (timer !== null) clearTimeout(timer);
      // Swift `defer { fileTree.stopWatching() }`; a later observe loads it again.
      this.fileTree.stopWatching();
      this.treeLoaded = false;
    };
  }

  async refresh(token: object | null = this.observing): Promise<void> {
    const current = () => token === null || this.observing === token;
    try {
      const updated = await this.git.fetchWorkingTreeChanges(this.rootPath);
      if (!current()) return;
      const paths = new Set(updated.map((file) => file.filename));
      const expanded = new Set([...this.s.expandedFiles].filter((name) => paths.has(name)));
      this.store.setState({
        files: updated,
        expandedFiles: expanded,
        diffs: only(this.s.diffs, paths),
        fileErrors: only(this.s.fileErrors, paths),
        error: null,
        isLoading: false,
      });
      for (const file of updated) {
        if (!expanded.has(file.filename)) continue;
        await this.loadDiff(file);
        if (!current()) return;
      }
    } catch (error) {
      if (!current()) return;
      this.store.setState({ error: gitErrorMessage(error), isLoading: false });
    }
    if (this.s.mode === 'files' && current()) {
      if (this.treeLoaded) await this.fileTree.refresh();
      else await this.loadTreeIfNeeded();
    }
  }

  /** Swift `loadTreeIfNeeded`: the tree is listed (and watched) the first time Files shows. */
  loadTreeIfNeeded(): Promise<void> {
    if (this.treeLoaded) return Promise.resolve();
    this.treeLoaded = true;
    return this.fileTree.load(this.rootPath, this.fileHost);
  }

  toggleFolder(node: FileNode): Promise<void> {
    const folders = new Set(this.s.expandedFolders);
    if (folders.has(node.relativePath)) {
      folders.delete(node.relativePath);
      this.store.setState({ expandedFolders: folders });
      return this.fileTree.collapseNode(node);
    }
    folders.add(node.relativePath);
    this.store.setState({ expandedFolders: folders });
    return this.fileTree.expandNode(node);
  }

  togglePreview(path: string): void {
    const previews = new Set(this.s.expandedPreviews);
    if (previews.has(path)) previews.delete(path);
    else previews.add(path);
    this.store.setState({ expandedPreviews: previews });
  }

  toggleFile(file: FileDiff): void {
    const expanded = new Set(this.s.expandedFiles);
    if (expanded.has(file.filename)) {
      expanded.delete(file.filename);
      this.store.setState({
        expandedFiles: expanded,
        diffs: without(this.s.diffs, file.filename),
        fileErrors: without(this.s.fileErrors, file.filename),
      });
    } else {
      expanded.add(file.filename);
      this.store.setState({ expandedFiles: expanded });
    }
  }

  async loadDiff(file: FileDiff): Promise<void> {
    if (this.loadingFiles.has(file.filename)) return;
    this.loadingFiles.add(file.filename);
    try {
      const diff = await this.git.fetchWorkingTreeFileDiff(this.rootPath, file);
      if (!this.s.expandedFiles.has(file.filename)) return;
      this.store.setState({
        diffs: { ...this.s.diffs, [file.filename]: diff },
        fileErrors: without(this.s.fileErrors, file.filename),
      });
    } catch (error) {
      this.store.setState({
        fileErrors: { ...this.s.fileErrors, [file.filename]: gitErrorMessage(error) },
      });
    } finally {
      this.loadingFiles.delete(file.filename);
    }
  }
}
