// WIN-8.2: the project, worktree and root checkout screens and the workspace files sidebar,
// over a project with scripted git calls (no backend).
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn(() => new Promise<never>(() => undefined));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));

// jsdom has no scrolling.
Element.prototype.scrollIntoView = () => undefined;

const { ProjectState } = await import('../../state/projectState.ts');
const { makeWorktreeModel } = await import('../../model/workspaceModel.ts');
const { ProjectDetailView } = await import('./ProjectDetailView.tsx');
const { WorktreeDetailView } = await import('./WorktreeDetailView.tsx');
const { WorkspaceFilesSidebar } = await import('./WorkspaceFilesSidebar.tsx');
const { WorkspaceFilesLayout } = await import('./WorkspaceFilesLayout.tsx');
const { WorkspaceFilesState } = await import('../../state/workspaceFilesState.ts');
const { DiffState } = await import('../../state/diffState.ts');
type FileDiff = import('../../services/git.ts').FileDiff;
type GitReviewSummary = import('../../services/git.ts').GitReviewSummary;
type DiffGitService = import('../../state/diffState.ts').DiffGitService;

const ROOT = 'C:\\repo\\demo';

function project(worktreeNames: string[] = ['alpha']) {
  const state = new ProjectState({
    projectRoot: ROOT,
    client: { send: () => Promise.resolve({ type: 'ok' }) },
  });
  state.store.setState({
    worktrees: worktreeNames.map((name) =>
      makeWorktreeModel({
        id: `wt-${name}`,
        name,
        path: `${ROOT}\\.pu\\worktrees\\${name}`,
        branch: `pu/${name}`,
        status: 'active',
        agents: [],
      }),
    ),
  });
  return state;
}

function file(filename: string, added = 1, statusCode = 'M'): FileDiff {
  return {
    filename,
    statusCode,
    added,
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
  };
}

function quietGit(): DiffGitService {
  return {
    identity: 'local',
    isLocal: true,
    fetchBranchReview: () =>
      Promise.resolve({
        comparisonBase: 'main',
        availableBases: [],
        files: [],
        commits: [],
        error: null,
      }),
    fetchLocalReview: () =>
      Promise.resolve({
        staged: [],
        unstaged: [],
        untracked: [],
        error: null,
        stagedError: null,
        unstagedError: null,
        untrackedError: null,
      }),
    fetchCommitDiff: () => Promise.resolve([]),
    fetchPRList: () => Promise.resolve([]),
    fetchPRDiff: () => Promise.resolve({ files: [] }),
    untrackedPreview: (f) => Promise.resolve(f),
    fetchReviewPage: () => Promise.reject(new Error('unused')),
  };
}

const quietObservation = {
  watch: () => () => undefined,
  poll: () => () => undefined,
  isAppActive: () => true,
  now: () => 0,
};

const makeDiffState = () => new DiffState({ git: quietGit(), observation: quietObservation });

afterEach(() => {
  invoke.mockClear();
});

describe('ProjectDetailView', () => {
  it('lists the root checkout and worktrees with their review summaries', async () => {
    const summaries: Record<string, GitReviewSummary> = {
      [ROOT]: { commitCount: 0, localFileCount: 2, error: null },
      [`${ROOT}\\.pu\\worktrees\\alpha`]: { commitCount: 3, localFileCount: 1, error: null },
      [`${ROOT}\\.pu\\worktrees\\beta`]: { commitCount: 0, localFileCount: 0, error: 'no base' },
    };
    const fetch = vi.fn((path: string) => Promise.resolve(summaries[path] as GitReviewSummary));
    render(
      <ProjectDetailView
        project={project(['alpha', 'beta'])}
        onSelectWorktree={() => undefined}
        reviewSummary={fetch}
        isAppActive={() => true}
      />,
    );
    expect(screen.getByText('demo', { selector: '.detail-title' })).toBeInTheDocument();
    expect(screen.getByText('2 worktrees')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Overview' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByRole('tab', { name: 'Channel' })).toBeInTheDocument();
    expect(screen.getByText('WORKTREES')).toBeInTheDocument();
    expect(screen.getAllByText('Reading changes…').length).toBe(3);
    expect(await screen.findByText('0 commits · 2 local files')).toBeInTheDocument();
    expect(await screen.findByText('3 commits · 1 local files')).toBeInTheDocument();
    expect(await screen.findByText('no base')).toBeInTheDocument();
    expect(fetch).toHaveBeenCalledWith(ROOT, null, null);
  });

  it('selects a worktree, opens the root checkout and comes back', async () => {
    const onSelect = vi.fn();
    render(
      <ProjectDetailView
        project={project()}
        onSelectWorktree={onSelect}
        reviewSummary={() => new Promise(() => undefined)}
      />,
    );
    fireEvent.click(screen.getByText('alpha'));
    expect(onSelect).toHaveBeenCalledWith('wt-alpha');
    fireEvent.click(screen.getByText('Root checkout'));
    expect(screen.getByRole('button', { name: 'Back to overview' })).toBeInTheDocument();
    expect(document.querySelector('[data-view="RootCheckoutDetailView"]')).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Back to overview' }));
    await waitFor(() => {
      expect(screen.getByText('WORKTREES')).toBeInTheDocument();
    });
  });

  it('shows the channel tab', () => {
    render(
      <ProjectDetailView
        project={project()}
        onSelectWorktree={() => undefined}
        reviewSummary={() => new Promise(() => undefined)}
      />,
    );
    fireEvent.click(screen.getByRole('tab', { name: 'Channel' }));
    expect(screen.getByText('ProjectChannelView')).toBeInTheDocument();
    expect(screen.queryByText('WORKTREES')).toBeNull();
  });
});

describe('WorktreeDetailView', () => {
  it('shows the header actions and toggles the file tree, changes and channel', () => {
    const p = project();
    const worktree = p.worktrees[0];
    if (worktree === undefined) throw new Error('no worktree');
    render(<WorktreeDetailView worktree={worktree} project={p} createDiffState={makeDiffState} />);
    expect(screen.getByText('pu/alpha')).toBeInTheDocument();
    for (const name of ['Show file tree', 'Files', 'Channel', 'Toggle changes view', 'Refresh']) {
      expect(screen.getByRole('button', { name })).toBeInTheDocument();
    }
    // Changes are shown first.
    expect(screen.queryByText('Select a file to edit')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Toggle changes view' }));
    expect(screen.getByText('Select a file to edit')).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Files' }));
    expect(screen.getByRole('button', { name: 'Hide file tree' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Show file tree' })).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Hide file tree' }));
    expect(screen.getByRole('button', { name: 'Show file tree' })).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Channel' }));
    expect(screen.getByText('ProjectChannelView')).toBeInTheDocument();
  });

  it('refreshes its review', () => {
    const p = project();
    const worktree = p.worktrees[0];
    if (worktree === undefined) throw new Error('no worktree');
    const diff = makeDiffState();
    const refresh = vi.spyOn(diff, 'refresh');
    render(<WorktreeDetailView worktree={worktree} project={p} createDiffState={() => diff} />);
    refresh.mockClear();
    fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
    expect(refresh).toHaveBeenCalledTimes(1);
  });
});

describe('WorkspaceFilesSidebar', () => {
  function filesState(changes: FileDiff[] | Error, diff?: FileDiff) {
    return new WorkspaceFilesState(ROOT, {
      git: {
        fetchWorkingTreeChanges: () =>
          changes instanceof Error ? Promise.reject(changes) : Promise.resolve(changes),
        fetchWorkingTreeFileDiff: (_root, f) => Promise.resolve(diff ?? f),
      },
    });
  }

  it('lists changes and expands one to its diff', async () => {
    const changed = file('src/app.ts', 2);
    const state = filesState([changed, file('notes.md', 0, '??')], {
      ...changed,
      hunks: [
        {
          header: '@@ -1 +1,2 @@',
          lines: [
            { type: 'context', content: 'const a = 1;', oldLineNo: 1, newLineNo: 1 },
            { type: 'addition', content: 'const b = 2;', oldLineNo: null, newLineNo: 2 },
          ],
        },
      ],
    });
    render(<WorkspaceFilesSidebar state={state} />);
    expect(screen.getByRole('tab', { name: 'Files' })).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: 'Point Guard' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Hide Workspace Sidebar' })).toBeInTheDocument();
    const header = await screen.findByRole('button', { name: 'src/app.ts, Expand diff' });
    expect(within(header).getByText('app.ts')).toBeInTheDocument();
    expect(within(header).getByText('+2')).toBeInTheDocument();
    expect(screen.getByText('New')).toBeInTheDocument();
    expect(screen.getByRole('tab', { name: /^Changes/ })).toHaveTextContent('Changes2');
    fireEvent.click(header);
    expect(await screen.findByText(/const b = 2;/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'src/app.ts, Collapse diff' }));
    expect(screen.queryByText(/const b = 2;/)).toBeNull();
  });

  it('shows its empty and error states', async () => {
    const { unmount } = render(<WorkspaceFilesSidebar state={filesState([])} />);
    expect(await screen.findByText('No changes')).toBeInTheDocument();
    unmount();
    render(<WorkspaceFilesSidebar state={filesState(new Error('not a git repository'))} />);
    expect(await screen.findByText('not a git repository')).toBeInTheDocument();
  });

  it('switches modes', async () => {
    const state = filesState([]);
    render(<WorkspaceFilesSidebar state={state} />);
    await screen.findByText('No changes');
    fireEvent.click(screen.getByRole('tab', { name: 'Point Guard' }));
    expect(screen.getByText('PiPointGuardChatView')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('tab', { name: 'Files' }));
    expect(screen.getByText('InlineFileTreeRow')).toBeInTheDocument();
  });

  it('is hidden and reopened from the layout', async () => {
    render(
      <WorkspaceFilesLayout rootPath={ROOT} machineId={null} createState={() => filesState([])}>
        <div>panes</div>
      </WorkspaceFilesLayout>,
    );
    await screen.findByText('No changes');
    act(() => {
      fireEvent.click(screen.getByRole('button', { name: 'Hide Workspace Sidebar' }));
    });
    expect(screen.queryByText('No changes')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show Workspace Sidebar' }));
    expect(await screen.findByText('No changes')).toBeInTheDocument();
  });
});
