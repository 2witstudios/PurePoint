// Front end of the project watchers (WIN-5.6): `src-tauri/src/watchers/`, ported from
// Services/{ManifestWatcher,WorktreeWatcher,FileTreeWatcher,MachineRegistryWatcher}.swift.
// Each `watch*` call starts a debounced backend watcher and resolves to its id; the backend
// raises one Tauri event per write burst, which the `on*` listeners receive.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { Request } from '../protocol';

export type WatchId = number;

export const WATCHER_EVENTS = {
  manifest: 'manifest-changed',
  worktrees: 'worktrees-changed',
  fileTree: 'file-tree-changed',
  machines: 'machines-changed',
} as const;

export interface ManifestChanged {
  projectRoot: string;
}

export interface WorktreesChanged {
  worktreePath: string;
}

export interface FileTreeChanged {
  root: string;
  /** Changed paths under `root` (at most 256; hidden names such as `.git` excluded). */
  paths: string[];
}

/** What `watchers_release` returns. */
export interface Released {
  token: number;
  /** How many watchers were stopped. */
  released: number;
}

/** Watches `<projectRoot>\.pu\manifest.json` (ProjectState refreshes on change). */
export function watchManifest(projectRoot: string): Promise<WatchId> {
  return invoke<WatchId>('watch_manifest', { projectRoot });
}

/** Watches a worktree's git metadata: HEAD, index and refs (DiffState refreshes). */
export function watchWorktree(worktreePath: string): Promise<WatchId> {
  return invoke<WatchId>('watch_worktree', { worktreePath });
}

/** Watches a file-tree root recursively (FileTreeState refreshes). */
export function watchFileTree(root: string): Promise<WatchId> {
  return invoke<WatchId>('watch_file_tree', { root });
}

/** Watches `%USERPROFILE%\.pu\machines.json` (AppState reloads machines). */
export function watchMachines(): Promise<WatchId> {
  return invoke<WatchId>('watch_machines');
}

/** Stops a watcher; resolves false for an unknown id. */
export function unwatch(id: WatchId): Promise<boolean> {
  return invoke<boolean>('unwatch', { id });
}

export function onManifestChanged(handler: (event: ManifestChanged) => void): Promise<UnlistenFn> {
  return listen<ManifestChanged>(WATCHER_EVENTS.manifest, (event) => {
    handler(event.payload);
  });
}

export function onWorktreesChanged(
  handler: (event: WorktreesChanged) => void,
): Promise<UnlistenFn> {
  return listen<WorktreesChanged>(WATCHER_EVENTS.worktrees, (event) => {
    handler(event.payload);
  });
}

export function onFileTreeChanged(handler: (event: FileTreeChanged) => void): Promise<UnlistenFn> {
  return listen<FileTreeChanged>(WATCHER_EVENTS.fileTree, (event) => {
    handler(event.payload);
  });
}

export function onMachinesChanged(handler: () => void): Promise<UnlistenFn> {
  return listen(WATCHER_EVENTS.machines, () => {
    handler();
  });
}

/** Stops every watcher at or under `path` once their handles are closed (WIN-3.13). */
export function releaseWatchers(path: string): Promise<Released> {
  return invoke<Released>('watchers_release', { path });
}

/** Re-creates a release's watchers; resolves to how many came back. */
export function restoreWatchers(token: number): Promise<number> {
  return invoke<number>('watchers_restore', { token });
}

/** Forgets a release whose request succeeded. */
export function discardWatchers(token: number): Promise<boolean> {
  return invoke<boolean>('watchers_discard', { token });
}

/**
 * The WIN-3.13 contract (REQ-WINAPP-043): releases the watchers under `path`, runs `request`
 * (the `delete_worktree`, `kill` or `clean` that removes the worktree), and re-creates the
 * watchers if it fails. The request's rejection is passed through unchanged.
 */
export async function withWatchersReleased<T>(path: string, request: () => Promise<T>): Promise<T> {
  const { token } = await releaseWatchers(path);
  let result: T;
  try {
    result = await request();
  } catch (error) {
    await restoreWatchers(token).catch(() => 0);
    throw error;
  }
  await discardWatchers(token).catch(() => false);
  return result;
}

/** `pu_core::paths::worktree_path`: `<projectRoot>\.pu\worktrees\<id>`. */
export function worktreePath(projectRoot: string, worktreeId: string): string {
  return joinPath(projectRoot, '.pu', 'worktrees', worktreeId);
}

/**
 * The directory whose watchers must be released before sending `request`, or null if the
 * request removes no worktree. `kill` of all worktrees releases the whole worktrees dir.
 */
export function releasePathFor(request: Request): string | null {
  if (request.type === 'delete_worktree') {
    return worktreePath(request.project_root, request.worktree_id);
  }
  if (request.type === 'kill') {
    const { target } = request;
    if (target === 'all_worktrees') return joinPath(request.project_root, '.pu', 'worktrees');
    if (typeof target === 'object' && 'worktree' in target) {
      return worktreePath(request.project_root, target.worktree);
    }
  }
  return null;
}

/** Sends `request` through `send`, releasing the watchers of the worktree it removes. */
export function sendReleasingWatchers<T>(
  request: Request,
  send: (request: Request) => Promise<T>,
): Promise<T> {
  const path = releasePathFor(request);
  return path === null ? send(request) : withWatchersReleased(path, () => send(request));
}

function joinPath(root: string, ...parts: string[]): string {
  const separator = root.includes('/') && !root.includes('\\') ? '/' : '\\';
  const trimmed = root.replace(/[\\/]+$/, '');
  return [trimmed, ...parts].join(separator);
}
