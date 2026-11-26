// Remote-host half of Services/FileIndex.swift (WIN-6.6); the local walk is tested in Rust.
import { describe, expect, it } from 'vitest';
import type { FsEntry } from '../protocol';
import { entriesFromListing, remoteListingLimit } from './fileIndex';

const entry = (path: string, overrides: Partial<FsEntry> = {}): FsEntry => ({
  name: path.split('/').at(-1) ?? path,
  path,
  is_dir: false,
  is_symlink: false,
  size: 1,
  modified_ms: 0,
  ignored: false,
  ...overrides,
});

describe('FileIndex remote listing', () => {
  it('keeps files relative to root and drops directories, ignored and noise entries', () => {
    const listing = [
      entry('/r/src', { is_dir: true }),
      entry('/r/src/main.rs'),
      entry('/r/build.log', { ignored: true }),
      entry('/r/node_modules/x/index.js'),
      entry('/r/pkg/.git/HEAD'),
      entry('/elsewhere/a.txt'),
    ];
    expect(entriesFromListing('/r', listing)).toEqual([
      { relativePath: 'src/main.rs', absolutePath: '/r/src/main.rs' },
      { relativePath: '/elsewhere/a.txt', absolutePath: '/elsewhere/a.txt' },
    ]);
  });

  it('stops at the limit', () => {
    const listing = ['a', 'b', 'c'].map((n) => entry(`/r/${n}`));
    expect(entriesFromListing('/r', listing, 2).map((e) => e.relativePath)).toEqual(['a', 'b']);
  });

  it('asks the daemon for headroom, capped at 20000', () => {
    expect(remoteListingLimit(100)).toBe(400);
    expect(remoteListingLimit()).toBe(20_000);
  });
});
