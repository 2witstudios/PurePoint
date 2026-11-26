// Front end of the local text-file commands (WIN-6.0): `src-tauri/src/files/text.rs`. The layout
// document's reads and conditional writes on this PC; remote projects go through their daemon's
// `fs_*` requests (`daemonLayoutHostIO`).
import { invoke } from '@tauri-apps/api/core';

import {
  LayoutWriteConflictError,
  type HostFileText,
  type LayoutHostIO,
} from '../model/workspacePersistence';
import type { Response } from '../protocol';
import type { DaemonRequester } from './daemonConnection';

/** How the file commands reject. */
export interface FsCommandError {
  kind: 'conflict' | 'io';
  message: string;
}

export function isFsCommandError(error: unknown): error is FsCommandError {
  return (
    typeof error === 'object' &&
    error !== null &&
    ((error as FsCommandError).kind === 'conflict' || (error as FsCommandError).kind === 'io') &&
    typeof (error as FsCommandError).message === 'string'
  );
}

/** The file's modification time (ms since the epoch), or null when there is no file. */
export function fsStatText(path: string): Promise<number | null> {
  return invoke<number | null>('fs_stat_text', { path });
}

/** The file as text (lossy UTF-8, at most `maxBytes`), or null when there is no file. */
export function fsReadText(path: string, maxBytes?: number): Promise<HostFileText | null> {
  return invoke<HostFileText | null>('fs_read_text', { path, maxBytes: maxBytes ?? null });
}

/**
 * Writes `text` only if the file's modification time is still `expectedModifiedMs` (null: only if
 * there is no file), creating its directory; resolves the new time, rejects `conflict` otherwise.
 */
export function fsWriteTextIfUnmodified(
  path: string,
  text: string,
  expectedModifiedMs: number | null,
): Promise<number> {
  return invoke<number>('fs_write_text_if_unmodified', { path, text, expectedModifiedMs });
}

/** `LayoutHostIO` over the local commands (Swift `LocalProjectHost`). */
export const localLayoutHostIO: LayoutHostIO = {
  stat: async (path) => {
    const modifiedMs = await fsStatText(path);
    return modifiedMs === null ? null : { modifiedMs };
  },
  read: (path, maxBytes) => fsReadText(path, maxBytes),
  write: async (path, text, expectedModifiedMs) => {
    try {
      return await fsWriteTextIfUnmodified(path, text, expectedModifiedMs);
    } catch (error) {
      if (isFsCommandError(error) && error.kind === 'conflict') {
        throw new LayoutWriteConflictError();
      }
      throw error;
    }
  },
};

function base64ToText(base64: string): string {
  const binary = atob(base64);
  const bytes = Uint8Array.from(binary, (ch) => ch.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

function textToBase64(text: string): string {
  let binary = '';
  for (const byte of new TextEncoder().encode(text)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

class DaemonFsError extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = 'DaemonFsError';
  }
}

/**
 * `LayoutHostIO` over a machine's daemon (Swift `DaemonProjectHost`): `fs_stat`, `fs_read` and
 * `fs_write` with `expected_modified_ms`, whose `conflict` reply is a write conflict.
 */
export function daemonLayoutHostIO(client: DaemonRequester): LayoutHostIO {
  return {
    stat: async (path) => {
      const response = await client.send({ type: 'fs_stat', path });
      if (response.type === 'fs_stat_result') {
        return response.exists ? { modifiedMs: response.modified_ms } : null;
      }
      if (response.type === 'error' && response.code === 'not_found') return null;
      throw replyError(response);
    },
    read: async (path, maxBytes) => {
      const response = await client.send({ type: 'fs_read', path, max_bytes: maxBytes });
      if (response.type === 'fs_read_result') {
        return {
          text: base64ToText(response.data_base64),
          modifiedMs: response.modified_ms,
          truncated: response.truncated,
        };
      }
      if (response.type === 'error' && response.code === 'not_found') return null;
      throw replyError(response);
    },
    write: async (path, text, expectedModifiedMs) => {
      const response = await client.send({
        type: 'fs_write',
        path,
        data_base64: textToBase64(text),
        expected_modified_ms: expectedModifiedMs,
        create_dirs: true,
      });
      if (response.type === 'fs_write_result') return response.modified_ms;
      if (response.type === 'error' && response.code === 'conflict') {
        throw new LayoutWriteConflictError();
      }
      throw replyError(response);
    },
  };
}

function replyError(response: Response): Error {
  return response.type === 'error'
    ? new DaemonFsError(response.code, response.message)
    : new DaemonFsError('UNEXPECTED', `Unexpected response from daemon: ${response.type}`);
}
