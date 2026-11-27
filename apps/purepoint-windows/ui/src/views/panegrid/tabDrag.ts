// Tab drag and drop for the pane grid (WIN-7.4): Swift `TabDragPayload` plus the drop geometry.
//
// HTML5 drag and drop (the Tauri window sets `dragDropEnabled: false`, REQ-WINAPP-066, so the page
// gets the events). The payload is a string with a prefix that keeps stray text drops out, as on
// the Mac. It also rides under its own MIME type, because `dataTransfer.getData` is empty until
// the drop: during `dragover` only the type list is readable, and the type is what says "a tab".
//
// Differences from Swift: the Mac drops a tab before another tab or appends it to a strip; here a
// tab can also land in a pane's content area, appended (the centre) or split off on an edge
// (`dropTabOnEdge`), like the iPad's and most tabbed editors' grids.
import { create } from 'zustand';
import type { PaneEdge } from '../../model/workspace.ts';

const PREFIX = 'purepoint-tab:';
/** The MIME type a tab drag carries, readable while dragging over a target. */
export const TAB_DRAG_TYPE = 'application/x-purepoint-tab';

export function tabDragString(surfaceId: number): string {
  return PREFIX + String(surfaceId);
}

export function surfaceIdFromDragString(text: string): number | null {
  if (!text.startsWith(PREFIX)) return null;
  const rest = text.slice(PREFIX.length);
  if (!/^\d+$/.test(rest)) return null;
  return Number(rest);
}

/** Whether a drag in progress is a tab (readable during `dragenter` and `dragover`). */
export function isTabDrag(dataTransfer: DataTransfer | null): boolean {
  return dataTransfer !== null && Array.from(dataTransfer.types).includes(TAB_DRAG_TYPE);
}

/** Put a tab on a drag that is starting. */
export function startTabDrag(dataTransfer: DataTransfer, surfaceId: number): void {
  const payload = tabDragString(surfaceId);
  dataTransfer.setData(TAB_DRAG_TYPE, payload);
  dataTransfer.setData('text/plain', payload);
  dataTransfer.effectAllowed = 'move';
}

/** The tab a drop carries, or null for anything else. */
export function droppedSurfaceId(dataTransfer: DataTransfer | null): number | null {
  if (dataTransfer === null) return null;
  return (
    surfaceIdFromDragString(dataTransfer.getData(TAB_DRAG_TYPE)) ??
    surfaceIdFromDragString(dataTransfer.getData('text/plain'))
  );
}

/** Where in a pane's content a tab lands: an edge (split off there) or the centre (append). */
export type PaneDropZone = PaneEdge | 'center';

/** How far in from an edge, as a share of the pane's size, the edge's drop zone reaches. */
export const EDGE_ZONE_FRACTION = 0.25;

/**
 * The zone under a point in a pane `width` x `height`, with `x` and `y` measured from its top-left
 * corner. The nearest edge wins inside the edge bands; everywhere else is the centre.
 */
export function dropZoneAt(x: number, y: number, width: number, height: number): PaneDropZone {
  if (!(width > 0) || !(height > 0)) return 'center';
  const distances: [PaneEdge, number][] = [
    ['left', x / width],
    ['right', (width - x) / width],
    ['top', y / height],
    ['bottom', (height - y) / height],
  ];
  let best: [PaneEdge, number] | null = null;
  for (const entry of distances) {
    if (entry[1] < EDGE_ZONE_FRACTION && (best === null || entry[1] < best[1])) best = entry;
  }
  return best === null ? 'center' : best[0];
}

interface TabDragState {
  /** The tab being dragged, while a tab drag is in progress. */
  readonly surfaceId: number | null;
  begin(surfaceId: number): void;
  end(): void;
}

/**
 * The tab drag in progress. The panes cover their content with a drop overlay while it is set, so
 * a terminal under the pointer never swallows the drag. A drop clears it as well as `dragend`,
 * because a tab that moved panes is a new element and its old one never sees `dragend`.
 */
export const useTabDrag = create<TabDragState>()((set) => ({
  surfaceId: null,
  begin: (surfaceId) => {
    set({ surfaceId });
  },
  end: () => {
    set({ surfaceId: null });
  },
}));
