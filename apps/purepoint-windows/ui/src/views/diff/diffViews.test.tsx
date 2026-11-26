// WIN-8.3: the review views (ReviewChangesView, DiffListView and its cards, PRRowView,
// InlineCodeView) and their pure halves (diffTextRendering, ReviewPageWindow), with scripted
// git calls (no backend).
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type {
  DiffLine,
  FileDiff,
  Hunk,
  PullRequestInfo,
  ReviewPage,
  ReviewPageCursor,
} from '../../services/git.ts';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => new Promise(() => undefined)) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));

// jsdom has no layout.
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView = () => undefined;
// The diff list's scroll view is 800 × 4000 px, so the virtualizer draws every row of the
// small diffs below.
const isScrollView = (element: Element) => element.classList.contains('diff-list-scroll');
const realRect = Element.prototype.getBoundingClientRect.bind(Element.prototype);
Element.prototype.getBoundingClientRect = function (this: Element) {
  return isScrollView(this) ? new DOMRect(0, 0, 800, 4000) : realRect.call(this);
};
Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
  configurable: true,
  get(this: HTMLElement) {
    return isScrollView(this) ? 800 : 0;
  },
});
Object.defineProperty(HTMLElement.prototype, 'clientHeight', {
  configurable: true,
  get(this: HTMLElement) {
    return isScrollView(this) ? 4000 : 0;
  },
});

const { DiffListView } = await import('./DiffListView.tsx');
const { ReviewChangesView } = await import('./ReviewChangesView.tsx');
const { PRRowView, labelColor } = await import('./PRRowView.tsx');
const { InlineCodeView } = await import('./InlineCodeView.tsx');
const { diffBodyRows, inlineCodeRows, inlineGutter } = await import('./diffTextRendering.ts');
const { ReviewPageWindow, MAX_WINDOW_LINES } = await import('./reviewPageWindow.ts');
const { DiffState } = await import('../../state/diffState.ts');
type DiffGitService = import('../../state/diffState.ts').DiffGitService;

function line(
  n: number,
  type: DiffLine['type'] = 'addition',
  content = `line ${String(n)}`,
): DiffLine {
  return {
    type,
    content,
    oldLineNo: type === 'addition' ? null : n,
    newLineNo: type === 'deletion' ? null : n,
  };
}

function file(filename: string, overrides: Partial<FileDiff> = {}): FileDiff {
  return {
    filename,
    statusCode: 'M',
    added: 2,
    removed: 1,
    hunks: [],
    oldFilename: null,
    isBinary: false,
    previewError: null,
    previewDeferred: false,
    conflictBaseline: null,
    patchArguments: ['diff', '--', filename],
    patchSlice: null,
    previewVersion: 'v1',
    ...overrides,
  };
}

function page(
  first: number,
  count: number,
  hasMore: boolean,
  header = '@@ -0,0 +1 @@',
): ReviewPage {
  return {
    slice: { snapshot: 'snap', start: 0, end: 100 },
    hunks: [{ header, lines: Array.from({ length: count }, (_, i) => line(first + i)) }],
    nextOffset: first + count,
    oldLine: 0,
    newLine: first + count,
    header,
    hasMore,
    isBinary: false,
    hasLongLines: false,
  };
}

/** A loader serving `pages` in order (by the cursor's offset). */
function pager(pages: ReviewPage[]) {
  return vi.fn((_file: FileDiff, after: ReviewPageCursor | null) => {
    const index =
      after === null ? 0 : pages.findIndex((p) => p.nextOffset === after.nextOffset) + 1;
    const next = pages[index];
    return next === undefined ? Promise.reject(new Error('no page')) : Promise.resolve(next);
  });
}

/** The code of every drawn diff line (highlighting splits it into tokens). */
const codeTexts = () => [...document.querySelectorAll('.diff-code')].map((el) => el.textContent);

async function expectCode(text: string) {
  await waitFor(() => {
    expect(codeTexts()).toContain(text);
  });
}

describe('diffTextRendering', () => {
  const hunks: Hunk[] = [
    { header: '@@ -1 +1 @@', lines: [line(1, 'context'), line(2, 'deletion')] },
    { header: '@@ -9 +9 @@', lines: [line(9)] },
  ];

  it('draws a separator between hunks', () => {
    expect(diffBodyRows(hunks).map((row) => row.kind)).toEqual([
      'line',
      'line',
      'separator',
      'line',
    ]);
  });

  it('puts a ⋯ row between inline hunks and numbers and marks the gutter', () => {
    const rows = inlineCodeRows(hunks);
    expect(rows.map((row) => row.text)).toEqual(['line 1', 'line 2', '⋯', 'line 9']);
    expect(inlineGutter(rows[1]?.line ?? null)).toEqual({ number: '2', marker: '−' });
    expect(inlineGutter(rows[3]?.line ?? null)).toEqual({ number: '9', marker: '+' });
    expect(inlineGutter(null)).toEqual({ number: '', marker: '' });
  });
});

describe('ReviewPageWindow', () => {
  it('appends consecutive pages and reloads earlier ones from checkpoints', () => {
    const window = new ReviewPageWindow();
    expect(window.cursor(0)).toBeNull();
    expect(() => window.cursor(2)).toThrow('Load the preceding page first.');
    window.store(page(1, 200, true), 0);
    window.store(page(201, 200, true), 1);
    expect(window.pages.length).toBe(2);
    expect(window.retainedLineCount).toBe(400);
    expect(window.cursor(2)?.nextOffset).toBe(401);
    window.releaseBodies();
    expect(window.isEmpty).toBe(true);
    expect(window.cursor(1)?.nextOffset).toBe(201);
  });

  it('replaces the window past its bound', () => {
    const window = new ReviewPageWindow();
    const pages = MAX_WINDOW_LINES / 200;
    for (let i = 0; i < pages; i += 1) window.store(page(i * 200 + 1, 200, true), i);
    expect(window.canExtend).toBe(false);
    window.store(page(pages * 200 + 1, 200, false), pages);
    expect(window.first).toBe(pages);
    expect(window.pages.length).toBe(1);
    expect(window.hasMore).toBe(false);
  });
});

describe('DiffListView', () => {
  it('lists cards, loads their first page and reads on as the end comes into view', async () => {
    const load = pager([page(1, 3, true), page(4, 2, false)]);
    render(
      <DiffListView
        diff={{ files: [file('src/app.ts'), file('README.md', { statusCode: 'A', removed: 0 })] }}
        isLoading={false}
        emptyMessage="none"
        error={null}
        loadPage={load}
      />,
    );
    expect(screen.getByText('2 changed files')).toBeInTheDocument();
    const header = screen.getByRole('button', { name: 'Collapse diff for src/app.ts' });
    expect(within(header).getByText('src/')).toBeInTheDocument();
    expect(within(header).getByText('app.ts')).toBeInTheDocument();
    expect(within(header).getByText('Modified')).toBeInTheDocument();
    expect(within(header).getByText('+2')).toBeInTheDocument();
    expect(within(header).getByText('-1')).toBeInTheDocument();
    expect(screen.getByText('Added')).toBeInTheDocument();
    await expectCode('line 3');
    // The pager is on screen, so the next page joins the card without a click (read on).
    await expectCode('line 5');
    expect(load).toHaveBeenCalledWith(
      expect.objectContaining({ filename: 'src/app.ts' }),
      expect.objectContaining({ nextOffset: 4 }),
    );
    expect(screen.getAllByText('Pages 1–2').length).toBe(2);
  });

  it('collapses a card, and shows notices for binary, renamed and conflicted files', async () => {
    render(
      <DiffListView
        diff={{
          files: [
            file('a.bin', { isBinary: true, patchArguments: null }),
            file('b.ts', { oldFilename: 'old/b.ts', patchArguments: null }),
            file('c.ts', {
              statusCode: 'U',
              conflictBaseline: 'ours (index stage 2)',
              patchArguments: null,
            }),
            file('big.ts', { added: 500, patchArguments: null }),
          ],
        }}
        isLoading={false}
        emptyMessage="none"
        error={null}
      />,
    );
    expect(screen.getByText('Binary file changed')).toBeInTheDocument();
    expect(screen.getByText('Renamed from old/b.ts')).toBeInTheDocument();
    expect(
      screen.getByText('Unresolved merge conflict · ours (index stage 2)'),
    ).toBeInTheDocument();
    expect(screen.getByText('Conflict')).toBeInTheDocument();
    // Large files start collapsed.
    expect(screen.getByRole('button', { name: 'Expand diff for big.ts' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Collapse diff for a.bin' }));
    await waitFor(() => {
      expect(screen.queryByText('Binary file changed')).toBeNull();
    });
  });

  it('shows the full hunks of a file that came with them, highlighted', async () => {
    const hunks: Hunk[] = [
      {
        header: '@@ -1 +1 @@',
        lines: [line(1, 'deletion', 'const a = 1;'), line(1, 'addition', 'const a = 2;')],
      },
    ];
    render(
      <DiffListView
        diff={{ files: [file('x.ts', { hunks, patchArguments: null })] }}
        isLoading={false}
        emptyMessage="none"
        error={null}
      />,
    );
    await waitFor(() => {
      expect(document.querySelectorAll('.diff-line .pp-tok-keyword').length).toBe(2);
    });
    expect(document.querySelector('.diff-line.deletion')).not.toBeNull();
    expect(document.querySelector('.diff-line.addition')).not.toBeNull();
  });

  it('finds and reveals a file from the navigator', () => {
    const scroll = vi.spyOn(window, 'requestAnimationFrame').mockImplementation((callback) => {
      callback(0);
      return 0;
    });
    render(
      <DiffListView
        diff={null}
        isLoading={false}
        emptyMessage="none"
        error={null}
        groups={[
          { title: 'Staged', files: [file('src/one.ts', { added: 400, patchArguments: null })] },
          { title: 'Unstaged', files: [file('docs/two.md', { patchArguments: null })] },
        ]}
      />,
    );
    expect(screen.getByText('Staged')).toBeInTheDocument();
    expect(screen.getByText('Unstaged')).toBeInTheDocument();
    const navigator = screen.getByRole('navigation', { name: 'Changed files' });
    expect(within(navigator).getByText('Files')).toBeInTheDocument();
    fireEvent.change(screen.getByPlaceholderText('Find changed file'), {
      target: { value: 'ONE' },
    });
    expect(within(navigator).queryByText('two.md')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show Staged diff for src/one.ts' }));
    expect(
      screen.getByRole('button', { name: 'Collapse diff for src/one.ts' }),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Hide changed files' }));
    expect(screen.queryByRole('navigation', { name: 'Changed files' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show changed files' }));
    scroll.mockRestore();
  });

  it('shows its loading, empty and error states', () => {
    const retry = vi.fn();
    const { rerender } = render(
      <DiffListView diff={null} isLoading emptyMessage="Working tree is clean" error={null} />,
    );
    expect(screen.getByRole('status', { name: 'Loading' })).toBeInTheDocument();
    rerender(
      <DiffListView
        diff={{ files: [] }}
        isLoading={false}
        emptyMessage="Working tree is clean"
        error={null}
      />,
    );
    expect(screen.getByText('Working tree is clean')).toBeInTheDocument();
    rerender(
      <DiffListView
        diff={{ files: [] }}
        isLoading={false}
        emptyMessage="x"
        error="fatal: bad revision"
        onRetry={retry}
      />,
    );
    expect(screen.getByText('fatal: bad revision')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(retry).toHaveBeenCalled();
  });

  it('reloads a card whose file changed on disk (a new preview version)', async () => {
    let version = 1;
    const load = vi.fn(() =>
      Promise.resolve({
        ...page(1, 1, false),
        hunks: [{ header: '@@', lines: [line(1, 'addition', `version ${String(version)}`)] }],
      }),
    );
    const { rerender } = render(
      <DiffListView
        diff={{ files: [file('a.md')] }}
        isLoading={false}
        emptyMessage=""
        error={null}
        loadPage={load}
      />,
    );
    await expectCode('version 1');
    version = 2;
    rerender(
      <DiffListView
        diff={{ files: [file('a.md', { previewVersion: 'v2' })] }}
        isLoading={false}
        emptyMessage=""
        error={null}
        loadPage={load}
      />,
    );
    await expectCode('version 2');
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('retries a page that failed to load', async () => {
    let fail = true;
    const load = vi.fn(() =>
      fail ? Promise.reject(new Error('snapshot gone')) : Promise.resolve(page(1, 2, false)),
    );
    render(
      <DiffListView
        diff={{ files: [file('a.ts')] }}
        isLoading={false}
        emptyMessage=""
        error={null}
        loadPage={load}
      />,
    );
    expect(await screen.findByText('snapshot gone')).toBeInTheDocument();
    fail = false;
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    await expectCode('line 2');
  });
});

describe('PRRowView', () => {
  const pr: PullRequestInfo = {
    number: 42,
    title: 'Port the diff viewer',
    url: 'https://github.com/o/r/pull/42',
    state: 'OPEN',
    headRefName: 'pu/win-8.3',
    baseRefName: 'main',
    author: { login: 'octo' },
    labels: [
      { name: 'windows', color: '0e8a16' },
      { name: 'odd', color: 'nope' },
    ],
    reviewDecision: 'APPROVED',
    additions: 10,
    deletions: 0,
    changedFiles: 3,
    isDraft: true,
    createdAt: '',
    updatedAt: '',
  };

  it('shows the number, title, draft, review state, author, branch, stats and labels', () => {
    render(<PRRowView pr={pr} />);
    for (const text of [
      '#42',
      'Port the diff viewer',
      'Draft',
      'octo',
      'pu/win-8.3',
      '+10',
      '3 files',
    ]) {
      expect(screen.getByText(text)).toBeInTheDocument();
    }
    expect(screen.queryByText('-0')).toBeNull();
    expect(screen.getByRole('img', { name: 'Approved' })).toBeInTheDocument();
    expect(screen.getByText('windows')).toHaveStyle({ background: '#0e8a16' });
    expect(labelColor('#ABCDEF')).toBe('#abcdef');
    expect(labelColor('nope')).toBe('gray');
  });
});

describe('InlineCodeView', () => {
  it('draws the gutter and the code, ⋯ between hunks', async () => {
    render(
      <InlineCodeView
        filename="a.ts"
        hunks={[
          { header: '', lines: [line(3, 'context', 'let x = 1;')] },
          { header: '', lines: [line(7, 'deletion', 'old();')] },
        ]}
      />,
    );
    expect(screen.getByText('3')).toBeInTheDocument();
    expect(screen.getByText('−')).toBeInTheDocument();
    expect(screen.getByText('⋯')).toBeInTheDocument();
    await waitFor(() => {
      expect(document.querySelector('.inline-code .pp-tok-keyword')?.textContent).toBe('let');
    });
  });
});

describe('ReviewChangesView', () => {
  function git(overrides: Partial<DiffGitService> = {}): DiffGitService {
    return {
      identity: 'local',
      isLocal: true,
      fetchBranchReview: () =>
        Promise.resolve({
          comparisonBase: 'main',
          availableBases: ['main', 'develop'],
          files: [],
          commits: [{ sha: 'abcdef1234567', subject: 'Add things', author: 'Ann', date: 'today' }],
          error: null,
        }),
      fetchLocalReview: () =>
        Promise.resolve({
          staged: [],
          unstaged: [file('src/edit.ts', { patchArguments: null })],
          untracked: [],
          error: null,
          stagedError: null,
          unstagedError: null,
          untrackedError: null,
        }),
      fetchCommitDiff: () => Promise.resolve([file('src/commit.ts', { patchArguments: null })]),
      fetchPRList: () => Promise.resolve([]),
      fetchPRDiff: () => Promise.resolve({ files: [] }),
      untrackedPreview: (f) => Promise.resolve(f),
      fetchReviewPage: () => Promise.reject(new Error('unused')),
      ...overrides,
    };
  }

  const observation = {
    watch: () => () => undefined,
    poll: () => () => undefined,
    isAppActive: () => true,
    now: () => 0,
  };

  function setup(overrides: Partial<DiffGitService> = {}) {
    const state = new DiffState({ git: git(overrides), observation });
    state.loadForWorktree({ path: 'C:\\wt', branch: 'pu/x', baseBranch: null });
    render(<ReviewChangesView state={state} />);
    return state;
  }

  it('shows the four tabs, the base menu and the branch empty state', async () => {
    const state = setup();
    for (const tab of ['Branch Changes', 'Uncommitted', 'Commits', 'PR Diffs']) {
      expect(screen.getByRole('tab', { name: tab })).toBeInTheDocument();
    }
    expect(await screen.findByText('No changes since main')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: /vs main/ }));
    const setBase = vi.spyOn(state, 'setComparisonBase');
    fireEvent.click(screen.getByRole('menuitem', { name: 'develop' }));
    expect(setBase).toHaveBeenCalledWith('develop');
  });

  it('groups uncommitted changes and refreshes them live', async () => {
    const state = setup();
    fireEvent.click(screen.getByRole('tab', { name: 'Uncommitted' }));
    expect(await screen.findByText('Unstaged')).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: 'Collapse diff for src/edit.ts' }),
    ).toBeInTheDocument();
    // The working tree changes: DiffState's refresh replaces the list.
    act(() => {
      state.store.setState({ unstagedDiff: [file('src/new.ts', { patchArguments: null })] });
    });
    expect(
      screen.getByRole('button', { name: 'Collapse diff for src/new.ts' }),
    ).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Collapse diff for src/edit.ts' })).toBeNull();
  });

  it('lists commits and opens one', async () => {
    setup();
    fireEvent.click(screen.getByRole('tab', { name: 'Commits' }));
    fireEvent.click(await screen.findByText('Add things'));
    expect(
      await screen.findByRole('button', { name: 'Collapse diff for src/commit.ts' }),
    ).toBeInTheDocument();
    expect(screen.getByText('abcdef12')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Back to commits' }));
    expect(await screen.findByText('Ann · today')).toBeInTheDocument();
  });

  it('shows the PR states', async () => {
    setup({ fetchPRList: () => Promise.reject(new Error('gh: not signed in')) });
    fireEvent.click(screen.getByRole('tab', { name: 'PR Diffs' }));
    expect(await screen.findByText('gh: not signed in')).toBeInTheDocument();
  });

  it('lists pull requests', async () => {
    const pr = {
      number: 7,
      title: 'Seven',
      url: 'u',
      state: 'OPEN',
      headRefName: 'h',
      baseRefName: 'main',
      author: { login: 'a' },
      labels: [],
      additions: 1,
      deletions: 1,
      changedFiles: 1,
      isDraft: false,
      createdAt: '',
      updatedAt: 'now',
    };
    const state = setup({ fetchPRList: () => Promise.resolve([pr]) });
    await waitFor(() => {
      expect(state.store.getState().pullRequests.length).toBe(1);
    });
    fireEvent.click(screen.getByRole('tab', { name: 'PR Diffs' }));
    // Opening the tab selects the first PR, as on the Mac.
    expect(await screen.findByText('#7 Seven')).toBeInTheDocument();
    expect(await screen.findByText('No changes in PR')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Back to pull requests' }));
    expect(await screen.findByText('Seven')).toBeInTheDocument();
  });
});
