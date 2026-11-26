// An in-memory FileHost for the file tree, editor and preview tests (WIN-8.4/8.5): files by
// absolute path (folders are implied by their files, or listed in `folders`), modification times
// that advance on every write, conditional writes, and watchers a test fires by writing "from
// outside" (`writeExternally`).
import { pathName, relativePath } from '../model/fileNode';
import {
  FileNotFoundError,
  FileTooLargeError,
  FileWriteConflictError,
  type FileHost,
  type FileHostEntry,
} from './fileHost';

export interface MemoryFileHost extends FileHost {
  readonly files: Map<string, { content: string; modifiedMs: number; binary: boolean }>;
  readonly folders: Set<string>;
  /** Relative paths `git check-ignore` reports (the current rules). */
  ignored: (relative: string) => boolean;
  /** Write as another program would: bumps the time and notifies the watchers. */
  writeExternally(path: string, content: string): void;
  /** Notify the watchers of a change under `path`. */
  notify(path: string): void;
  /** Every `list` call, for tests that count listings. */
  readonly listed: string[];
  /** Listings wait for this before answering, when set. */
  gate: Promise<void> | null;
}

export function memoryFileHost(
  initial: Record<string, string>,
  options: { folders?: readonly string[]; isLocal?: boolean; separator?: '\\' | '/' } = {},
): MemoryFileHost {
  const separator = options.separator ?? '\\';
  let clock = 1_000;
  const files = new Map<string, { content: string; modifiedMs: number; binary: boolean }>();
  for (const [path, content] of Object.entries(initial)) {
    files.set(path, { content, modifiedMs: (clock += 1), binary: content.includes('\0') });
  }
  const folders = new Set(options.folders ?? []);
  const watchers = new Set<{ directory: string; onChange: (paths: readonly string[]) => void }>();
  const parentOf = (path: string) => path.slice(0, path.lastIndexOf(separator));
  const allFolders = () => {
    const all = new Set(folders);
    for (const path of files.keys()) {
      let parent = parentOf(path);
      while (parent.includes(separator)) {
        all.add(parent);
        parent = parentOf(parent);
      }
    }
    return all;
  };

  const host: MemoryFileHost = {
    isLocal: options.isLocal ?? true,
    identity: options.isLocal === false ? 'machine:test' : 'local',
    files,
    folders,
    ignored: () => false,
    listed: [],
    gate: null,
    async list(directory, root) {
      host.listed.push(directory);
      if (host.gate !== null) await host.gate;
      const entries = new Map<string, FileHostEntry>();
      const known = allFolders();
      if (!known.has(directory) && ![...files.keys()].some((p) => parentOf(p) === directory)) {
        throw new FileNotFoundError(directory);
      }
      const add = (path: string, isDirectory: boolean) => {
        if (parentOf(path) !== directory) return;
        const name = pathName(path);
        const relative = relativePath(root, path) ?? name;
        entries.set(path, {
          name,
          path,
          isDirectory,
          ignored: !name.startsWith('.') && host.ignored(relative),
        });
      };
      for (const path of files.keys()) add(path, false);
      for (const path of known) add(path, true);
      return [...entries.values()];
    },
    read(path, maxBytes = Infinity) {
      const file = files.get(path);
      if (file === undefined) return Promise.reject(new FileNotFoundError(path));
      if (file.content.length > maxBytes) return Promise.reject(new FileTooLargeError(maxBytes));
      return Promise.resolve({
        content: file.binary ? '' : file.content,
        isBinary: file.binary,
        modifiedMs: file.modifiedMs,
      });
    },
    stat(path) {
      return Promise.resolve(files.get(path)?.modifiedMs ?? null);
    },
    write(path, text, expectedModifiedMs) {
      const current = files.get(path)?.modifiedMs ?? null;
      if (current !== expectedModifiedMs) {
        return Promise.reject(new FileWriteConflictError(path));
      }
      const modifiedMs = (clock += 1);
      files.set(path, { content: text, modifiedMs, binary: false });
      return Promise.resolve(modifiedMs);
    },
    // The daemon has no rename request.
    rename:
      options.isLocal === false
        ? null
        : (from, to) => {
            const file = files.get(from);
            if (file === undefined) return Promise.reject(new FileNotFoundError(from));
            files.delete(from);
            files.set(to, { ...file, modifiedMs: (clock += 1) });
            return Promise.resolve();
          },
    remove(path) {
      files.delete(path);
      for (const key of [...files.keys()]) if (key.startsWith(path + separator)) files.delete(key);
      folders.delete(path);
      return Promise.resolve();
    },
    watch(directory, onChange) {
      const watcher = { directory, onChange };
      watchers.add(watcher);
      return () => {
        watchers.delete(watcher);
      };
    },
    writeExternally(path, content) {
      files.set(path, { content, modifiedMs: (clock += 1), binary: false });
      host.notify(path);
    },
    notify(path) {
      for (const watcher of [...watchers]) {
        if (relativePath(watcher.directory, path) !== null) watcher.onChange([path]);
      }
    },
  };
  return host;
}
