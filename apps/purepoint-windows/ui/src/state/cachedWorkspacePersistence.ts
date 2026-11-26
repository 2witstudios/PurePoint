// The registry's synchronous `WorkspacePersistence` (WIN-6.2) over the async layout IO of WIN-6.3
// (WIN-6.0 integration).
//
// Swift's registry reads and writes `.pu/workspaces.json` synchronously. Here file access is async
// (Tauri commands), so this adapter keeps a mirror of each open project's layout document and
// answers the registry from it:
//
// - **Load at project open.** `open(projectRoot)` reads the document (migrating older ones through
//   `loadWorkspacesForRegistry`) before the project's first manifest is reconciled; the registry's
//   `loadForRegistry` then returns that result synchronously. ProjectState awaits it.
// - **Queued writes.** `saveLocal` decides against the mirror at once, as Swift decides against the
//   disk: a document that changed since the registry's stamp is a conflict to adopt, one written by
//   a newer build is skipped, anything else is written. The write itself is queued per file: one
//   in flight, the latest text pending (earlier pending texts are superseded), each conditional on
//   the modification time last seen on disk. The registry already debounces saves by a second.
// - **Conflict outcome.** A queued write that meets another writer's document (`pu grid`, or a Mac
//   working on this project over SSH) is not retried. The mirror takes the document on disk and
//   `onConflict` fires; the owner calls `registry.adoptLocalLayoutIfChanged`, which sees the new
//   stamp, merges, and saves the merge, as Swift's conflict path does.
//
// Stamps: the registry compares `localModifiedMs` for equality only, so the adapter hands it its own
// version stamps (a counter bumped whenever the mirror's text changes) rather than disk times. A
// queued write gets its stamp immediately; the disk time it produces later never reaches the
// registry, so the registry never mistakes this app's own write for an external change.

import {
  MAX_LAYOUT_BYTES,
  LayoutWriteConflictError,
  documentText,
  isNewerThanSupported,
  loadWorkspacesForRegistry,
  workspaceFilePath,
  workspacesFromText,
  type LayoutHostIO,
  type WorkspaceFileIO,
} from '../model/workspacePersistence';
import type {
  HostFileData,
  LayoutDocumentData,
  RemoteSaveOutcome,
  WorkspacePersistence,
} from '../model/workspacePersistenceTypes';
import type { Workspace } from '../model/workspace';

export interface CachedWorkspacePersistenceOptions {
  /** The local file access (the Tauri `fs_*_text` commands in the app). */
  readonly host: LayoutHostIO;
  /** A queued write met another writer's document; the mirror now holds that document. */
  readonly onConflict?: (projectRoot: string) => void;
  /** A queued write failed for another reason; the mirror keeps this app's text. */
  readonly onWriteError?: (projectRoot: string, message: string) => void;
}

interface Mirror {
  text: string | null;
  truncated: boolean;
  /** The file's modification time on disk as last read or written. */
  diskMs: number | null;
  /** The stamp the registry sees: null exactly when there is no document. */
  stamp: number | null;
}

interface WriteQueue {
  pending: string | null;
  running: Promise<void> | null;
}

type Loaded =
  | { readonly ok: true; readonly workspaces: Workspace[] }
  | { readonly ok: false; readonly error: unknown };

/** Thrown by `loadForRegistry` for a project whose `open` has not finished. */
export class LayoutNotLoadedError extends Error {
  constructor(projectRoot: string) {
    super(`The layout of ${projectRoot} has not been read yet.`);
    this.name = 'LayoutNotLoadedError';
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export class CachedWorkspacePersistence implements WorkspacePersistence {
  readonly maxRemoteLayoutBytes = MAX_LAYOUT_BYTES;

  private readonly host: LayoutHostIO;
  private readonly onConflict: (projectRoot: string) => void;
  private readonly onWriteError: (projectRoot: string, message: string) => void;
  private readonly mirrors = new Map<string, Mirror>();
  private readonly queues = new Map<string, WriteQueue>();
  private readonly loaded = new Map<string, Loaded>();
  private nextStamp = 1;

  constructor(options: CachedWorkspacePersistenceOptions) {
    this.host = options.host;
    this.onConflict = options.onConflict ?? (() => {});
    this.onWriteError = options.onWriteError ?? (() => {});
  }

  // MARK: Async lifecycle

  /**
   * Read a local project's layout so `loadForRegistry` can answer for it. Migration writes go to
   * disk before this resolves. Never rejects: a failed restore is reported by `loadForRegistry`.
   */
  async open(projectRoot: string): Promise<void> {
    const path = this.filePath(projectRoot);
    await this.idle(path);
    try {
      await this.reload(path);
    } catch {
      // As Swift's `try?` read: no document. `loadWorkspacesForRegistry` reads again below.
    }
    const io: WorkspaceFileIO = {
      read: async (target) => {
        if (target === path) return this.mirror(path).text;
        return (await this.host.read(target, MAX_LAYOUT_BYTES))?.text ?? null;
      },
      write: async (target, text) => {
        if (target === path) {
          await this.writeThrough(path, text);
          return;
        }
        const existing = await this.host.stat(target);
        await this.host.write(target, text, existing?.modifiedMs ?? null);
      },
    };
    try {
      this.loaded.set(projectRoot, {
        ok: true,
        workspaces: await loadWorkspacesForRegistry(io, projectRoot),
      });
    } catch (error) {
      this.loaded.set(projectRoot, { ok: false, error });
    }
  }

  /**
   * Re-read the document if its time on disk changed (another writer). Resolves whether the mirror
   * changed; the caller then calls `registry.adoptLocalLayoutIfChanged`. Waits for queued writes
   * first, whose own conflicts are reported through `onConflict`.
   */
  async refresh(projectRoot: string): Promise<boolean> {
    const path = this.filePath(projectRoot);
    if (!this.mirrors.has(path)) return false;
    await this.idle(path);
    let onDisk: number | null;
    try {
      onDisk = (await this.host.stat(path))?.modifiedMs ?? null;
    } catch {
      return false;
    }
    if (onDisk === this.mirror(path).diskMs) return false;
    try {
      return await this.reload(path);
    } catch {
      return false;
    }
  }

  /** Resolves once every queued write has been attempted (quit, close). */
  async flush(): Promise<void> {
    for (;;) {
      const running = [...this.queues.values()].flatMap((q) => (q.running ? [q.running] : []));
      if (running.length === 0) return;
      await Promise.all(running);
    }
  }

  /** Forget a closed project once its writes have gone out. */
  async close(projectRoot: string): Promise<void> {
    const path = this.filePath(projectRoot);
    await this.idle(path);
    this.loaded.delete(projectRoot);
    this.mirrors.delete(path);
    this.queues.delete(path);
  }

  /** Whether `open` has finished for the project. */
  isOpen(projectRoot: string): boolean {
    return this.loaded.has(projectRoot);
  }

  // MARK: WorkspacePersistence

  filePath(projectRoot: string): string {
    return workspaceFilePath(projectRoot);
  }

  loadForRegistry(projectRoot: string): Workspace[] {
    const loaded = this.loaded.get(projectRoot);
    if (loaded === undefined) throw new LayoutNotLoadedError(projectRoot);
    if (!loaded.ok) throw loaded.error;
    return [...loaded.workspaces];
  }

  documentData(
    workspaces: readonly Workspace[],
    activeWorkspaceId: string | null,
  ): LayoutDocumentData | null {
    return documentText(workspaces, activeWorkspaceId);
  }

  workspaces(data: LayoutDocumentData, projectId: string): Workspace[] {
    return workspacesFromText(data, projectId);
  }

  isNewerThanSupported(data: LayoutDocumentData): boolean {
    return isNewerThanSupported(data);
  }

  localModifiedMs(projectRoot: string): number | null {
    return this.mirrors.get(this.filePath(projectRoot))?.stamp ?? null;
  }

  saveLocal(
    data: LayoutDocumentData,
    projectRoot: string,
    expectedModifiedMs: number | null,
  ): RemoteSaveOutcome {
    const path = this.filePath(projectRoot);
    const mirror = this.mirrors.get(path);
    if (mirror === undefined) return { kind: 'skipped' }; // never opened: nothing known to merge
    if (mirror.stamp !== null && mirror.stamp !== expectedModifiedMs) {
      if (mirror.text === null || mirror.truncated || isNewerThanSupported(mirror.text)) {
        return { kind: 'skipped' };
      }
      return { kind: 'conflict', document: mirror.text, modifiedMs: mirror.stamp };
    }
    if (mirror.text !== null && isNewerThanSupported(mirror.text)) return { kind: 'skipped' };
    mirror.text = data;
    mirror.truncated = false;
    mirror.stamp = this.nextStamp++;
    this.enqueue(projectRoot, path, data);
    return { kind: 'written', modifiedMs: mirror.stamp };
  }

  readLocal(path: string, maxBytes: number): HostFileData {
    const mirror = this.mirrors.get(path);
    if (mirror?.text == null || mirror.stamp === null) {
      throw new Error(`${path} could not be read.`);
    }
    const truncated = mirror.truncated || new TextEncoder().encode(mirror.text).length > maxBytes;
    return { data: mirror.text, truncated, modifiedMs: mirror.stamp };
  }

  // MARK: Private

  private mirror(path: string): Mirror {
    let mirror = this.mirrors.get(path);
    if (mirror === undefined) {
      mirror = { text: null, truncated: false, diskMs: null, stamp: null };
      this.mirrors.set(path, mirror);
    }
    return mirror;
  }

  /** Read the document into the mirror; resolves whether its text changed. Rejects on failure. */
  private async reload(path: string): Promise<boolean> {
    const read = await this.host.read(path, MAX_LAYOUT_BYTES);
    const mirror = this.mirror(path);
    const text = read?.text ?? null;
    mirror.diskMs = read?.modifiedMs ?? null;
    mirror.truncated = read?.truncated ?? false;
    if (text === mirror.text && mirror.stamp !== null) return false;
    mirror.text = text;
    mirror.stamp = text === null ? null : this.nextStamp++;
    return true;
  }

  private async writeThrough(path: string, text: string): Promise<void> {
    const mirror = this.mirror(path);
    mirror.diskMs = await this.host.write(path, text, mirror.diskMs);
    mirror.text = text;
    mirror.truncated = false;
    mirror.stamp = this.nextStamp++;
  }

  private queue(path: string): WriteQueue {
    let queue = this.queues.get(path);
    if (queue === undefined) {
      queue = { pending: null, running: null };
      this.queues.set(path, queue);
    }
    return queue;
  }

  private async idle(path: string): Promise<void> {
    for (;;) {
      const running = this.queues.get(path)?.running;
      if (running == null) return;
      await running;
    }
  }

  private enqueue(projectRoot: string, path: string, text: string): void {
    const queue = this.queue(path);
    queue.pending = text;
    queue.running ??= this.drain(projectRoot, path, queue);
  }

  private async drain(projectRoot: string, path: string, queue: WriteQueue): Promise<void> {
    try {
      while (queue.pending !== null) {
        const text = queue.pending;
        queue.pending = null;
        const mirror = this.mirror(path);
        try {
          mirror.diskMs = await this.host.write(path, text, mirror.diskMs);
        } catch (error) {
          if (!(error instanceof LayoutWriteConflictError)) {
            this.onWriteError(projectRoot, message(error));
            continue;
          }
          // Superseded: the registry merges the document on disk and saves the merge.
          queue.pending = null;
          try {
            await this.reload(path);
          } catch (readError) {
            this.onWriteError(projectRoot, message(readError));
            continue;
          }
          this.onConflict(projectRoot);
        }
      }
    } finally {
      queue.running = null;
    }
  }
}
