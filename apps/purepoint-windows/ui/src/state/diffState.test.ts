// The DiffState assertions of GitReviewTests.swift (`stateChecks`, `emptyRefreshChecks`, the end of
// `lazyReviewChecks` and the in-flight PR refresh of `reviewRegressionChecks`), deferred by WIN-8.1
// to WIN-6.0. Swift drives DiffState over real repositories and a fake `gh`; the git side is
// covered by `src-tauri/src/git/tests/`, so here a scripted `DiffGitService` stands in for it and
// the assertions are DiffState's: loading flags, tab switching, selection and stale responses.
// Titles are the Swift `require` messages.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const { DiffState } = await import('./diffState');
type DiffGitService = import('./diffState').DiffGitService;
type DiffObservation = import('./diffState').DiffObservation;
type FileDiff = import('../services/git').FileDiff;
type GitCommitInfo = import('../services/git').GitCommitInfo;
type PullRequestInfo = import('../services/git').PullRequestInfo;
type DiffData = import('../services/git').DiffData;

function file(filename: string, body: string[] = [], statusCode = 'M'): FileDiff {
  return {
    filename,
    statusCode,
    added: body.length,
    removed: 0,
    hunks:
      body.length === 0
        ? []
        : [
            {
              header: '@@ -0,0 +1 @@',
              lines: body.map((content, i) => ({
                type: 'addition' as const,
                content,
                oldLineNo: null,
                newLineNo: i + 1,
              })),
            },
          ],
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

const summary = (f: FileDiff, summariesOnly: boolean): FileDiff =>
  summariesOnly ? { ...f, hunks: [] } : f;

function commit(sha: string, subject = sha): GitCommitInfo {
  return { sha, subject, author: 'Test', date: 'today' };
}

function pr(number: number, updatedAt = 'today'): PullRequestInfo {
  return {
    number,
    title: 'One',
    url: `https://example.test/${String(number)}`,
    state: 'OPEN',
    headRefName: 'feature',
    baseRefName: 'main',
    author: { login: 'test' },
    labels: [],
    additions: 1,
    deletions: 0,
    changedFiles: 1,
    isDraft: false,
    createdAt: 'today',
    updatedAt,
  };
}

interface Repo {
  inferredBase: string;
  bases: Set<string>;
  commits: GitCommitInfo[];
  files: FileDiff[];
  unstaged: FileDiff[];
}

interface Gate {
  promise: Promise<void>;
  open: () => void;
}

function gate(): Gate {
  let open = () => {};
  const promise = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { promise, open };
}

/** A scripted GitService: repositories by path, a commit store, and `gh`'s PRs. */
class FakeGit implements DiffGitService {
  readonly identity = 'fake';
  readonly isLocal = true;
  readonly repos = new Map<string, Repo>();
  readonly commitFiles = new Map<string, FileDiff[]>();
  readonly unavailableCommits = new Set<string>();
  /** Per-sha holds, released by the test, to order responses. */
  readonly commitGates = new Map<string, Gate>();
  prs: PullRequestInfo[] = [];
  prLine = 'remote first';
  prListFails = false;
  prDiffFails = false;
  prDiffGate: Gate | null = null;
  prDiffStarted = false;

  repo(path: string, repo: Partial<Repo> = {}): Repo {
    const full: Repo = {
      inferredBase: 'main',
      bases: new Set(['main']),
      commits: [],
      files: [],
      unstaged: [],
      ...repo,
    };
    this.repos.set(path, full);
    return full;
  }

  fetchBranchReview(path: string, baseBranch: string | null, summariesOnly: boolean) {
    const repo = this.repos.get(path);
    const base = baseBranch ?? repo?.inferredBase ?? '';
    if (repo === undefined || !repo.bases.has(base)) {
      return Promise.resolve({
        comparisonBase: base,
        availableBases: [],
        files: [],
        commits: [],
        error: `Unknown base ${base}`,
      });
    }
    return Promise.resolve({
      comparisonBase: base,
      availableBases: [...repo.bases],
      files: repo.files.map((f) => summary(f, summariesOnly)),
      commits: [...repo.commits],
      error: null,
    });
  }

  fetchLocalReview(path: string, summariesOnly: boolean) {
    const repo = this.repos.get(path);
    return Promise.resolve({
      staged: [],
      unstaged: (repo?.unstaged ?? []).map((f) => summary(f, summariesOnly)),
      untracked: [],
      error: null,
      stagedError: null,
      unstagedError: null,
      untrackedError: null,
    });
  }

  async fetchCommitDiff(_path: string, sha: string, summariesOnly: boolean) {
    await this.commitGates.get(sha)?.promise;
    if (this.unavailableCommits.has(sha)) throw new Error(`fatal: bad object ${sha}`);
    return (this.commitFiles.get(sha) ?? []).map((f) => summary(f, summariesOnly));
  }

  fetchPRList() {
    if (this.prListFails) return Promise.reject(new Error('remote-unavailable'));
    return Promise.resolve([...this.prs]);
  }

  async fetchPRDiff(_cwd: string, _number: number, summariesOnly: boolean): Promise<DiffData> {
    this.prDiffStarted = true;
    await this.prDiffGate?.promise;
    if (this.prDiffFails || this.prListFails) throw new Error('temporary-diff-error');
    return { files: [summary(file('remote.txt', [this.prLine], 'A'), summariesOnly)] };
  }

  untrackedPreview(f: FileDiff) {
    return Promise.resolve(f);
  }

  fetchReviewPage(f: FileDiff) {
    const body = f.filename === 'remote.txt' ? [this.prLine] : ['page'];
    return Promise.resolve({
      slice: { snapshot: 's', start: 0, end: 1 },
      hunks: file(f.filename, body).hunks,
      nextOffset: 1,
      oldLine: 0,
      newLine: 1,
      header: '',
      hasMore: false,
      isBinary: false,
      hasLongLines: false,
    });
  }
}

const tick = () =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });

/** Swift `settle`: wait until no load is in flight. */
async function settle(state: InstanceType<typeof DiffState>): Promise<void> {
  for (let i = 0; i < 200; i += 1) {
    const s = state.store.getState();
    if (
      !s.isLoadingBranch &&
      !s.isLoadingUnstaged &&
      !s.isLoadingPRs &&
      !s.isLoadingPRDiff &&
      !s.isLoadingCommit
    ) {
      return;
    }
    await tick();
  }
  throw new Error('state refresh timed out');
}

const worktree = { path: '/repo', branch: 'feature', baseBranch: 'main' };

function featureRepo(git: FakeGit) {
  git.repo('/repo', {
    commits: [commit('c2', 'second'), commit('c1', 'first')],
    files: [file('first.txt', ['first']), file('second.txt', ['second'])],
  });
  git.commitFiles.set('c2', [file('second.txt', ['second'])]);
  git.commitFiles.set('c1', [file('first.txt', ['first'])]);
  git.prs = [pr(1)];
}

describe('GitReviewTests.stateChecks (DiffState)', () => {
  it('default branch and recorded base; remote initial patch', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    await settle(state);
    const s = state.store.getState();
    expect(s.activeTab).toBe('branch');
    expect(s.commits).toHaveLength(2);
    expect(s.comparisonBase).toBe('main');
    expect(s.prDiff?.files[0]?.added).toBe(1);
    expect(s.prError).toBeNull();
  });

  it('opening a worktree does not download a background PR patch; PR tab loads metadata with deferred bodies', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const lazy = new DiffState({ git, observeChanges: false });
    lazy.loadForWorktree(worktree);
    await settle(lazy);
    expect(lazy.selectedPR).toBeNull();
    expect(lazy.prDiff).toBeNull();
    expect(lazy.store.getState().pullRequests).not.toHaveLength(0);
    expect(git.prDiffStarted).toBe(false);

    lazy.activeTab = 'prDiffs';
    await settle(lazy);
    expect(lazy.prDiff?.files[0]?.hunks).toEqual([]);
    expect(lazy.prDiff?.files[0]?.added).toBe(1);
    // PR body loads through overview page routing.
    const page = await lazy.loadPage(lazy.prDiff?.files[0] ?? file('x'));
    expect(page.hunks[0]?.lines.at(-1)?.content).toBe('remote first');
  });

  it('failed selected commit retains an actionable error; local refresh retries it; selected commit loads metadata only', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const lazy = new DiffState({ git, observeChanges: false });
    lazy.loadForWorktree(worktree);
    await settle(lazy);
    lazy.activeTab = 'commits';
    git.unavailableCommits.add('c2');
    lazy.selectCommit(lazy.store.getState().commits[0] ?? commit('c2'));
    await settle(lazy);
    expect(lazy.commitError).not.toBeNull();
    expect(lazy.store.getState().commitDiff).toEqual([]);
    git.unavailableCommits.delete('c2');
    lazy.refresh();
    await settle(lazy);
    expect(lazy.commitError).toBeNull();
    const diff = lazy.store.getState().commitDiff;
    expect(diff[0]?.filename).toBe('second.txt');
    expect(diff[0]?.hunks).toEqual([]);
  });

  it('lazy review tab switching preserves explicit PR list navigation', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const lazy = new DiffState({ git, observeChanges: false });
    lazy.loadForWorktree(worktree);
    await settle(lazy);
    lazy.activeTab = 'prDiffs';
    await settle(lazy);
    lazy.selectedPR = null;
    lazy.prDiff = null;
    lazy.activeTab = 'branch';
    lazy.activeTab = 'prDiffs';
    await settle(lazy);
    expect(lazy.selectedPR).toBeNull();
    expect(lazy.prDiff).toBeNull();
    lazy.stopWatching();
  });

  it('latest commit selection wins', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    await settle(state);
    const [second, first] = state.store.getState().commits;
    const slow = gate();
    git.commitGates.set('c1', slow);
    state.selectCommit(first ?? commit('c1'));
    state.selectCommit(second ?? commit('c2'));
    await settle(state);
    slow.open(); // the superseded response lands last
    await tick();
    expect(state.store.getState().commitDiff[0]?.filename).toBe('second.txt');
  });

  it('explicit refresh updates selected PR; PR error retains evidence', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    await settle(state);
    git.prLine = 'remote refreshed';
    state.refresh();
    await settle(state);
    expect(state.prDiff?.files[0]?.hunks[0]?.lines.at(-1)?.content).toBe('remote refreshed');
    git.prListFails = true;
    state.refresh();
    await settle(state);
    expect(state.store.getState().prError).toContain('remote-unavailable');
    expect(state.store.getState().ghAvailable).toBe(false);
    expect(state.prDiff).not.toBeNull();
  });

  it('malformed PR patch retains evidence', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    await settle(state);
    git.prDiffFails = true;
    state.refresh();
    await settle(state);
    expect(state.store.getState().prError).not.toBeNull();
    expect(state.prDiff?.files).toHaveLength(1);
  });

  it('refresh and tab switching must preserve explicit PR list navigation', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    await settle(state);
    state.selectedPR = null;
    state.prDiff = null;
    state.refresh();
    await settle(state);
    expect(state.selectedPR).toBeNull();
    expect(state.prDiff).toBeNull();
    expect(state.store.getState().pullRequests).not.toHaveLength(0);
    state.activeTab = 'branch';
    state.activeTab = 'prDiffs';
    await settle(state);
    expect(state.selectedPR).toBeNull();
    expect(state.prDiff).toBeNull();
  });

  it('explicit base failure invalidates a pending PR diff; base error recovers', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    await settle(state);
    git.prListFails = true;
    state.selectPR(state.store.getState().pullRequests[0] ?? pr(1));
    state.setComparisonBase('missing-base');
    await settle(state);
    let s = state.store.getState();
    expect(s.branchError).not.toBeNull();
    expect(s.comparisonBase).toBe('missing-base');
    expect(s.branchDiff).toEqual([]);
    expect(s.isLoadingPRDiff).toBe(false);
    git.prListFails = false;
    state.setComparisonBase('main');
    await settle(state);
    s = state.store.getState();
    expect(s.branchError).toBeNull();
    expect(s.branchDiff).toHaveLength(2);
  });

  it('stale workspace responses rejected', async () => {
    const git = new FakeGit();
    featureRepo(git);
    git.repo('/other');
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    state.loadForProject('/other');
    await settle(state);
    const s = state.store.getState();
    expect(s.commits).toEqual([]);
    expect(s.branchDiff).toEqual([]);
    expect(s.comparisonBase).toBe('main');
  });

  it('stop invalidates in-flight loads', async () => {
    const git = new FakeGit();
    featureRepo(git);
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForWorktree(worktree);
    state.stopWatching();
    await tick();
    await tick();
    const s = state.store.getState();
    expect(s.isLoadingBranch).toBe(false);
    expect(s.isLoadingPRs).toBe(false);
    expect(s.commits).toEqual([]);
  });
});

describe('GitReviewTests.emptyRefreshChecks (DiffState)', () => {
  it('keeps empty evidence visible while refreshing, and resets initial loading on new selections', async () => {
    const git = new FakeGit();
    const repo = git.repo('/clean');
    const state = new DiffState({ git, observeChanges: false, summariesOnly: false });
    state.loadForProject('/clean');
    // new project shows initial loading
    expect(state.isInitiallyLoadingBranch).toBe(true);
    expect(state.isInitiallyLoadingUnstaged).toBe(true);
    await settle(state);
    for (let i = 0; i < 3; i += 1) {
      state.refresh();
      // background refresh still fetches Git evidence
      expect(state.store.getState().isLoadingBranch).toBe(true);
      expect(state.store.getState().isLoadingUnstaged).toBe(true);
      // empty branch, commits and clean working tree stay visible during refresh
      expect(state.isInitiallyLoadingBranch).toBe(false);
      expect(state.isInitiallyLoadingUnstaged).toBe(false);
      await settle(state);
      expect(state.store.getState().branchDiff).toEqual([]);
      expect(state.store.getState().commits).toEqual([]);
      expect(state.localFileCount).toBe(0);
    }
    // quiet refresh still publishes file edits
    repo.unstaged = [file('root.txt', ['edited'])];
    state.refresh();
    await settle(state);
    expect(state.store.getState().unstagedDiff).toHaveLength(1);

    repo.commits = [commit('empty')];
    repo.bases.add('feature');
    git.commitFiles.set('empty', []);
    state.loadForWorktree({ path: '/clean', branch: 'feature', baseBranch: 'main' });
    // worktree selection resets initial loading
    expect(state.isInitiallyLoadingBranch).toBe(true);
    expect(state.isInitiallyLoadingUnstaged).toBe(true);
    await settle(state);
    state.selectCommit(state.store.getState().commits[0] ?? commit('empty'));
    // first commit selection loads its patch
    expect(state.isInitiallyLoadingCommit).toBe(true);
    await settle(state);
    state.refresh();
    await settle(state);
    state.selectCommit(state.store.getState().commits[0] ?? commit('empty'));
    // empty selected commit remains visible during reload
    expect(state.store.getState().isLoadingCommit).toBe(true);
    expect(state.isInitiallyLoadingCommit).toBe(false);
    expect(state.store.getState().commitDiff).toEqual([]);
    await settle(state);

    repo.commits = [];
    state.setComparisonBase('feature');
    // new base reloads branch evidence without hiding local evidence
    expect(state.isInitiallyLoadingBranch).toBe(true);
    expect(state.isInitiallyLoadingUnstaged).toBe(false);
    await settle(state);
    // new comparison replaces commit list
    expect(state.store.getState().commits).toEqual([]);

    const unavailable = commit('0'.repeat(40), 'Unavailable');
    git.unavailableCommits.add(unavailable.sha);
    for (let i = 0; i < 2; i += 1) {
      state.commitError = null;
      state.selectCommit(unavailable);
      // failed commit fetch retries show loading rather than false empty evidence
      expect(state.isInitiallyLoadingCommit).toBe(true);
      expect(state.store.getState().commitDiff).toEqual([]);
      await settle(state);
      // unavailable commit exposes fetch failure
      expect(state.commitError).not.toBeNull();
      expect(state.store.getState().commitDiff).toEqual([]);
    }
    state.stopWatching();
  });
});

describe('GitReviewTests.lazyReviewChecks (DiffState)', () => {
  it('overview defaults to metadata-only loading', async () => {
    const git = new FakeGit();
    git.repo('/large', { files: [file('large.txt', ['line 0', 'line 1'])] });
    const state = new DiffState({ git, observeChanges: false });
    state.loadForProject('/large');
    await settle(state);
    const branch = state.store.getState().branchDiff;
    expect(branch).toHaveLength(1);
    expect(branch.every((f) => f.hunks.length === 0)).toBe(true);
    state.stopWatching();
  });
});

describe('GitReviewTests.reviewRegressionChecks (DiffState in-flight PR refresh)', () => {
  it('keeps an interrupted PR refresh visibly stale and resumes it on return', async () => {
    const git = new FakeGit();
    git.repo('/repo');
    git.prs = [pr(1, 'v1')];
    const state = new DiffState({ git, observeChanges: false });
    state.loadForProject('/repo');
    await settle(state);
    state.activeTab = 'prDiffs';
    await settle(state);
    // initial PR snapshot is current
    expect(state.prDiff).not.toBeNull();
    expect(state.isPRDiffStale).toBe(false);

    git.prs = [pr(1, 'v2')];
    git.prLine = 'updated remote';
    const delay = gate();
    git.prDiffGate = delay;
    git.prDiffStarted = false;
    state.refresh();
    const started = () => git.prDiffStarted;
    for (let i = 0; i < 100 && !started(); i += 1) await tick();
    // updated PR download is in flight
    expect(state.store.getState().isLoadingPRDiff).toBe(true);
    expect(state.selectedPR?.updatedAt).toBe('v2');
    state.activeTab = 'branch';
    // interrupted PR refresh retains visibly stale evidence
    expect(state.prDiff).not.toBeNull();
    expect(state.isPRDiffStale).toBe(true);
    expect(state.store.getState().isLoadingPRDiff).toBe(false);
    git.prDiffGate = null;
    state.activeTab = 'prDiffs';
    // returning to stale PR resumes its interrupted refresh
    expect(state.store.getState().isLoadingPRDiff).toBe(true);
    delay.open();
    await settle(state);
    // successful replacement marks the loaded PR revision current
    expect(state.isPRDiffStale).toBe(false);
    const refreshed = await state.loadPage(state.prDiff?.files[0] ?? file('x'));
    // reentry replaces stale PR patch content
    expect(refreshed.hunks[0]?.lines.at(-1)?.content).toBe('updated remote');

    state.selectedPR = null;
    state.prDiff = null;
    git.prDiffFails = true;
    state.selectPR(state.store.getState().pullRequests[0] ?? pr(1));
    await settle(state);
    // failed selected PR has an actionable load error
    expect(state.store.getState().prError).not.toBeNull();
    expect(state.prDiff).toBeNull();
    git.prDiffFails = false;
    state.refreshPRs();
    await settle(state);
    // ordinary PR polling retries an unloaded selected PR even when updatedAt is unchanged
    expect(state.store.getState().prError).toBeNull();
    expect(state.prDiff).not.toBeNull();
    state.stopWatching();
  });
});

describe('DiffState change observation', () => {
  function observation() {
    const watched: string[] = [];
    let onChange = () => {};
    let poll = () => {};
    let interval = 0;
    let stops = 0;
    let now = 0;
    let active = true;
    const fake: DiffObservation = {
      watch: (path, change) => {
        watched.push(path);
        onChange = change;
        return () => {
          stops += 1;
        };
      },
      poll: (tickFn, ms) => {
        poll = tickFn;
        interval = ms;
        return () => {
          stops += 1;
        };
      },
      isAppActive: () => active,
      now: () => now,
    };
    return {
      fake,
      watched,
      change: () => {
        onChange();
      },
      poll: () => {
        poll();
      },
      interval: () => interval,
      stops: () => stops,
      setNow: (value: number) => {
        now = value;
      },
      setActive: (value: boolean) => {
        active = value;
      },
    };
  }

  it('watches local worktrees, polls every 2 s while active and lists PRs every 30 s', async () => {
    const git = new FakeGit();
    const repo = git.repo('/repo');
    git.prs = [pr(1)];
    const obs = observation();
    const listPRs = vi.spyOn(git, 'fetchPRList');
    const state = new DiffState({ git, observation: obs.fake });
    state.loadForProject('/repo');
    await settle(state);
    expect(obs.watched).toEqual(['/repo']);
    expect(obs.interval()).toBe(2000);
    expect(listPRs).toHaveBeenCalledTimes(1);

    repo.unstaged = [file('a.txt', ['a'])];
    obs.change();
    await settle(state);
    expect(state.store.getState().unstagedDiff).toHaveLength(1);

    obs.setNow(10_000);
    obs.poll();
    await settle(state);
    expect(listPRs).toHaveBeenCalledTimes(1);
    obs.setNow(30_000);
    obs.setActive(false);
    obs.poll();
    expect(state.store.getState().isLoadingBranch).toBe(false);
    obs.setActive(true);
    obs.poll();
    await settle(state);
    expect(listPRs).toHaveBeenCalledTimes(2);

    state.stopWatching();
    expect(obs.stops()).toBe(2);
  });

  it('only polls a remote host, every 5 s', () => {
    const git = new FakeGit();
    git.repo('/remote');
    const remote: DiffGitService = Object.assign(Object.create(git) as FakeGit, {
      isLocal: false,
      identity: 'machine:x',
    });
    const obs = observation();
    const state = new DiffState({ git: remote, observation: obs.fake });
    state.loadForProject('/remote');
    expect(obs.watched).toEqual([]);
    expect(obs.interval()).toBe(5000);
    state.stopWatching();
  });
});
