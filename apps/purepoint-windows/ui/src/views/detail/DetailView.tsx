// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/DetailView.swift (WIN-7.1).
//
// Routes the sidebar selection to its screen. The screens are later leaves; until each lands its
// slot renders a placeholder naming the view that will fill it (the leaf is in the parity matrix).
import { useStore } from 'zustand';
import { isPointGuard } from '../../model/workspace.ts';
import type { SidebarSelection } from '../../model/sidebarItem.ts';
import { Icon } from '../../theme/Icon.tsx';
import type { SfSymbolName } from '../../theme/icons.ts';
import { useAppState } from '../shell/appContext.ts';
import { useTitlebarClearance } from '../shell/TitlebarClearance.tsx';

export interface DetailViewProps {
  selection: SidebarSelection | null;
}

/** A screen that is not ported yet: its title, and the view that will draw it. */
export function ScreenPlaceholder({
  icon,
  title,
  view,
}: {
  icon: SfSymbolName;
  title: string;
  view?: string;
}) {
  return (
    <div className="detail-placeholder" data-view={view}>
      <Icon name={icon} size={40} className="detail-placeholder-icon" />
      <span className="detail-placeholder-title">{title}</span>
      {view !== undefined && <span className="detail-placeholder-view">{view}</span>}
    </div>
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
      return app.fileRoot(selection.id) === null ? (
        <ScreenPlaceholder icon="rectangle.split.2x2" title="Workspace" view="PaneGridView" />
      ) : (
        <ScreenPlaceholder
          icon="rectangle.split.2x2"
          title="Workspace"
          view="WorkspaceFilesLayout + PaneGridView"
        />
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
          return <ScreenPlaceholder icon="calendar" title="Schedule" view="ScheduleView" />;
      }
      break;
    case 'worktree':
      return app.projectStateForWorktree(selection.id) === null ? (
        <ScreenPlaceholder icon="arrow.triangle.branch" title="Worktree not found" />
      ) : (
        <ScreenPlaceholder
          icon="arrow.triangle.branch"
          title="Worktree"
          view="WorktreeDetailView"
        />
      );
    case 'channel':
      return (
        <ScreenPlaceholder
          icon="bubble.left.and.bubble.right"
          title="Project channel"
          {...(app.projectState(selection.id) === null ? {} : { view: 'ProjectChannelView' })}
        />
      );
    case 'project':
      return (
        <ScreenPlaceholder
          icon="folder.fill"
          title="Project"
          {...(app.projectState(selection.id) === null ? {} : { view: 'ProjectDetailView' })}
        />
      );
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
