// Port of apps/purepoint-macos/purepoint-macos/State/DiffState.swift (WIN-6.0).
//
// The review panel's state for one worktree or project: branch changes against a base, the
// working tree, commits and pull requests, refreshed by a git-metadata watcher and polling.
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in a Zustand store (`diff.store`) the
// views subscribe to; the class mirrors the Swift methods and keeps the task bookkeeping private.
// Swift cancels a superseded `Task`; here each async load holds a token and drops its result when
// the token is no longer current, which is what Swift's `guard !Task.isCancelled` does.

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { DiffTab } from '../model/diff';
import type { WorktreeModel } from '../model/workspaceModel';
import {
  fetchBranchReview,
  fetchCommitDiff,
  fetchLocalReview,
  fetchPRDiff,
  fetchPRList,
  fetchReviewPage,
  fetchUntrackedPreview,
  gitErrorMessage,
  type DiffData,
  type FileDiff,
  type GitBranchReview,
  type GitCommitInfo,
  type GitLocalReview,
  type PullRequestInfo,
  type ReviewPage,
  type ReviewPageCursor,
} from '../services/git';
import { onWorktreesChanged, unwatch, watchWorktree } from '../services/watchers';

// MARK: - Git seam

/** The GitService calls DiffState makes, routed through one project host. */
export interface DiffGitService {
  /** Swift `host.identity`: services for the same host are interchangeable. */
  readonly identity: string;
  /** Local hosts are watched; remote ones only polled (every 5 s rather than 2 s). */
  readonly isLocal: boolean;
  /** Never rejects: failures come back in `error` (Swift returns them the same way). */
  fetchBranchReview(
    path: string,
    baseBranch: string | null,
    summariesOnly: boolean,
  ): Promise<GitBranchReview>;
  fetchLocalReview(path: string, summariesOnly: boolean): Promise<GitLocalReview>;
  fetchCommitDiff(path: string, sha: string, summariesOnly: boolean): Promise<FileDiff[]>;
  fetchPRList(cwd: string, branch: string | null): Promise<PullRequestInfo[]>;
  fetchPRDiff(cwd: string, prNumber: number, summariesOnly: boolean): Promise<DiffData>;
  /** Never rejects: a failed preview is the file with `previewError` set. */
  untrackedPreview(file: FileDiff, path: string): Promise<FileDiff>;
  fetchReviewPage(
    file: FileDiff,
    path: string,
    cursor: ReviewPageCursor | null,
  ): Promise<ReviewPage>;
}

function failedBranchReview(base: string | null, error: unknown): GitBranchReview {
  return {
    comparisonBase: base ?? '',
    availableBases: [],
    files: [],
    commits: [],
    error: gitErrorMessage(error),
  };
}

function failedLocalReview(error: unknown): GitLocalReview {
  const message = gitErrorMessage(error);
  return {
    staged: [],
    unstaged: [],
    untracked: [],
    error: message,
    stagedError: message,
    unstagedError: message,
    untrackedError: message,
  };
}

/** The git backend (`services/git.ts`) for this PC (`machineId` null) or a machine. */
export function gitServiceFor(machineId: string | null = null): DiffGitService {
  return {
    identity: machineId === null ? 'local' : `machine:${machineId}`,
    isLocal: machineId === null,
    fetchBranchReview: (path, base, summariesOnly) =>
      fetchBranchReview(path, base, summariesOnly, machineId).catch((error: unknown) =>
        failedBranchReview(base, error),
      ),
    fetchLocalReview: (path, summariesOnly) =>
      fetchLocalReview(path, summariesOnly, machineId).catch(failedLocalReview),
    fetchCommitDiff: (path, sha, summariesOnly) =>
      fetchCommitDiff(path, sha, summariesOnly, machineId),
    fetchPRList: (cwd, branch) => fetchPRList(cwd, branch, machineId),
    fetchPRDiff: (cwd, prNumber, summariesOnly) =>
      fetchPRDiff(cwd, prNumber, summariesOnly, machineId),
    untrackedPreview: (file, path) =>
      fetchUntrackedPreview(path, file.filename, machineId).catch((error: unknown) => ({
        ...file,
        previewError: gitErrorMessage(error),
      })),
    fetchReviewPage: (file, path, cursor) => fetchReviewPage(path, file, cursor, machineId),
  };
}

// MARK: - Change observation seam

export interface DiffObservation {
  /** Watch a worktree's git metadata; returns the stop function. */
  watch(path: string, onChange: () => void): () => void;
  /** Run `tick` every `intervalMs`; returns the stop function. */
  poll(tick: () => void, intervalMs: number): () => void;
  /** Swift polls only while `NSApplication.shared.isActive`. */
  isAppActive(): boolean;
  now(): number;
}

/** The backend worktree watcher (WIN-5.6), timers and window focus. */
export const DEFAULT_DIFF_OBSERVATION: DiffObservation = {
  watch(path, onChange) {
    let stopped = false;
    let id: number | null = null;
    let unlisten: (() => void) | null = null;
    void onWorktreesChanged((event) => {
      if (!stopped && event.worktreePath === path) onChange();
    }).then((fn) => {
      if (stopped) fn();
      else unlisten = fn;
    });
    void watchWorktree(path).then(
      (watchId) => {
        if (stopped) void unwatch(watchId);
        else id = watchId;
      },
      () => {},
    );
    return () => {
      stopped = true;
      unlisten?.();
      if (id !== null) void unwatch(id);
    };
  },
  poll(tick, intervalMs) {
    const handle = setInterval(tick, intervalMs);
    return () => {
      clearInterval(handle);
    };
  },
  isAppActive: () => typeof document === 'undefined' || document.hasFocus(),
  now: () => Date.now(),
};

// MARK: - Observable state

export interface DiffStateFields {
  readonly activeTab: DiffTab;
  readonly branchDiff: readonly FileDiff[];
  readonly stagedDiff: readonly FileDiff[];
  readonly unstagedDiff: readonly FileDiff[];
  readonly untrackedDiff: readonly FileDiff[];
  readonly commits: readonly GitCommitInfo[];
  readonly selectedCommit: GitCommitInfo | null;
  readonly commitDiff: readonly FileDiff[];
  readonly comparisonBase: string;
  readonly availableBases: readonly string[];
  readonly branchError: string | null;
  readonly commitError: string | null;
  readonly localError: string | null;
  readonly prError: string | null;
  readonly isLoadingBranch: boolean;
  readonly isLoadingUnstaged: boolean;
  readonly isLoadingCommit: boolean;
  /** Empty results are loaded evidence too; polling must not replace them with a spinner. */
  readonly hasLoadedBranch: boolean;
  readonly hasLoadedLocal: boolean;
  readonly hasLoadedCommit: boolean;
  readonly pullRequests: readonly PullRequestInfo[];
  readonly selectedPR: PullRequestInfo | null;
  readonly prDiff: DiffData | null;
  readonly loadedPRVersion: string | null;
  readonly isLoadingPRs: boolean;
  readonly isLoadingPRDiff: boolean;
  readonly ghAvailable: boolean;
  /** Swift `reviewIdentity`: changes whenever loaded evidence is invalidated. */
  readonly reviewIdentity: number;
}

const INITIAL: DiffStateFields = {
  activeTab: 'branch',
  branchDiff: [],
  stagedDiff: [],
  unstagedDiff: [],
  untrackedDiff: [],
  commits: [],
  selectedCommit: null,
  commitDiff: [],
  comparisonBase: '',
  availableBases: [],
  branchError: null,
  commitError: null,
  localError: null,
  prError: null,
  isLoadingBranch: false,
  isLoadingUnstaged: false,
  isLoadingCommit: false,
  hasLoadedBranch: false,
  hasLoadedLocal: false,
  hasLoadedCommit: false,
  pullRequests: [],
  selectedPR: null,
  prDiff: null,
  loadedPRVersion: null,
  isLoadingPRs: false,
  isLoadingPRDiff: false,
  ghAvailable: true,
  reviewIdentity: 0,
};

// Derived values, for views that select from the store.

export const isInitiallyLoadingBranch = (s: DiffStateFields): boolean =>
  s.isLoadingBranch && !s.hasLoadedBranch;
export const isInitiallyLoadingUnstaged = (s: DiffStateFields): boolean =>
  s.isLoadingUnstaged && !s.hasLoadedLocal;
export const isInitiallyLoadingCommit = (s: DiffStateFields): boolean =>
  s.isLoadingCommit && !s.hasLoadedCommit;
export const isPRDiffStale = (s: DiffStateFields): boolean =>
  s.prDiff !== null && s.loadedPRVersion !== (s.selectedPR?.updatedAt ?? null);
export const diffError = (s: DiffStateFields): string | null =>
  s.branchError ?? s.commitError ?? s.localError ?? s.prError;
export const localFileCount = (s: DiffStateFields): number =>
  new Set([...s.stagedDiff, ...s.unstagedDiff, ...s.untrackedDiff].map((f) => f.filename)).size;

/** Swift's `CancellationError` from `loadPage` when the review changed meanwhile. */
export class ReviewCancelledError extends Error {
  constructor() {
    super('The review changed while the page was loading.');
    this.name = 'ReviewCancelledError';
  }
}

export interface DiffStateOptions {
  readonly git?: DiffGitService;
  /** False: callers drive refresh themselves (no watcher, no polling). */
  readonly observeChanges?: boolean;
  /** True: overviews load metadata only and patch bodies arrive page by page. */
  readonly summariesOnly?: boolean;
  readonly observation?: DiffObservation;
}

const PR_REFRESH_INTERVAL_MS = 30_000;

type Token = object;

// MARK: - DiffState

export class DiffState {
  readonly store: StoreApi<DiffStateFields>;

  private git: DiffGitService;
  private readonly observeChanges: boolean;
  private readonly summariesOnly: boolean;
  private readonly observation: DiffObservation;

  private stopWatcher: (() => void) | null = null;
  private stopPolling: (() => void) | null = null;
  private path: string | null = null;
  private branch: string | null = null;
  private requestedBase: string | null = null;
  private generation = 0;
  private localTask: Token | null = null;
  private prTask: Token | null = null;
  private commitTask: Token | null = null;
  private prDiffTask: Token | null = null;
  private initialPRSelectionMade = false;
  private hasSelectedPR = false;
  private lastPRRefresh = Number.NEGATIVE_INFINITY;

  constructor(options: DiffStateOptions = {}) {
    this.git = options.git ?? gitServiceFor(null);
    this.observeChanges = options.observeChanges ?? true;
    this.summariesOnly = options.summariesOnly ?? true;
    this.observation = options.observation ?? DEFAULT_DIFF_OBSERVATION;
    this.store = createStore<DiffStateFields>()(() => INITIAL);
  }

  // MARK: Observable accessors

  private get s(): DiffStateFields {
    return this.store.getState();
  }

  private set(partial: Partial<DiffStateFields>): void {
    this.store.setState(partial);
  }

  get activeTab(): DiffTab {
    return this.s.activeTab;
  }

  /** Swift's `activeTab` with its `didSet`. */
  set activeTab(tab: DiffTab) {
    this.set({ activeTab: tab });
    if (this.summariesOnly && tab !== 'prDiffs') {
      this.prDiffTask = null;
      this.set({ isLoadingPRDiff: false });
    }
    const s = this.s;
    if (tab === 'prDiffs' && (s.prDiff === null || isPRDiffStale(s))) {
      if (s.selectedPR !== null) this.selectPR(s.selectedPR);
      else if (!this.hasSelectedPR && s.pullRequests[0] !== undefined) {
        this.selectPR(s.pullRequests[0]);
      }
    }
  }

  get selectedPR(): PullRequestInfo | null {
    return this.s.selectedPR;
  }

  set selectedPR(pr: PullRequestInfo | null) {
    this.set({ selectedPR: pr });
  }

  get prDiff(): DiffData | null {
    return this.s.prDiff;
  }

  set prDiff(diff: DiffData | null) {
    this.set({ prDiff: diff });
  }

  get commitError(): string | null {
    return this.s.commitError;
  }

  set commitError(message: string | null) {
    this.set({ commitError: message });
  }

  get isInitiallyLoadingBranch(): boolean {
    return isInitiallyLoadingBranch(this.s);
  }

  get isInitiallyLoadingUnstaged(): boolean {
    return isInitiallyLoadingUnstaged(this.s);
  }

  get isInitiallyLoadingCommit(): boolean {
    return isInitiallyLoadingCommit(this.s);
  }

  get isPRDiffStale(): boolean {
    return isPRDiffStale(this.s);
  }

  get error(): string | null {
    return diffError(this.s);
  }

  get localFileCount(): number {
    return localFileCount(this.s);
  }

  get reviewIdentity(): number {
    return this.generation;
  }

  // MARK: Loading

  /** Route git through a project's host (Swift `use(host:)`). */
  use(git: DiffGitService): void {
    if (git.identity === this.git.identity) return;
    this.stopWatching();
    this.git = git;
  }

  loadForWorktree(worktree: Pick<WorktreeModel, 'path' | 'branch' | 'baseBranch'>): void {
    this.load(worktree.path, worktree.branch, worktree.baseBranch);
  }

  loadForProject(root: string): void {
    this.load(root, null, null);
  }

  private load(path: string, branch: string | null, base: string | null): void {
    this.stopWatching();
    this.path = path;
    this.branch = branch;
    this.requestedBase = base;
    this.initialPRSelectionMade = false;
    this.hasSelectedPR = false;
    this.activeTab = 'branch';
    this.set({
      comparisonBase: base ?? '',
      availableBases: [],
      branchDiff: [],
      stagedDiff: [],
      unstagedDiff: [],
      untrackedDiff: [],
      commits: [],
      selectedCommit: null,
      commitDiff: [],
      pullRequests: [],
      selectedPR: null,
      prDiff: null,
      loadedPRVersion: null,
      branchError: null,
      commitError: null,
      localError: null,
      prError: null,
      hasLoadedBranch: false,
      hasLoadedLocal: false,
      hasLoadedCommit: false,
    });
    this.refresh();
    // Explicitly driven callers can own refresh timing without background races.
    if (!this.observeChanges) return;
    // Metadata watching is local-only; remote worktrees rely on the polling below.
    if (this.git.isLocal) {
      this.stopWatcher = this.observation.watch(path, () => {
        this.refreshLocal();
      });
    }
    this.stopPolling = this.observation.poll(
      () => {
        // Recursive file changes need polling. Hidden/background app work is bounded.
        if (!this.observation.isAppActive()) return;
        this.refreshLocal();
        if (this.observation.now() - this.lastPRRefresh >= PR_REFRESH_INTERVAL_MS) {
          this.refreshPRs();
        }
      },
      this.git.isLocal ? 2000 : 5000,
    );
  }

  setComparisonBase(base: string): void {
    this.requestedBase = base;
    this.generation += 1;
    this.localTask = null;
    this.commitTask = null;
    this.prTask = null;
    this.prDiffTask = null;
    // Old branch and commit evidence belongs to a different comparison.
    this.set({
      comparisonBase: base,
      reviewIdentity: this.generation,
      isLoadingPRDiff: false,
      isLoadingPRs: false,
      branchDiff: [],
      commits: [],
      selectedCommit: null,
      commitDiff: [],
      isLoadingCommit: false,
      hasLoadedBranch: false,
      hasLoadedCommit: false,
    });
    this.refresh();
  }

  selectCommit(commit: GitCommitInfo): void {
    const path = this.path;
    if (path === null) return;
    if (this.s.selectedCommit?.sha !== commit.sha) {
      this.set({ commitDiff: [], hasLoadedCommit: false });
    }
    const task: Token = {};
    this.commitTask = task;
    this.set({ selectedCommit: commit, isLoadingCommit: true, commitError: null });
    const token = this.generation;
    const current = () =>
      this.commitTask === task &&
      this.generation === token &&
      this.s.selectedCommit?.sha === commit.sha;
    void (async () => {
      try {
        const files = await this.git.fetchCommitDiff(path, commit.sha, this.summariesOnly);
        if (!current()) return;
        this.set({ commitDiff: files, hasLoadedCommit: true });
      } catch (error) {
        if (!current()) return;
        this.set({ commitError: gitErrorMessage(error) });
      }
      this.set({ isLoadingCommit: false });
    })();
  }

  selectPR(pr: PullRequestInfo): void {
    const path = this.path;
    if (path === null) return;
    const changed = this.s.selectedPR?.number !== pr.number;
    this.hasSelectedPR = true;
    const task: Token = {};
    this.prDiffTask = task;
    this.set({ selectedPR: pr, isLoadingPRDiff: true });
    if (changed) this.set({ prDiff: null, loadedPRVersion: null });
    const token = this.generation;
    const current = () =>
      this.prDiffTask === task &&
      this.generation === token &&
      this.s.selectedPR?.number === pr.number;
    void (async () => {
      try {
        const diff = await this.git.fetchPRDiff(path, pr.number, this.summariesOnly);
        if (!current()) return;
        this.set({ prDiff: diff, loadedPRVersion: pr.updatedAt, prError: null });
      } catch (error) {
        if (!current()) return;
        this.set({ prError: gitErrorMessage(error) });
      }
      this.set({ isLoadingPRDiff: false });
    })();
  }

  async loadUntrackedPreview(file: FileDiff): Promise<void> {
    const path = this.path;
    if (path === null) return;
    const token = this.generation;
    const preview = await this.git.untrackedPreview(file, path);
    if (this.generation !== token) return;
    const untracked = this.s.untrackedDiff;
    const index = untracked.findIndex((candidate) => candidate.filename === file.filename);
    if (index === -1) return;
    this.set({ untrackedDiff: untracked.map((item, i) => (i === index ? preview : item)) });
  }

  async loadPage(file: FileDiff, after: ReviewPageCursor | null = null): Promise<ReviewPage> {
    const path = this.path;
    if (path === null) throw new ReviewCancelledError();
    const token = this.generation;
    const loaded = await this.git.fetchReviewPage(file, path, after);
    if (token !== this.generation) throw new ReviewCancelledError();
    return loaded;
  }

  refresh(): void {
    this.refreshLocal();
    this.refreshPRs(true);
  }

  private refreshLocal(): void {
    const path = this.path;
    if (path === null || this.localTask !== null) return;
    const token = this.generation;
    const base = this.requestedBase;
    const task: Token = {};
    this.localTask = task;
    this.set({ isLoadingBranch: true, isLoadingUnstaged: true });
    const current = () => this.localTask === task && this.generation === token;
    void (async () => {
      const branchReview = await this.git.fetchBranchReview(path, base, this.summariesOnly);
      if (!current()) return;
      const local = await this.git.fetchLocalReview(path, this.summariesOnly);
      if (!current()) return;
      this.set({
        comparisonBase: branchReview.comparisonBase,
        availableBases: branchReview.availableBases,
        branchError: branchReview.error,
        localError: local.error,
      });
      if (branchReview.error === null) {
        this.set({ branchDiff: branchReview.files, commits: branchReview.commits });
        const selected = this.s.selectedCommit;
        if (selected !== null) {
          if (branchReview.commits.some((commit) => commit.sha === selected.sha)) {
            const s = this.s;
            if (!this.summariesOnly || (!s.hasLoadedCommit && !s.isLoadingCommit)) {
              this.selectCommit(selected);
            }
          } else {
            this.commitTask = null;
            this.set({ selectedCommit: null, commitDiff: [], isLoadingCommit: false });
          }
        }
      }
      if (local.stagedError === null) this.set({ stagedDiff: local.staged });
      if (local.unstagedError === null) this.set({ unstagedDiff: local.unstaged });
      if (local.untrackedError === null) this.set({ untrackedDiff: local.untracked });
      this.localTask = null;
      this.set({
        hasLoadedBranch: true,
        hasLoadedLocal: true,
        isLoadingBranch: false,
        isLoadingUnstaged: false,
      });
    })();
  }

  refreshPRs(forceDiff = false): void {
    const path = this.path;
    if (path === null || this.prTask !== null) return;
    const token = this.generation;
    const branch = this.branch;
    const task: Token = {};
    this.prTask = task;
    this.lastPRRefresh = this.observation.now();
    this.set({ isLoadingPRs: true });
    const current = () => this.prTask === task && this.generation === token;
    void (async () => {
      try {
        const prs = await this.git.fetchPRList(path, branch);
        if (!current()) return;
        const shouldSelectInitialPR = !this.initialPRSelectionMade;
        this.initialPRSelectionMade = true;
        this.set({ ghAvailable: true, pullRequests: prs, prError: null });
        const s = this.s;
        const selected = s.selectedPR;
        const updated =
          selected === null ? undefined : prs.find((pr) => pr.number === selected.number);
        const showingPRs = !this.summariesOnly || s.activeTab === 'prDiffs';
        if (selected !== null && updated !== undefined) {
          if (showingPRs) {
            if (forceDiff || !this.summariesOnly || s.loadedPRVersion !== updated.updatedAt) {
              this.selectPR(updated);
            }
          } else if (selected.updatedAt !== updated.updatedAt) {
            this.set({ selectedPR: updated });
          }
        } else if (
          shouldSelectInitialPR &&
          !this.hasSelectedPR &&
          prs[0] !== undefined &&
          showingPRs
        ) {
          this.selectPR(prs[0]);
        } else if (prs.length === 0 || selected !== null) {
          this.prDiffTask = null;
          this.set({ selectedPR: null, prDiff: null, isLoadingPRDiff: false });
        }
      } catch (error) {
        if (!current()) return;
        // Keep previously fetched evidence and expose auth/network/decoding failures.
        this.set({ prError: gitErrorMessage(error), ghAvailable: false });
      }
      this.prTask = null;
      this.set({ isLoadingPRs: false });
    })();
  }

  stopWatching(): void {
    this.generation += 1;
    this.path = null;
    this.stopWatcher?.();
    this.stopWatcher = null;
    this.stopPolling?.();
    this.stopPolling = null;
    this.localTask = null;
    this.prTask = null;
    this.commitTask = null;
    this.prDiffTask = null;
    this.set({
      reviewIdentity: this.generation,
      isLoadingBranch: false,
      isLoadingUnstaged: false,
      isLoadingCommit: false,
      isLoadingPRs: false,
      isLoadingPRDiff: false,
    });
  }
}
