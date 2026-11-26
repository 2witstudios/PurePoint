// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/DetailView.swift (WIN-7.1).
//
// Routes the sidebar selection to its screen. The screens are later leaves; until each lands its
// slot renders a placeholder naming the view that will fill it (the leaf is in the parity matrix).
// WIN-8.2 fills the project, worktree and workspace files screens.
import { useStore } from 'zustand';
import { isPointGuard } from '../../model/workspace.ts';
import type { SidebarSelection } from '../../model/sidebarItem.ts';
import type { ProjectState } from '../../state/projectState.ts';
import { useAppState } from '../shell/appContext.ts';
import { useTitlebarClearance } from '../shell/TitlebarClearance.tsx';
import { AgentsHubView } from '../agents/AgentsHubView.tsx';
import { PaneGridView } from '../panegrid/PaneGridView.tsx';
import { ShowsWorkspaceSidebarToggleContext } from '../panegrid/PaneTabBar.tsx';
import { ScheduleView } from '../schedule/ScheduleView.tsx';
import { ScreenPlaceholder } from './placeholders.tsx';
import { ProjectChannelView } from '../channel/ProjectChannelView.tsx';
import { ProjectDetailView } from './ProjectDetailView.tsx';
import { WorkspaceFilesLayout } from './WorkspaceFilesLayout.tsx';
import { WorktreeDetailView } from './WorktreeDetailView.tsx';

export { ScreenPlaceholder } from './placeholders.tsx';

export interface DetailViewProps {
  selection: SidebarSelection | null;
}

/** Follows the worktree's model (branch, base, agents) as the project reports changes. */
function WorktreeRoute({ project, worktreeId }: { project: ProjectState; worktreeId: string }) {
  const worktree = useStore(project.store, (state) =>
    state.worktrees.find((candidate) => candidate.id === worktreeId),
  );
  return worktree === undefined ? (
    <ScreenPlaceholder icon="arrow.triangle.branch" title="Worktree not found" />
  ) : (
    <WorktreeDetailView worktree={worktree} project={project} />
  );
}

function SelectedContent({ selection }: { selection: SidebarSelection }) {
  const app = useAppState();
  // Re-render when projects or workspaces change, so a placeholder turns into "not found".
  useStore(app.store, (state) => state.projectRevision);
  const registry = app.registry;

  switch (selection.kind) {
    // Every workspace renders as a grid; a one-pane workspace is a grid of one.
    case 'workspace': {
      if (registry === null) {
        return (
          <ScreenPlaceholder icon="rectangle.split.2x2" title="Workspace" view="PaneGridView" />
        );
      }
      const workspace = registry.workspace(selection.id);
      if (workspace === null) {
        return <ScreenPlaceholder icon="rectangle.split.2x2" title="Workspace not found" />;
      }
      if (isPointGuard(workspace)) {
        return (
          <ScreenPlaceholder
            icon="bubble.left.and.bubble.right"
            title="Point Guard"
            view="PointGuardView"
          />
        );
      }
      const rootPath = app.fileRoot(selection.id);
      const paneGrid = (
        <PaneGridView key={selection.id} workspaceId={selection.id} registry={registry} />
      );
      return rootPath === null ? (
        paneGrid
      ) : (
        <WorkspaceFilesLayout
          rootPath={rootPath}
          machineId={app.projectStateForWorkspace(selection.id)?.machineId ?? null}
          reopenInContent={false}
        >
          <ShowsWorkspaceSidebarToggleContext value={true}>
            {paneGrid}
          </ShowsWorkspaceSidebarToggleContext>
        </WorkspaceFilesLayout>
      );
    }
    case 'nav':
      switch (selection.item) {
        case 'dashboard':
          return (
            <ScreenPlaceholder
              icon="bubble.left.and.bubble.right"
              title="Point Guard"
              view="PointGuardView"
            />
          );
        case 'agents':
          return <AgentsHubView />;
        case 'schedule':
          return <ScheduleView />;
      }
      break;
    case 'worktree': {
      const project = app.projectStateForWorktree(selection.id);
      return project === null ? (
        <ScreenPlaceholder icon="arrow.triangle.branch" title="Worktree not found" />
      ) : (
        <WorktreeRoute key={selection.id} project={project} worktreeId={selection.id} />
      );
    }
    case 'channel': {
      const project = app.projectState(selection.id);
      return project === null ? (
        <ScreenPlaceholder icon="bubble.left.and.bubble.right" title="Project channel" />
      ) : (
        <ProjectChannelView key={project.projectId} project={project} />
      );
    }
    case 'project': {
      const project = app.projectState(selection.id);
      return project === null ? (
        <ScreenPlaceholder icon="folder.fill" title="Project" />
      ) : (
        <ProjectDetailView
          project={project}
          onSelectWorktree={(worktreeId) => {
            // The shell selects it (ContentView follows `pendingSelectWorktreeId`).
            app.setPendingSelectWorktreeId(worktreeId);
          }}
        />
      );
    }
  }
  return null;
}

export function DetailView({ selection }: DetailViewProps) {
  const clearance = useTitlebarClearance();
  return (
    // The title bar is a band across the whole window on Windows, so every screen starts below
    // it (the Mac's dashboard and workspaces draw their tab strip in the band instead).
    <div className="detail-view" style={{ paddingTop: clearance.height }}>
      {selection === null ? (
        <ScreenPlaceholder icon="sidebar.left" title="Select an item" />
      ) : (
        <SelectedContent selection={selection} />
      )}
    </div>
  );
}
