// Port of apps/purepoint-macos/purepoint-macosTests/SpawnTargetResolverTests.swift (WIN-6.6).
// Each title keeps the Swift test function name.
import { describe, expect, it } from 'vitest';
import { type SpawnSelection, type SpawnTarget, resolveSpawnTarget } from './spawnTargetResolver';

function resolve({
  isWorktree = false,
  selection = null,
  knownWorktrees = {},
}: {
  isWorktree?: boolean;
  selection?: SpawnSelection | null;
  knownWorktrees?: Record<string, string>;
} = {}): SpawnTarget {
  return resolveSpawnTarget({
    isWorktree,
    selection,
    worktreeIdForWorkspace: (id) => knownWorktrees[id],
  });
}

describe('SpawnTargetResolverTests', () => {
  it('isWorktreeReturnsNonRootWithNilWorktree', () => {
    expect(resolve({ isWorktree: true })).toEqual({ root: false, worktree: null });
  });

  it('isWorktreeTakesPrecedenceOverSelection', () => {
    expect(resolve({ isWorktree: true, selection: { kind: 'worktree', id: 'wt1' } })).toEqual({
      root: false,
      worktree: null,
    });
  });

  it('worktreeSelectionReturnsWorktreeId', () => {
    expect(resolve({ selection: { kind: 'worktree', id: 'wt1' } })).toEqual({
      root: false,
      worktree: 'wt1',
    });
  });

  it('workspaceSelectionInWorktreeSpawnsIntoThatWorktree', () => {
    const target = resolve({
      selection: { kind: 'workspace', id: 'ws-1' },
      knownWorktrees: { 'ws-1': 'wt-matched' },
    });
    expect(target).toEqual({ root: false, worktree: 'wt-matched' });
  });

  it('workspaceSelectionInProjectRootSpawnsAtRoot', () => {
    expect(resolve({ selection: { kind: 'workspace', id: 'ws-1' } })).toEqual({
      root: true,
      worktree: null,
    });
  });

  it('nilSelectionReturnsRoot', () => {
    expect(resolve()).toEqual({ root: true, worktree: null });
  });

  it('navSelectionReturnsRoot', () => {
    expect(resolve({ selection: { kind: 'nav', item: 'dashboard' } })).toEqual({
      root: true,
      worktree: null,
    });
  });

  it('projectSelectionReturnsRoot', () => {
    expect(resolve({ selection: { kind: 'project', id: '/some/path' } })).toEqual({
      root: true,
      worktree: null,
    });
  });
});
