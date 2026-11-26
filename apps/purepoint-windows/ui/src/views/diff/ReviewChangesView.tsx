// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/ReviewChangesView.swift (WIN-8.3).
//
// The review of a worktree or root checkout over its DiffState: Branch Changes (against a base
// the user can switch), Uncommitted (staged, unstaged, untracked), Commits (pick one to see its
// patch) and PR Diffs (pick an open pull request). Each list is a DiffListView paging over the
// backend's review pages; DiffState's watcher and polling refresh it as the working tree changes.
import { useMemo, useState } from 'react';
import { useStore } from 'zustand';
import { DIFF_TAB_TITLES, DIFF_TABS, type DiffFileGroup, type DiffTab } from '../../model/diff.ts';
import type {
  FileDiff,
  GitCommitInfo,
  PullRequestInfo,
  ReviewPageCursor,
} from '../../services/git.ts';
import { openExternal } from '../../services/opener.ts';
import {
  isInitiallyLoadingBranch,
  isInitiallyLoadingCommit,
  isInitiallyLoadingUnstaged,
  isPRDiffStale,
  type DiffState,
} from '../../state/diffState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { GHUnavailableView } from '../shell/DaemonErrorBanner.tsx';
import { DiffListView } from './DiffListView.tsx';
import { PRRowView } from './PRRowView.tsx';
import './diff.css';

// DiffState's setters (Swift's `didSet`s), kept out of the components.
function selectTab(state: DiffState, tab: DiffTab): void {
  state.activeTab = tab;
}

function clearSelectedPR(state: DiffState): void {
  state.selectedPR = null;
  state.prDiff = null;
}

function BaseMenu({ state }: { state: DiffState }) {
  const comparisonBase = useStore(state.store, (s) => s.comparisonBase);
  const availableBases = useStore(state.store, (s) => s.availableBases);
  const [open, setOpen] = useState(false);
  return (
    <div className="diff-base-menu">
      <button
        type="button"
        className="detail-button diff-base-button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          setOpen((shown) => !shown);
        }}
      >
        {`vs ${comparisonBase}`}
        <Icon name="chevron.down" size={10} />
      </button>
      {open && (
        <div
          className="diff-base-list"
          role="menu"
          aria-label="Comparison base"
          onPointerLeave={() => {
            setOpen(false);
          }}
        >
          {availableBases.map((base) => (
            <button
              key={base}
              type="button"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                state.setComparisonBase(base);
              }}
            >
              {base}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function Patches({
  state,
  files,
  loading,
  empty,
  error,
}: {
  state: DiffState;
  files: readonly FileDiff[];
  loading: boolean;
  empty: string;
  error: string | null;
}) {
  // Stable while the files are, so the list keeps its rows between renders.
  const diff = useMemo(
    () => (loading && files.length === 0 ? null : { files: [...files] }),
    [loading, files],
  );
  return (
    <DiffListView
      diff={diff}
      isLoading={loading}
      emptyMessage={empty}
      error={error}
      onRetry={() => {
        state.refresh();
      }}
      loadPage={(file: FileDiff, after: ReviewPageCursor | null) => state.loadPage(file, after)}
    />
  );
}

function UncommittedTab({ state }: { state: DiffState }) {
  const loading = useStore(state.store, isInitiallyLoadingUnstaged);
  const localError = useStore(state.store, (s) => s.localError);
  const staged = useStore(state.store, (s) => s.stagedDiff);
  const unstaged = useStore(state.store, (s) => s.unstagedDiff);
  const untracked = useStore(state.store, (s) => s.untrackedDiff);
  const groups = useMemo<DiffFileGroup[]>(
    () => [
      { title: 'Staged', files: staged },
      { title: 'Unstaged', files: unstaged },
      { title: 'Untracked', files: untracked },
    ],
    [staged, unstaged, untracked],
  );
  const diff = useMemo(() => (loading ? null : { files: [] }), [loading]);
  return (
    <DiffListView
      diff={diff}
      isLoading={loading}
      emptyMessage="Working tree is clean"
      error={localError}
      onRetry={() => {
        state.refresh();
      }}
      groups={groups}
      loadPage={(file, after) => state.loadPage(file, after)}
      onLoadPreview={(file) => {
        void state.loadUntrackedPreview(file);
      }}
    />
  );
}

function CommitsTab({ state }: { state: DiffState }) {
  const branchError = useStore(state.store, (s) => s.branchError);
  const commits = useStore(state.store, (s) => s.commits);
  const selectedCommit = useStore(state.store, (s) => s.selectedCommit);
  const commitDiff = useStore(state.store, (s) => s.commitDiff);
  const commitError = useStore(state.store, (s) => s.commitError);
  const loadingCommit = useStore(state.store, isInitiallyLoadingCommit);
  const loadingBranch = useStore(state.store, isInitiallyLoadingBranch);
  const comparisonBase = useStore(state.store, (s) => s.comparisonBase);

  if (branchError !== null) {
    return <Patches state={state} files={[]} loading={false} empty="" error={branchError} />;
  }
  if (selectedCommit !== null) {
    return (
      <div className="review-stack">
        <div className="review-subheader">
          <button
            type="button"
            className="detail-icon-button"
            aria-label="Back to commits"
            title="Back to commits"
            onClick={() => {
              state.store.setState({ selectedCommit: null });
            }}
          >
            <Icon name="chevron.left" size={13} />
          </button>
          <span className="review-subheader-title">{selectedCommit.subject}</span>
          <span className="detail-spacer" />
          <span className="review-sha">{selectedCommit.sha.slice(0, 8)}</span>
        </div>
        <Patches
          key={selectedCommit.sha}
          state={state}
          files={commitDiff}
          loading={loadingCommit}
          empty="This commit has no file changes"
          error={commitError}
        />
      </div>
    );
  }
  return (
    <div className="review-list">
      {commits.map((commit: GitCommitInfo) => (
        <button
          key={commit.sha}
          type="button"
          className="review-commit-row"
          onClick={() => {
            state.selectCommit(commit);
          }}
        >
          <Icon
            name="point.3.connected.trianglepath.dotted"
            size={14}
            className="review-row-icon"
          />
          <span className="review-commit-text">
            <span className="review-commit-subject">{commit.subject}</span>
            <span className="review-commit-meta">{`${commit.author} · ${commit.date}`}</span>
          </span>
          <span className="detail-spacer" />
          <span className="review-sha">{commit.sha.slice(0, 8)}</span>
        </button>
      ))}
      {commits.length === 0 && (
        <div className="review-empty">
          {loadingBranch ? 'Loading commits…' : `No commits beyond ${comparisonBase}`}
        </div>
      )}
    </div>
  );
}

function PRTab({ state }: { state: DiffState }) {
  const stale = useStore(state.store, isPRDiffStale);
  const isLoadingPRDiff = useStore(state.store, (s) => s.isLoadingPRDiff);
  const isLoadingPRs = useStore(state.store, (s) => s.isLoadingPRs);
  const prError = useStore(state.store, (s) => s.prError);
  const pullRequests = useStore(state.store, (s) => s.pullRequests);
  const prDiff = useStore(state.store, (s) => s.prDiff);
  const ghAvailable = useStore(state.store, (s) => s.ghAvailable);
  const selectedPR = useStore(state.store, (s) => s.selectedPR);

  let body;
  if (prError !== null && pullRequests.length === 0 && prDiff === null) {
    body = <Patches state={state} files={[]} loading={false} empty="" error={prError} />;
  } else if (!ghAvailable && pullRequests.length === 0 && prDiff === null) {
    body = <GHUnavailableView />;
  } else if (selectedPR !== null) {
    body = (
      <div className="review-stack">
        <div className="review-subheader">
          <button
            type="button"
            className="detail-icon-button"
            aria-label="Back to pull requests"
            title="Back to pull requests"
            onClick={() => {
              clearSelectedPR(state);
            }}
          >
            <Icon name="chevron.left" size={13} />
          </button>
          <span className="review-subheader-title">{`#${String(selectedPR.number)} ${selectedPR.title}`}</span>
          <span className="detail-spacer" />
          <button
            type="button"
            className="detail-icon-button"
            aria-label="Open pull request"
            title="Open pull request"
            onClick={() => {
              openExternal(selectedPR.url);
            }}
          >
            <Icon name="arrow.up.right.square" size={13} />
          </button>
        </div>
        <DiffListView
          key={`pr-${String(selectedPR.number)}`}
          diff={prDiff}
          isLoading={isLoadingPRDiff}
          emptyMessage="No changes in PR"
          error={prDiff === null ? prError : null}
          onRetry={() => {
            state.refresh();
          }}
          loadPage={(file, after) => state.loadPage(file, after)}
        />
      </div>
    );
  } else if (isLoadingPRs && pullRequests.length === 0) {
    body = (
      <div className="diff-list-state">
        <span className="diff-spinner" role="status" aria-label="Loading" />
      </div>
    );
  } else if (pullRequests.length === 0) {
    body = <div className="diff-list-state">No open pull requests</div>;
  } else {
    body = (
      <div className="review-list">
        {pullRequests.map((pr: PullRequestInfo) => (
          <button
            key={pr.number}
            type="button"
            className="review-pr-row"
            onClick={() => {
              state.selectPR(pr);
            }}
          >
            <PRRowView pr={pr} />
          </button>
        ))}
      </div>
    );
  }

  return (
    <div className="review-stack">
      {stale && (
        <div className="review-banner">
          <span>
            {isLoadingPRDiff
              ? 'Showing the previous PR snapshot. Refreshing changes…'
              : 'Showing the previous PR snapshot. Refresh to load current changes.'}
          </span>
          <span className="detail-spacer" />
          <button
            type="button"
            className="detail-button detail-bordered-button"
            onClick={() => {
              if (selectedPR !== null) state.selectPR(selectedPR);
            }}
          >
            Retry
          </button>
        </div>
      )}
      {prError !== null && (pullRequests.length > 0 || prDiff !== null) && (
        <div className="review-banner warning">
          <Icon name="exclamationmark.triangle" size={13} />
          <span className="review-banner-text">{prError}</span>
          <span className="detail-spacer" />
          <button
            type="button"
            className="detail-button detail-bordered-button"
            onClick={() => {
              state.refresh();
            }}
          >
            Retry
          </button>
        </div>
      )}
      {body}
    </div>
  );
}

function BranchTab({ state }: { state: DiffState }) {
  const files = useStore(state.store, (s) => s.branchDiff);
  const loading = useStore(state.store, isInitiallyLoadingBranch);
  const comparisonBase = useStore(state.store, (s) => s.comparisonBase);
  const error = useStore(state.store, (s) => s.branchError);
  return (
    <Patches
      state={state}
      files={files}
      loading={loading}
      empty={`No changes since ${comparisonBase}`}
      error={error}
    />
  );
}

export function ReviewChangesView({ state }: { state: DiffState }) {
  const activeTab = useStore(state.store, (s) => s.activeTab);
  const reviewIdentity = useStore(state.store, (s) => s.reviewIdentity);
  const comparisonBase = useStore(state.store, (s) => s.comparisonBase);

  return (
    <div className="review-changes" data-view="ReviewChangesView">
      <div className="review-header">
        <div className="segmented review-tabs" role="tablist" aria-label="Review">
          {DIFF_TABS.map((tab) => (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={activeTab === tab}
              onClick={() => {
                selectTab(state, tab);
              }}
            >
              {DIFF_TAB_TITLES[tab]}
            </button>
          ))}
        </div>
        <span className="detail-spacer" />
        {(activeTab === 'branch' || activeTab === 'commits') && <BaseMenu state={state} />}
      </div>
      {/* Swift's `.id(...)`: a new review, tab or base starts the content over. */}
      <div
        className="review-content"
        key={`${String(reviewIdentity)}/${activeTab}/${comparisonBase}`}
      >
        {activeTab === 'branch' ? (
          <BranchTab state={state} />
        ) : activeTab === 'unstaged' ? (
          <UncommittedTab state={state} />
        ) : activeTab === 'commits' ? (
          <CommitsTab state={state} />
        ) : (
          <PRTab state={state} />
        )}
      </div>
    </div>
  );
}
