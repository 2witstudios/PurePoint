// Command wiring of the git service wrappers; the git behaviour is tested in Rust.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

const git = await import('./git');

const file = (overrides: Partial<import('./git').FileDiff> = {}): import('./git').FileDiff => ({
  filename: 'a.txt',
  statusCode: 'M',
  added: 1,
  removed: 0,
  hunks: [],
  oldFilename: null,
  isBinary: false,
  previewError: null,
  previewDeferred: false,
  conflictBaseline: null,
  patchArguments: null,
  patchSlice: null,
  previewVersion: '',
  ...overrides,
});

describe('git service', () => {
  beforeEach(() => invoke.mockReset());

  it('calls the backend commands with camelCase arguments', async () => {
    invoke.mockResolvedValue([]);
    await git.fetchBranchReview('C:\\r', 'main');
    expect(invoke).toHaveBeenLastCalledWith('git_branch_review', {
      root: 'C:\\r',
      baseBranch: 'main',
      summariesOnly: true,
      machineId: null,
    });
    await git.fetchPRDiff('C:\\r', 7, false, 'm1');
    expect(invoke).toHaveBeenLastCalledWith('git_pr_diff', {
      cwd: 'C:\\r',
      prNumber: 7,
      summariesOnly: false,
      machineId: 'm1',
    });
    await git.fetchReviewPage('C:\\r', file());
    expect(invoke).toHaveBeenLastCalledWith('git_review_page', {
      root: 'C:\\r',
      file: file(),
      cursor: null,
      machineId: null,
    });
  });

  it('builds the next-page cursor from a page', () => {
    const slice = { snapshot: 's.patch', start: 0, end: 90 };
    const page = {
      slice,
      hunks: [{ header: '@@ -1 +1 @@', lines: [] }],
      nextOffset: 40,
      oldLine: 3,
      newLine: 4,
      header: '@@ -1 +1 @@',
      hasMore: true,
      isBinary: false,
      hasLongLines: false,
    };
    expect(git.continuation(page)).toEqual({
      slice,
      nextOffset: 40,
      oldLine: 3,
      newLine: 4,
      header: '@@ -1 +1 @@',
      isBinary: false,
    });
  });

  it('reports gh missing or signed out as the unavailable PR state', async () => {
    invoke.mockRejectedValueOnce({ kind: 'review', message: 'gh not found' });
    expect(await git.loadPullRequests('C:\\r', null)).toEqual({
      ghAvailable: false,
      pullRequests: [],
      error: 'gh not found',
    });
    invoke.mockResolvedValueOnce([]);
    expect(await git.loadPullRequests('C:\\r', 'feature')).toEqual({
      ghAvailable: true,
      pullRequests: [],
      error: null,
    });
    expect(invoke).toHaveBeenLastCalledWith('git_pr_list', {
      cwd: 'C:\\r',
      branch: 'feature',
      machineId: null,
    });
  });

  it('counts unique local paths and collapses large or untracked files', () => {
    const review = {
      staged: [file()],
      unstaged: [file(), file({ filename: 'b.txt' })],
      untracked: [file({ filename: 'c.txt', statusCode: '??' })],
      error: null,
      stagedError: null,
      unstagedError: null,
      untrackedError: null,
    };
    expect(git.uniquePathCount(review)).toBe(3);
    expect(git.shouldCollapse(file({ statusCode: '??' }))).toBe(true);
    expect(git.shouldCollapse(file({ added: 150, removed: 51 }))).toBe(true);
    expect(git.shouldCollapse(file())).toBe(false);
  });

  it('extracts messages from any rejection', () => {
    expect(git.gitErrorMessage({ kind: 'tooLarge', message: 'File is too large' })).toBe(
      'File is too large',
    );
    expect(git.gitErrorMessage(new Error('boom'))).toBe('boom');
    expect(git.gitErrorMessage('plain')).toBe('plain');
  });
});
