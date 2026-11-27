// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineViewController+ProjectDrag.swift
// (WIN-7.8).
//
// Dragging a project row reorders it within its section (the local projects, or one machine's).
// The order is saved on the machine that owns the project (`reorder_projects`), so every device
// lists it the same way.
//
// Differences from Swift: HTML5 drag and drop (REQ-WINAPP-066) in place of the pasteboard. A
// drag carries the project id under its own MIME type, which a drop target can see while the drag
// is over it (`getData` is empty until the drop). The Mac drops between rows; here a project row's
// top half drops before it and its bottom half after it, and any row inside a project drops before
// that project, as the Mac's `validateDrop` retargets it. The MACHINES title drops at the end of
// the local projects.

import type { SidebarNode } from '../../model/sidebarItem.ts';
import { type ProjectDrop, projectDrop, sidebarAncestors } from '../../model/sidebarOutline.ts';

/** Swift `projectDragType` (`com.purepoint.sidebar-project`). */
export const PROJECT_DRAG_TYPE = 'application/x-purepoint-sidebar-project';

export function startProjectDrag(dataTransfer: DataTransfer, projectId: string): void {
  dataTransfer.setData(PROJECT_DRAG_TYPE, projectId);
  dataTransfer.effectAllowed = 'move';
}

export function isProjectDrag(dataTransfer: DataTransfer | null): boolean {
  return dataTransfer !== null && Array.from(dataTransfer.types).includes(PROJECT_DRAG_TYPE);
}

export function draggedProjectId(dataTransfer: DataTransfer | null): string | null {
  const id = dataTransfer?.getData(PROJECT_DRAG_TYPE) ?? '';
  return id === '' ? null : id;
}

export type ProjectDropPosition = 'before' | 'after';

/** The half of a row the pointer is over. */
export function dropPositionAt(clientY: number, top: number, height: number): ProjectDropPosition {
  return clientY < top + height / 2 ? 'before' : 'after';
}

/** Where the drop indicator is drawn and where the dragged project lands. */
export interface ProjectDropTarget {
  /** The row the indicator is drawn on. */
  readonly rowKey: string;
  readonly position: ProjectDropPosition;
  readonly drop: ProjectDrop;
}

/**
 * Swift `validateDrop`: a drop of `projectId` over `hovered` (in `position`), or null when the
 * project cannot land there (another section, or where it already is).
 */
export function projectDropTarget(
  projectId: string,
  hovered: SidebarNode,
  position: ProjectDropPosition,
  roots: readonly SidebarNode[],
): ProjectDropTarget | null {
  const chain = sidebarAncestors((node) => node === hovered, roots);
  if (hovered.kind === 'sectionHeader') {
    // The MACHINES title: after the last local project.
    const index = roots.indexOf(hovered);
    const drop = index === -1 ? null : projectDrop(projectId, null, index, roots);
    return drop === null ? null : { rowKey: hovered.rowKey, position: 'before', drop };
  }
  // Hovering on or inside a project means that project, among its siblings.
  const projectIndex =
    hovered.kind === 'project' ? chain.length : chain.findIndex((node) => node.kind === 'project');
  if (projectIndex === -1) return null;
  const project = projectIndex === chain.length ? hovered : chain[projectIndex];
  if (project === undefined) return null;
  const parent = projectIndex === 0 ? null : (chain[projectIndex - 1] ?? null);
  const siblings = parent === null ? roots : parent.children;
  const childIndex = siblings.indexOf(project);
  if (childIndex === -1) return null;
  const side: ProjectDropPosition = project === hovered ? position : 'before';
  const drop = projectDrop(projectId, parent, childIndex + (side === 'after' ? 1 : 0), roots);
  return drop === null ? null : { rowKey: project.rowKey, position: side, drop };
}
