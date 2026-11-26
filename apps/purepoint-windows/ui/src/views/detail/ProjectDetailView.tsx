// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/ProjectDetailView.swift (WIN-8.2).
//
// A project's overview: the root checkout and each worktree with its review summary (commits
// against the base, locally changed files), refreshed every 5 s while the window is active, and
// the project channel. Choosing a worktree selects it in the sidebar; choosing the root checkout
// opens RootCheckoutDetailView in place.
//
// Differences from Swift: the Channel tab is ProjectChannelView (WIN-8.10), a placeholder until
// it lands. A remote project's summaries go through its machine (`machineId`), as Swift's
// `project.git` does.
import { useEffect, useState } from 'react';
import { useStore } from 'zustand';
import type { AgentModel } from '../../model/workspaceModel.ts';
import { agentDisplayName } from '../../model/workspaceModel.ts';
import {
  gitErrorMessage,
  reviewSummary as fetchReviewSummary,
  type GitReviewSummary,
} from '../../services/git.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { ScreenPlaceholder } from './placeholders.tsx';
import { RootCheckoutDetailView } from './RootCheckoutDetailView.tsx';
import './detail.css';

const PROJECT_TABS = ['overview', 'channel'] as const;
type ProjectTab = (typeof PROJECT_TABS)[number];
const PROJECT_TAB_TITLES: Record<ProjectTab, string> = { overview: 'Overview', channel: 'Channel' };

const SUMMARY_INTERVAL_MS = 5000;

export type ReviewSummaryFetcher = (
  root: string,
  baseBranch: string | null,
  machineId: string | null,
) => Promise<GitReviewSummary>;

export interface ProjectDetailViewProps {
  project: ProjectState;
  /** Selects a worktree in the sidebar (the shell's selection). */
  onSelectWorktree: (worktreeId: string) => void;
  /** The git summary call; tests script it. */
  reviewSummary?: ReviewSummaryFetcher;
  /** Swift polls only while `NSApplication.shared.isActive`. */
  isAppActive?: () => boolean;
}

const defaultFetch: ReviewSummaryFetcher = (root, base, machineId) =>
  fetchReviewSummary(root, base, machineId).catch((error: unknown): GitReviewSummary => ({
    commitCount: 0,
    localFileCount: 0,
    error: gitErrorMessage(error),
  }));

const defaultIsActive = () => document.hasFocus();

function delay(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, ms);
    signal.addEventListener('abort', () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

/** The summary loop: the root, then each worktree, then sleep 5 s. */
function useReviewSummaries(
  project: ProjectState,
  active: boolean,
  fetchSummary: ReviewSummaryFetcher,
  isAppActive: () => boolean,
): Record<string, GitReviewSummary> {
  const [summaries, setSummaries] = useState<Record<string, GitReviewSummary>>({});
  const [summaryProject, setSummaryProject] = useState(project.projectId);
  // Swift `.onChange(of: project.projectId)`: start over for another project.
  if (summaryProject !== project.projectId) {
    setSummaryProject(project.projectId);
    setSummaries({});
  }

  useEffect(() => {
    if (!active) return;
    const controller = new AbortController();
    const { signal } = controller;
    // A function, so the checks after each await are not narrowed away.
    const cancelled = () => signal.aborted;
    void (async () => {
      while (!cancelled()) {
        if (!isAppActive()) {
          await delay(SUMMARY_INTERVAL_MS, signal);
          continue;
        }
        const root = project.projectRoot;
        const worktrees = project.worktrees;
        const rootSummary = await fetchSummary(root, null, project.machineId);
        if (cancelled()) return;
        setSummaries((current) => ({ ...current, [root]: rootSummary }));
        for (const worktree of worktrees) {
          const summary = await fetchSummary(worktree.path, worktree.baseBranch, project.machineId);
          if (cancelled()) return;
          setSummaries((current) => ({ ...current, [worktree.path]: summary }));
        }
        const paths = new Set([root, ...worktrees.map((worktree) => worktree.path)]);
        setSummaries((current) =>
          Object.fromEntries(Object.entries(current).filter(([path]) => paths.has(path))),
        );
        await delay(SUMMARY_INTERVAL_MS, signal);
      }
    })();
    return () => {
      controller.abort();
    };
  }, [project, active, fetchSummary, isAppActive]);

  return summaries;
}

function ProjectRow({
  name,
  branch,
  agents,
  summary,
  root = false,
  onClick,
}: {
  name: string;
  branch: string;
  agents: readonly AgentModel[];
  summary: GitReviewSummary | undefined;
  root?: boolean;
  onClick: () => void;
}) {
  return (
    <button type="button" className="project-row" onClick={onClick}>
      <Icon
        name={root ? 'folder' : 'arrow.triangle.branch'}
        size={14}
        className="project-row-icon"
      />
      <span className="project-row-body">
        <span className="project-row-name">{name}</span>
        <span className="project-row-branch">{branch}</span>
        {summary === undefined ? (
          <span className="project-row-summary pending">Reading changes…</span>
        ) : summary.error !== null ? (
          <span className="project-row-summary error">{summary.error}</span>
        ) : (
          <span className="project-row-summary">
            {`${String(summary.commitCount)} commits · ${String(summary.localFileCount)} local files`}
          </span>
        )}
        {agents.length > 0 && (
          <span className="project-row-agents">{agents.map(agentDisplayName).join(', ')}</span>
        )}
      </span>
      <Icon name="chevron.right" size={10} className="project-row-chevron" />
    </button>
  );
}

export function ProjectDetailView({
  project,
  onSelectWorktree,
  reviewSummary = defaultFetch,
  isAppActive = defaultIsActive,
}: ProjectDetailViewProps) {
  const worktrees = useStore(project.store, (state) => state.worktrees);
  const rootAgents = useStore(project.store, (state) => state.rootAgents);
  const [selectedTab, setSelectedTab] = useState<ProjectTab>('overview');
  const [showRootFiles, setShowRootFiles] = useState(false);
  const [tabProject, setTabProject] = useState(project.projectId);
  if (tabProject !== project.projectId) {
    setTabProject(project.projectId);
    setSelectedTab('overview');
    setShowRootFiles(false);
  }
  const summaries = useReviewSummaries(
    project,
    selectedTab === 'overview' && !showRootFiles,
    reviewSummary,
    isAppActive,
  );

  return (
    <div className="detail-screen" data-view="ProjectDetailView">
      <div className="detail-header project-header">
        <Icon name="folder" size={14} className="detail-header-icon" />
        <span className="detail-title">{project.projectName}</span>
        <span className="detail-subtitle">{`${String(worktrees.length)} worktrees`}</span>
        <span className="detail-spacer" />
        <div className="segmented" role="tablist" aria-label="Project view">
          {PROJECT_TABS.map((tab) => (
            <button
              key={tab}
              type="button"
              role="tab"
              aria-selected={selectedTab === tab}
              onClick={() => {
                setSelectedTab(tab);
              }}
            >
              {PROJECT_TAB_TITLES[tab]}
            </button>
          ))}
        </div>
      </div>
      {selectedTab === 'channel' ? (
        <ScreenPlaceholder
          icon="bubble.left.and.bubble.right"
          title="Project channel"
          view="ProjectChannelView"
        />
      ) : showRootFiles ? (
        <>
          <div className="back-bar">
            <button
              type="button"
              className="detail-button detail-bordered-button"
              onClick={() => {
                setShowRootFiles(false);
              }}
            >
              Back to overview
            </button>
          </div>
          <RootCheckoutDetailView project={project} />
        </>
      ) : (
        <div className="project-overview">
          <span className="section-label">WORKTREES</span>
          <div className="project-rows">
            <ProjectRow
              name="Root checkout"
              branch={project.projectName}
              agents={rootAgents}
              summary={summaries[project.projectRoot]}
              root
              onClick={() => {
                setShowRootFiles(true);
              }}
            />
            {worktrees.map((worktree) => (
              <ProjectRow
                key={worktree.id}
                name={worktree.name}
                branch={worktree.branch}
                agents={worktree.agents}
                summary={summaries[worktree.path]}
                onClick={() => {
                  onSelectWorktree(worktree.id);
                }}
              />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
