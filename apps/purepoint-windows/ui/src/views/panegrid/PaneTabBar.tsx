// Port of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/PaneTabBar.swift (WIN-7.4): the strip
// at the top of every pane: its tabs, then the pane's actions.
//
// Always shown, even for a single tab, so a background agent's state is visible without switching
// to it and the pane actions never hide behind a hover. Like the Mac's, the strip is always dark.
//
// Differences from Swift: drag and drop is HTML5 (tabDrag.ts); `TabCloseConfirmation` is
// tabCloseConfirmation.tsx; the Point Guard pane controls come with the Point Guard chat (WIN-9.x).
import { type DragEvent, createContext, useContext, useState, useSyncExternalStore } from 'react';
import { useStore } from 'zustand';
import { isAgentStatusAlive } from '../../model/agentStatus.ts';
import { canSplit, topTrailingLeafId } from '../../model/paneSplit.ts';
import { agentDisplayName, type AgentModel } from '../../model/workspaceModel.ts';
import { contentAgentId, paneActiveTab, paneCount, type Surface } from '../../model/workspace.ts';
import type { WorkspaceRegistry } from '../../state/workspaceRegistry.ts';
import { useSidebarVisibilityStore } from '../../state/sidebarVisibilityStore.ts';
import { pointGuardSessionFor } from '../../state/pointGuardSession.ts';
import { Icon } from '../../theme/Icon.tsx';
import type { SfSymbolName } from '../../theme/icons.ts';
import { WorkspaceSidebarToggle } from '../detail/WorkspaceFilesSidebar.tsx';
import { PointGuardPaneControls } from '../chat/PointGuardPaneControls.tsx';
import { useAppState } from '../shell/appContext.ts';
import { terminalHost } from '../terminal/terminals.ts';
import { closeConfirmationSource, confirmTabClose } from './tabCloseConfirmation.tsx';
import { droppedSurfaceId, isTabDrag, startTabDrag, useTabDrag } from './tabDrag.ts';

/** Swift `PaneTabBar.height`. */
export const PANE_TAB_BAR_HEIGHT = 30;

/**
 * Whether the grid's top-right strip carries the workspace sidebar's reopen button while the
 * sidebar is hidden (Swift `showsWorkspaceSidebarToggle`): true inside a workspace files layout.
 */
export const ShowsWorkspaceSidebarToggleContext = createContext(false);

/** Whether an agent's terminal printed output while not on screen (the tab's dot). */
function useUnseenOutput(agentId: string | null): boolean {
  const cache = terminalHost().cache;
  return useSyncExternalStore(
    (listener) => cache.subscribe(listener),
    () => agentId !== null && cache.hasUnseenOutput(agentId),
  );
}

/** Drag-over highlighting for a drop target that takes tabs. */
function useTabDropTarget(onDrop: (surfaceId: number) => void) {
  const [targeted, setTargeted] = useState(false);
  return {
    targeted,
    handlers: {
      onDragEnter: (event: DragEvent) => {
        if (!isTabDrag(event.dataTransfer)) return;
        event.preventDefault();
        setTargeted(true);
      },
      onDragOver: (event: DragEvent) => {
        if (!isTabDrag(event.dataTransfer)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
      },
      onDragLeave: (event: DragEvent) => {
        const next = event.relatedTarget;
        if (next instanceof Node && event.currentTarget.contains(next)) return;
        setTargeted(false);
      },
      onDrop: (event: DragEvent) => {
        setTargeted(false);
        useTabDrag.getState().end();
        const surfaceId = droppedSurfaceId(event.dataTransfer);
        if (surfaceId === null) return;
        event.preventDefault();
        event.stopPropagation();
        onDrop(surfaceId);
      },
    },
  };
}

export interface PaneTabBarProps {
  workspaceId: string;
  leafId: number;
  isFocused: boolean;
  registry: WorkspaceRegistry;
}

export function PaneTabBar({ workspaceId, leafId, isFocused, registry }: PaneTabBarProps) {
  const app = useAppState();
  const workspace = useStore(registry.store, () => registry.workspace(workspaceId));
  const showsToggleHere = useContext(ShowsWorkspaceSidebarToggleContext);
  const sidebarShown = useSidebarVisibilityStore((state) => state.workspaceSidebarShown);
  const pane = workspace?.panes[leafId];
  const strip = useTabDropTarget((surfaceId) => {
    // Dropping past the last tab appends to this pane.
    registry.moveTab(workspaceId, surfaceId, leafId);
  });

  if (workspace === null || pane === undefined) return null;
  const splittable = canSplit(workspace.root);

  const split = (axis: 'vertical' | 'horizontal') => {
    registry.setFocus(workspaceId, leafId);
    registry.split(workspaceId, leafId, axis);
    const after = registry.workspace(workspaceId);
    const focused = after?.panes[after.focusedLeafId];
    registry.markPendingPalette(focused?.activeTabId ?? null);
  };

  return (
    <div
      className="pane-tab-bar"
      data-testid="pane-tab-bar"
      data-leaf-id={leafId}
      data-drop-targeted={strip.targeted}
      style={{ height: PANE_TAB_BAR_HEIGHT }}
    >
      <div className="pane-tab-strip" role="tablist" aria-label="Tabs" {...strip.handlers}>
        {pane.tabs.map((surface) => (
          <PaneTab
            key={surface.id}
            workspaceId={workspaceId}
            surface={surface}
            isActive={surface.id === pane.activeTabId}
            isFocusedPane={isFocused}
            registry={registry}
          />
        ))}
      </div>
      <TabBarButton
        icon="plus"
        help="New Tab"
        onClick={() => {
          registry.markPendingPalette(registry.newTab(workspaceId, leafId));
        }}
      />
      <div className="pane-tab-bar-spacer" />
      <div className="pane-tab-bar-divider" />
      <div className="pane-tab-bar-actions">
        {paneActiveTab(pane)?.content.kind === 'pointGuard' && (
          <PointGuardPaneControls session={pointGuardSessionFor(app)} />
        )}
        {splittable && (
          <TabBarButton
            icon="rectangle.split.2x1"
            help="Split Right"
            onClick={() => {
              split('vertical');
            }}
          />
        )}
        {splittable && (
          <TabBarButton
            icon="rectangle.split.1x2"
            help="Split Below"
            onClick={() => {
              split('horizontal');
            }}
          />
        )}
        {paneCount(workspace) > 1 && (
          <TabBarButton
            icon="xmark.rectangle"
            help="Close Pane"
            onClick={() => {
              confirmTabClose(pane.tabs, workspaceId, closeConfirmationSource(app), () => {
                registry.closePane(workspaceId, leafId);
              });
            }}
          />
        )}
        {/* The hidden workspace sidebar reopens from the strip's trailing end. */}
        {showsToggleHere && !sidebarShown && topTrailingLeafId(workspace.root) === leafId && (
          <WorkspaceSidebarToggle />
        )}
      </div>
    </div>
  );
}

interface PaneTabProps {
  workspaceId: string;
  surface: Surface;
  isActive: boolean;
  isFocusedPane: boolean;
  registry: WorkspaceRegistry;
}

/** The kind/status icon and its color class (Swift `icon`). */
function tabIcon(surface: Surface, agent: AgentModel | null): [SfSymbolName, string] {
  switch (surface.content.kind) {
    case 'agent': {
      const alive = agent !== null && isAgentStatusAlive(agent.status);
      if (agent?.agentType === 'terminal') return ['terminal', alive ? 'secondary' : 'red'];
      if (agent !== null && !alive) return ['xmark.circle', 'red'];
      return ['sparkle', 'green'];
    }
    case 'pointGuard':
      return ['bubble.left.and.bubble.right', 'secondary'];
    case 'file':
      return ['doc.text', 'secondary'];
    case 'empty':
      return ['magnifyingglass', 'secondary'];
  }
}

/** Swift `title`. */
export function tabTitle(surface: Surface, agent: AgentModel | null): string {
  switch (surface.content.kind) {
    case 'agent':
      return agent === null ? surface.content.agentId : agentDisplayName(agent);
    case 'pointGuard':
      return 'Point Guard';
    case 'file': {
      const path = surface.content.path;
      if (path === null) return 'Files';
      return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
    }
    case 'empty':
      return 'New Tab';
  }
}

/**
 * One tab: kind/status icon, name, and a trailing slot that shows the close button on the active
 * or hovered tab and the unseen-output dot otherwise.
 */
function PaneTab({ workspaceId, surface, isActive, isFocusedPane, registry }: PaneTabProps) {
  const app = useAppState();
  // Agents' names and statuses change with the projects.
  useStore(app.store, (state) => state.projectRevision);
  const agentId = contentAgentId(surface.content);
  const agent = agentId === null ? null : app.agent(agentId);
  const unseen = useUnseenOutput(isActive ? null : agentId);
  const [hovered, setHovered] = useState(false);
  const drop = useTabDropTarget((moved) => {
    // Dropping on a tab inserts before it.
    if (moved !== surface.id) registry.moveTabBefore(workspaceId, moved, surface.id);
  });

  const title = tabTitle(surface, agent);
  const [icon, tint] = tabIcon(surface, agent);
  const help = [
    title,
    ...(agent === null ? [] : [isAgentStatusAlive(agent.status) ? 'running' : 'exited']),
    ...(unseen ? ['new output'] : []),
  ].join(' — ');

  const close = () => {
    confirmTabClose([surface], workspaceId, closeConfirmationSource(app), () => {
      registry.closeTab(workspaceId, surface.id);
    });
  };

  return (
    <div
      className="pane-tab"
      role="tab"
      aria-selected={isActive}
      aria-label={help}
      title={help}
      tabIndex={-1}
      draggable
      data-testid="pane-tab"
      data-surface-id={surface.id}
      data-active={isActive}
      data-focused-pane={isFocusedPane}
      data-unseen={unseen}
      data-drop-targeted={drop.targeted}
      onMouseEnter={() => {
        setHovered(true);
      }}
      onMouseLeave={() => {
        setHovered(false);
      }}
      onClick={() => {
        registry.selectTab(workspaceId, surface.id);
      }}
      onMouseDown={(event) => {
        // No autoscroll on a middle press; the release closes the tab.
        if (event.button === 1) event.preventDefault();
      }}
      onAuxClick={(event) => {
        if (event.button === 1) close();
      }}
      onDragStart={(event) => {
        startTabDrag(event.dataTransfer, surface.id);
        useTabDrag.getState().begin(surface.id);
      }}
      onDragEnd={() => {
        useTabDrag.getState().end();
      }}
      {...drop.handlers}
    >
      <Icon name={icon} size={11} className="pane-tab-icon" data-tint={tint} />
      <span className="pane-tab-title">{title}</span>
      <span className="pane-tab-trailing">
        {isActive || hovered ? (
          <button
            type="button"
            className="pane-tab-close"
            aria-label="Close Tab"
            data-testid="pane-tab-close"
            draggable={false}
            onClick={(event) => {
              event.stopPropagation();
              close();
            }}
          >
            <Icon name="xmark" size={9} />
          </button>
        ) : unseen ? (
          <span className="pane-tab-unseen" aria-hidden="true" data-testid="pane-tab-unseen" />
        ) : null}
      </span>
    </div>
  );
}

/** Swift `TabBarButton`: an icon button in the pane's strip. */
export function TabBarButton({
  icon,
  help,
  onClick,
}: {
  icon: SfSymbolName;
  help: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className="pane-tab-bar-button"
      aria-label={help}
      title={help}
      onClick={onClick}
    >
      <Icon name={icon} size={11} />
    </button>
  );
}
