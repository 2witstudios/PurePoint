// Front end of Services/FileIndex.swift (WIN-6.6). Local projects list in the Rust backend
// (`src-tauri/src/files/index.rs`, the `file_index` command); remote projects list through the
// daemon, and `entriesFromListing` applies the same rules to that listing.
import { invoke } from '@tauri-apps/api/core';
import type { PaletteFileEntry } from '../model/commandPalette';
import type { FsEntry } from '../protocol';

/** Same bound as the Mac's `FileIndex.defaultLimit`. */
export const FILE_INDEX_DEFAULT_LIMIT = 5000;

/** The file tree's noise directories and files; kept in step with `SKIPPED_NAMES` in Rust. */
export const FILE_INDEX_SKIPPED_NAMES: ReadonlySet<string> = new Set([
  '.git',
  '.DS_Store',
  '.build',
  '.swiftpm',
  'xcuserdata',
  'DerivedData',
  '__pycache__',
  '.tsbuildinfo',
  'node_modules',
  'target',
]);

/** Up to `limit` files under a local `root`, skipping the file tree's noise directories. */
export function fileIndex(
  root: string,
  limit: number = FILE_INDEX_DEFAULT_LIMIT,
): Promise<PaletteFileEntry[]> {
  return invoke<PaletteFileEntry[]>('file_index', { root, limit });
}

/** The daemon lists directories too; ask it for headroom so the file limit is still reached. */
export const remoteListingLimit = (limit: number = FILE_INDEX_DEFAULT_LIMIT): number =>
  Math.min(limit * 4, 20_000);

/** Palette entries from a daemon recursive listing of `root` (remote hosts). */
export function entriesFromListing(
  root: string,
  entries: readonly FsEntry[],
  limit: number = FILE_INDEX_DEFAULT_LIMIT,
): PaletteFileEntry[] {
  const result: PaletteFileEntry[] = [];
  for (const entry of entries) {
    if (result.length >= limit) break;
    if (entry.is_dir || entry.ignored) continue;
    const relativePath = entry.path.startsWith(`${root}/`)
      ? entry.path.slice(root.length + 1)
      : entry.path;
    if (relativePath.split('/').some((part) => FILE_INDEX_SKIPPED_NAMES.has(part))) continue;
    result.push({ relativePath, absolutePath: entry.path });
  }
  return result;
}
