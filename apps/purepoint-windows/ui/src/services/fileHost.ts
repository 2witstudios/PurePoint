// Where a file tree's (and an editor's) files live (WIN-8.4): the Windows side of the Mac's
// `ProjectHost` as the file views use it (`Services/ProjectHost.swift`), with the reads of
// `Services/FileIOService.swift` (binary detection, the size limits). This PC goes through the
// backend's file commands (`src-tauri/src/files/{tree,text}.rs`) and the `file-tree-changed`
// watcher; a remote project goes through its machine's daemon (`fs_list`, `fs_read`, `fs_stat`,
// `fs_write`, `fs_delete`).
//
// Difference from Swift: a remote host's watcher is a 5 s poll while the app is active rather than
// a daemon `subscribe_fs` stream (the relay is request/response); the tree and editor refresh the
// same way on either.
import { invoke } from '@tauri-apps/api/core';
import { relativePath, samePath } from '../model/fileNode';
import type { Response } from '../protocol';
import type { DaemonRequester } from './daemonConnection';
import { onFileTreeChanged, unwatch, watchFileTree } from './watchers';

/** One directory entry (the daemon's `FsEntry`, the backend's `DirEntry`). */
export interface FileHostEntry {
  readonly name: string;
  readonly path: string;
  readonly isDirectory: boolean;
  /** `git check-ignore` reports it ignored. */
  readonly ignored: boolean;
}

/** A file's text as read through a host (Swift `HostTextFile`). */
export interface HostTextFile {
  readonly content: string;
  /** A NUL byte in the first 8 KB, or bytes that are not UTF-8: `content` is empty. */
  readonly isBinary: boolean;
  /** The modification time of the version read: a later save is checked against it. */
  readonly modifiedMs: number;
}

function formatBytes(bytes: number): string {
  return bytes >= 1_048_576
    ? `${String(Math.round(bytes / 1_048_576))} MB`
    : `${String(Math.round(bytes / 1000))} KB`;
}

/** Swift `FilePreviewError.tooLarge`. */
export class FileTooLargeError extends Error {
  constructor(readonly limit: number) {
    super(`File is too large to open (over ${formatBytes(limit)}).`);
    this.name = 'FileTooLargeError';
  }
}

/** A conditional write refused: the file changed since the version the caller saw. */
export class FileWriteConflictError extends Error {
  constructor(path: string) {
    super(`${path} changed since it was read.`);
    this.name = 'FileWriteConflictError';
  }
}

export class FileNotFoundError extends Error {
  constructor(path: string) {
    super(`${path} does not exist.`);
    this.name = 'FileNotFoundError';
  }
}

/** The most a local editor opens (the backend's read cap). */
export const LOCAL_READ_LIMIT = 8 * 1_048_576;
/** Swift `FileIOService.remoteReadLimit`: the daemon's default read. */
export const REMOTE_READ_LIMIT = 1_048_576;
/** Swift `HostError.maxRemoteSaveBytes`: a remote save travels in one request line. */
export const MAX_REMOTE_SAVE_BYTES = 750_000;

/** Swift `FileIOService.decode`: binary when a NUL is in the first 8 KB or it is not UTF-8. */
export function decodeFileBytes(bytes: Uint8Array): { content: string; isBinary: boolean } {
  if (bytes.subarray(0, 8192).includes(0)) return { content: '', isBinary: true };
  try {
    return { content: new TextDecoder('utf-8', { fatal: true }).decode(bytes), isBinary: false };
  } catch {
    return { content: '', isBinary: true };
  }
}

function base64ToBytes(base64: string): Uint8Array {
  const binary = atob(base64);
  return Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
}

function textToBase64(text: string): string {
  let binary = '';
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

export interface FileHost {
  readonly isLocal: boolean;
  /** Swift `ProjectHost.identity`: `local`, or the machine. */
  readonly identity: string;
  /** The entries of `directory`, ignore verdicts taken in the repository at `root`. */
  list(directory: string, root: string): Promise<FileHostEntry[]>;
  /** The file's text; rejects `FileNotFoundError`, or `FileTooLargeError` past `maxBytes`. */
  read(path: string, maxBytes?: number): Promise<HostTextFile>;
  /** The modification time, null when there is no file. */
  stat(path: string): Promise<number | null>;
  /**
   * Writes `text` only if the file's modification time is still `expectedModifiedMs`; resolves the
   * new time, rejects `FileWriteConflictError` when it changed.
   */
  write(path: string, text: string, expectedModifiedMs: number | null): Promise<number>;
  /** Rename in place; null when the host cannot (the daemon has no rename request). */
  readonly rename: ((from: string, to: string) => Promise<void>) | null;
  remove(path: string): Promise<void>;
  /** Calls `onChange` with the changed paths (empty: unknown) while `directory` changes. */
  watch(directory: string, onChange: (paths: readonly string[]) => void): () => void;
}

/** How the backend's file commands reject (`FsError`). */
interface FsCommandError {
  kind: string;
  message: string;
}

function isConflict(error: unknown): boolean {
  return (
    typeof error === 'object' && error !== null && (error as FsCommandError).kind === 'conflict'
  );
}

export function fileHostErrorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = (error as FsCommandError).message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}

/** What `fs_read_document` returns. */
interface DocumentFile {
  text: string;
  isBinary: boolean;
  modifiedMs: number;
  truncated: boolean;
}

function appIsActive(): boolean {
  return typeof document === 'undefined' || document.hasFocus();
}

/** This PC: the backend's commands and the recursive file-tree watcher. */
export const localFileHost: FileHost = {
  isLocal: true,
  identity: 'local',
  list: (directory, root) => invoke<FileHostEntry[]>('fs_list_dir', { path: directory, root }),
  async read(path, maxBytes = LOCAL_READ_LIMIT) {
    const file = await invoke<DocumentFile | null>('fs_read_document', { path, maxBytes });
    if (file === null) throw new FileNotFoundError(path);
    if (file.truncated) throw new FileTooLargeError(maxBytes);
    return { content: file.text, isBinary: file.isBinary, modifiedMs: file.modifiedMs };
  },
  stat: (path) => invoke<number | null>('fs_stat_text', { path }),
  async write(path, text, expectedModifiedMs) {
    try {
      return await invoke<number>('fs_write_text_if_unmodified', {
        path,
        text,
        expectedModifiedMs,
      });
    } catch (error) {
      if (isConflict(error)) throw new FileWriteConflictError(path);
      throw error;
    }
  },
  rename: (from, to) => invoke<undefined>('fs_rename', { from, to }),
  remove: (path) => invoke<undefined>('fs_delete', { path }),
  watch(directory, onChange) {
    let stopped = false;
    let id: number | null = null;
    let unlisten: (() => void) | null = null;
    void onFileTreeChanged((event) => {
      if (stopped || !samePath(event.root, directory)) return;
      onChange(event.paths);
    }).then((stop) => {
      if (stopped) stop();
      else unlisten = stop;
    });
    void watchFileTree(directory)
      .then((watchId) => {
        if (stopped) void unwatch(watchId).catch(() => false);
        else id = watchId;
      })
      .catch((error: unknown) => {
        console.warn(`PurePoint: could not watch ${directory}`, error);
      });
    return () => {
      stopped = true;
      unlisten?.();
      if (id !== null) void unwatch(id).catch(() => false);
    };
  },
};

function daemonError(response: Response): Error {
  return new Error(
    response.type === 'error' ? response.message : `Unexpected response: ${response.type}`,
  );
}

/** Swift's remote watcher interval: a daemon round trip, so every 5 s and only while active. */
export const REMOTE_POLL_MS = 5000;

/** A machine's daemon (Swift `DaemonProjectHost`). */
export function daemonFileHost(
  machineId: string,
  client: DaemonRequester,
  isActive: () => boolean = appIsActive,
): FileHost {
  return {
    isLocal: false,
    identity: `machine:${machineId}`,
    async list(directory) {
      const response = await client.send({ type: 'fs_list', path: directory });
      if (response.type !== 'fs_list_result') throw daemonError(response);
      return response.entries.map((entry) => ({
        name: entry.name,
        path: entry.path,
        isDirectory: entry.is_dir,
        ignored: entry.ignored,
      }));
    },
    async read(path, maxBytes = REMOTE_READ_LIMIT) {
      const response = await client.send({ type: 'fs_read', path, max_bytes: maxBytes });
      if (response.type === 'error' && response.code === 'not_found') {
        throw new FileNotFoundError(path);
      }
      if (response.type !== 'fs_read_result') throw daemonError(response);
      // Like local files over the limit, an over-long remote file is not shown.
      if (response.truncated) throw new FileTooLargeError(maxBytes);
      const decoded = decodeFileBytes(base64ToBytes(response.data_base64));
      return { ...decoded, modifiedMs: response.modified_ms };
    },
    async stat(path) {
      const response = await client.send({ type: 'fs_stat', path });
      if (response.type === 'fs_stat_result') return response.exists ? response.modified_ms : null;
      if (response.type === 'error' && response.code === 'not_found') return null;
      throw daemonError(response);
    },
    async write(path, text, expectedModifiedMs) {
      const response = await client.send({
        type: 'fs_write',
        path,
        data_base64: textToBase64(text),
        expected_modified_ms: expectedModifiedMs,
        create_dirs: false,
      });
      if (response.type === 'fs_write_result') return response.modified_ms;
      if (response.type === 'error' && response.code === 'conflict') {
        throw new FileWriteConflictError(path);
      }
      throw daemonError(response);
    },
    rename: null,
    async remove(path) {
      const response = await client.send({ type: 'fs_delete', path });
      if (response.type === 'error') throw daemonError(response);
    },
    watch(_directory, onChange) {
      const timer = setInterval(() => {
        if (isActive()) onChange([]);
      }, REMOTE_POLL_MS);
      return () => {
        clearInterval(timer);
      };
    },
  };
}

/** Whether a watcher's changed paths may touch `path` (empty paths: anything changed). */
export function changeTouches(paths: readonly string[], path: string): boolean {
  return paths.length === 0 || paths.some((changed) => relativePath(changed, path) !== null);
}

/** The host of a project's files: this PC, or the machine's daemon the project talks to. */
export function projectFileHost(
  project: { readonly machineId: string | null; readonly client: DaemonRequester } | null,
): FileHost {
  if (project?.machineId == null) return localFileHost;
  return daemonFileHost(project.machineId, project.client);
}
