// Port of RemoteFolderPickerTests.swift's `FsBrowseRequestTests` (WIN-8.14), the platform-neutral
// part; the rest of that file is `#if os(iOS)`. The Windows picker's own browsing rules are
// `remoteFolderBrowser.test.ts`.
import { describe, expect, it } from 'vitest';
import { fsBrowseRequest } from './machineStateOpenProject';

describe('FsBrowseRequest', () => {
  it('given fsBrowse, should encode a directories-only fs_list', () => {
    const object = JSON.parse(JSON.stringify(fsBrowseRequest('~', false))) as Record<
      string,
      unknown
    >;
    expect(object.type).toBe('fs_list');
    expect(object.path).toBe('~');
    expect(object.include_hidden).toBe(false);
    expect(object.max_entries).toBe(20_000);
    expect(object.directories_only).toBe(true);
    expect(object.recursive).toBeUndefined();
  });

  it('given a plain fs_list, should not ask for directories only', () => {
    const plain = { type: 'fs_list', path: '/srv/app' };
    expect(JSON.parse(JSON.stringify(plain))).not.toHaveProperty('directories_only');
  });
});
