// Port of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/PaneCellView.swift (WIN-7.4): one pane
// of the grid, its tab strip above the active tab's content: a terminal, the file navigator, or
// the empty-tab placeholder.
//
// Only the active tab is rendered. A background agent's terminal stays alive in the terminal cache
// (keyed by agent ID), so switching back is instant and keeps scrollback.
//
// Differences from Swift:
// - The file navigator (`FilePaneView`) is WIN-8.4/8.5 and Point Guard's chat surface is WIN-9.x;
//   their tabs show placeholders naming those views.
// - The empty tab's New Agent button, and the tab a split or New Tab creates, open the command
//   palette (views/palette/openPalette.ts `openEmptyTabPalette`).
// - While a tab is dragged, the content is covered by a drop overlay: the centre appends the tab
//   to this pane, an edge splits it off on that side (tabDrag.ts).
import { type DragEvent, useEffect, useState } from 'react';
import { useStore } from 'zustand';
import { paneActiveTab } from '../../model/workspace.ts';
import type { WorkspaceRegistry } from '../../state/workspaceRegistry.ts';
import { Icon } from '../../theme/Icon.tsx';
import { ScreenPlaceholder } from '../detail/placeholders.tsx';
import { openEmptyTabPalette } from '../palette/openPalette.ts';
import { useAppState } from '../shell/appContext.ts';
import { TerminalPane } from '../terminal/TerminalPane.tsx';
import { PointGuardChatSurfaceView } from '../chat/PointGuardView.tsx';
import { PaneTabBar } from './PaneTabBar.tsx';
import {
  type PaneDropZone,
  dropZoneAt,
  droppedSurfaceId,
  isTabDrag,
  useTabDrag,
} from './tabDrag.ts';

export interface PaneCellViewProps {
  workspaceId: string;
  leafId: number;
  isFocused: boolean;
  registry: WorkspaceRegistry;
}

export function PaneCellView({ workspaceId, leafId, isFocused, registry }: PaneCellViewProps) {
  const pane = useStore(registry.store, () => registry.workspace(workspaceId)?.panes[leafId]);
  const activeTab = pane === undefined ? null : paneActiveTab(pane);
  const focus = () => {
    registry.setFocus(workspaceId, leafId);
  };

  return (
    <div
      className="pane-cell"
      data-testid="pane-cell"
      data-leaf-id={leafId}
      data-focused={isFocused}
      onMouseDown={focus}
    >
      <PaneTabBar
        workspaceId={workspaceId}
        leafId={leafId}
        isFocused={isFocused}
        registry={registry}
      />
      <div className="pane-cell-content">
        {activeTab !== null && (
          <PaneContent
            workspaceId={workspaceId}
            surfaceId={activeTab.id}
            content={activeTab.content}
            isFocused={isFocused}
            onFocus={focus}
            registry={registry}
          />
        )}
        <PaneDropOverlay workspaceId={workspaceId} leafId={leafId} registry={registry} />
      </div>
    </div>
  );
}

function PaneContent({
  workspaceId,
  surfaceId,
  content,
  isFocused,
  onFocus,
  registry,
}: {
  workspaceId: string;
  surfaceId: number;
  content: NonNullable<ReturnType<typeof paneActiveTab>>['content'];
  isFocused: boolean;
  onFocus: () => void;
  registry: WorkspaceRegistry;
}) {
  const app = useAppState();
  // An agent the manifest has not delivered yet shows the terminal background until it does.
  const known = useStore(
    app.store,
    () => content.kind === 'agent' && app.agent(content.agentId) !== null,
  );
  switch (content.kind) {
    case 'agent':
      return known ? (
        <TerminalPane agentId={content.agentId} autoFocus={isFocused} onFocus={onFocus} />
      ) : (
        <div className="pane-terminal-pending" data-testid="pane-terminal-pending" />
      );
    case 'pointGuard':
      return <PointGuardChatSurfaceView isFocused={isFocused} />;
    case 'file':
      return <ScreenPlaceholder icon="doc.text" title="Files" view="FilePaneView" />;
    case 'empty':
      return (
        <EmptyTabView
          key={surfaceId}
          workspaceId={workspaceId}
          surfaceId={surfaceId}
          registry={registry}
        />
      );
  }
}

/** Swift `EmptyTabView`: an empty tab, which opens the palette to spawn an agent or open files. */
function EmptyTabView({
  workspaceId,
  surfaceId,
  registry,
}: {
  workspaceId: string;
  surfaceId: number;
  registry: WorkspaceRegistry;
}) {
  const app = useAppState();
  const pending = useStore(registry.store, (state) => state.pendingPaletteSurfaceId);
  // A split or New Tab marks the tab it created; that tab opens the palette once it is on screen.
  useEffect(() => {
    if (pending !== surfaceId) return;
    registry.markPendingPalette(null);
    openEmptyTabPalette(app, workspaceId, surfaceId);
  }, [app, pending, registry, surfaceId, workspaceId]);

  return (
    <div className="pane-empty-tab" data-testid="pane-empty-tab">
      <Icon name="rectangle.dashed" size={32} className="pane-empty-tab-icon" />
      <span className="pane-empty-tab-title">Empty Tab</span>
      <span className="pane-empty-tab-caption">Spawn a new agent, or open files</span>
      <button
        type="button"
        className="pane-empty-tab-button"
        onClick={() => {
          openEmptyTabPalette(app, workspaceId, surfaceId);
        }}
      >
        New Agent{'…'}
      </button>
    </div>
  );
}

/** Covers the content while a tab is dragged: the centre appends, an edge splits (tabDrag.ts). */
function PaneDropOverlay({
  workspaceId,
  leafId,
  registry,
}: {
  workspaceId: string;
  leafId: number;
  registry: WorkspaceRegistry;
}) {
  const dragging = useTabDrag((state) => state.surfaceId !== null);
  const [zone, setZone] = useState<PaneDropZone | null>(null);
  if (!dragging) return null;

  const zoneOf = (event: DragEvent<HTMLDivElement>): PaneDropZone => {
    const rect = event.currentTarget.getBoundingClientRect();
    return dropZoneAt(event.clientX - rect.left, event.clientY - rect.top, rect.width, rect.height);
  };

  return (
    <div
      className="pane-drop-overlay"
      data-testid="pane-drop-overlay"
      data-leaf-id={leafId}
      data-zone={zone ?? undefined}
      onDragEnter={(event) => {
        if (!isTabDrag(event.dataTransfer)) return;
        event.preventDefault();
        setZone(zoneOf(event));
      }}
      onDragOver={(event) => {
        if (!isTabDrag(event.dataTransfer)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = 'move';
        const next = zoneOf(event);
        if (next !== zone) setZone(next);
      }}
      onDragLeave={() => {
        setZone(null);
      }}
      onDrop={(event) => {
        const where = zoneOf(event);
        setZone(null);
        useTabDrag.getState().end();
        const surfaceId = droppedSurfaceId(event.dataTransfer);
        if (surfaceId === null) return;
        event.preventDefault();
        if (where === 'center') registry.moveTab(workspaceId, surfaceId, leafId);
        else registry.dropTabOnEdge(workspaceId, surfaceId, leafId, where);
      }}
    >
      {zone !== null && <div className="pane-drop-indicator" data-zone={zone} />}
    </div>
  );
}
