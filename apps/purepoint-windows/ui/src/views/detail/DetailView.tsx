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
import { useAgentSelection } from '../terminal/agentSelection.ts';
import { AgentTerminalsView } from '../terminal/AgentTerminals.tsx';
import { ScheduleView } from '../schedule/ScheduleView.tsx';
import { ScreenPlaceholder } from './placeholders.tsx';
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
        <ScreenPlaceholder icon="rectangle.split.2x2" title="Workspace" view="PaneGridView" />
      );
      return rootPath === null ? (
        paneGrid
      ) : (
        <WorkspaceFilesLayout
          rootPath={rootPath}
          machineId={app.projectStateForWorkspace(selection.id)?.machineId ?? null}
        >
          {paneGrid}
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
          return <ScreenPlaceholder icon="cpu" title="Agents" view="AgentsHubView" />;
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
    case 'channel':
      return (
        <ScreenPlaceholder
          icon="bubble.left.and.bubble.right"
          title="Project channel"
          {...(app.projectState(selection.id) === null ? {} : { view: 'ProjectChannelView' })}
        />
      );
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
  // Selected agents show their terminals (WIN-7.3) until the pane grid (WIN-7.4) takes over.
  const agentIds = useAgentSelection((state) => state.agentIds);
  return (
    // The title bar is a band across the whole window on Windows, so every screen starts below
    // it (the Mac's dashboard and workspaces draw their tab strip in the band instead).
    <div className="detail-view" style={{ paddingTop: clearance.height }}>
      {agentIds.length > 0 ? (
        <AgentTerminalsView agentIds={agentIds} />
      ) : selection === null ? (
        <ScreenPlaceholder icon="sidebar.left" title="Select an item" />
      ) : (
        <SelectedContent selection={selection} />
      )}
    </div>
  );
}
