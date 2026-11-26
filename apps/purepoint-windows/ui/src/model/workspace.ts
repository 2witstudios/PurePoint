// Port of apps/purepoint-macos/purepoint-macos/Models/Workspace.swift (WIN-6.1).
//
// A workspace is the single unit of the UI: **one sidebar row, one pane layout.** The sidebar
// renders workspaces and nothing else, so there is no code path that turns an agent into a row on
// its own: a pane and a stray row for the same agent are one state, not two kept in sync.
//
// Layout and content are separate: `root` is pure geometry, and `panes` maps each of its leaves to
// a tab stack of surfaces. Invariants, restored by `normalizeWorkspace`:
// 1. The keys of `panes` are exactly the tree's leaf IDs, and every pane has a tab.
// 2. Surface IDs are unique within the workspace and below `nextSurfaceId`.
// 3. Every live agent in the manifest occupies exactly one surface of exactly one workspace.
//    `reconcileWorkspaces` is the only function that establishes that, and it is total: any
//    (stored layout, manifest) pair maps to one canonical answer.
//
// Swift's `Workspace` is a value type with `mutating` methods. Here every type is immutable data
// and every mutation is a function returning a new value (plus whatever the Swift method
// returned). `panes` is a record keyed by leaf ID so a workspace stays plain JSON.

import type { SpawnPlacement } from '../protocol';
import {
  allLeafIds,
  canSplit,
  findAdjacentLeaf,
  firstLeafId,
  leaf,
  leafCount,
  removingLeaf,
  removingLeaves,
  settingRatio,
  siblingLeafId,
  splittingLeaf,
  type PaneAxis,
  type PaneSplitNode,
} from './paneSplit';
import { structurallyEqual, type AgentModel, type WorktreeModel } from './workspaceModel';

export type { SpawnPlacement } from '../protocol';

// MARK: - WorkspaceContainer

/**
 * Where a workspace lives: directly in the project root, or inside a git worktree. Mirrors the
 * manifest's only two containers (`Manifest.agents` vs `WorktreeEntry.agents`).
 */
export type WorkspaceContainer =
  { readonly kind: 'projectRoot' } | { readonly kind: 'worktree'; readonly worktreeId: string };

export const PROJECT_ROOT: WorkspaceContainer = { kind: 'projectRoot' };

export function worktreeContainer(worktreeId: string): WorkspaceContainer {
  return { kind: 'worktree', worktreeId };
}

export function containerWorktreeId(container: WorkspaceContainer): string | null {
  return container.kind === 'worktree' ? container.worktreeId : null;
}

export function containersEqual(a: WorkspaceContainer, b: WorkspaceContainer): boolean {
  return a.kind === b.kind && containerWorktreeId(a) === containerWorktreeId(b);
}

// MARK: - SurfaceContent

/** What a tab shows. Shells are agents too (the daemon spawns them under an `ag-` ID). */
export type SurfaceContent =
  /** A tab waiting for the palette: spawn an agent, open a file, or stay empty. */
  | { readonly kind: 'empty' }
  /** The shared Point Guard chat. */
  | { readonly kind: 'pointGuard' }
  | { readonly kind: 'agent'; readonly agentId: string }
  /** The file navigator/editor. `path` is the absolute file to show; `null` opens the navigator alone. */
  | { readonly kind: 'file'; readonly path: string | null };

export const EMPTY_CONTENT: SurfaceContent = { kind: 'empty' };
export const POINT_GUARD_CONTENT: SurfaceContent = { kind: 'pointGuard' };

export function agentContent(agentId: string): SurfaceContent {
  return { kind: 'agent', agentId };
}

export function fileContent(path: string | null): SurfaceContent {
  return { kind: 'file', path };
}

export function contentAgentId(content: SurfaceContent): string | null {
  return content.kind === 'agent' ? content.agentId : null;
}

export function contentsEqual(a: SurfaceContent, b: SurfaceContent): boolean {
  return structurallyEqual(a, b);
}

// MARK: - Surface and Pane

/**
 * One tab. The ID is stable for the tab's lifetime (moving it between panes keeps it), so it is
 * what reservations, drag and drop, and `pu grid tab` address.
 */
export interface Surface {
  readonly id: number;
  readonly content: SurfaceContent;
}

/**
 * The content of one leaf of the split tree: an ordered stack of tabs, one of them active. A pane
 * always holds at least one tab; an "empty pane" is one whose only tab is `empty`.
 */
export interface Pane {
  readonly tabs: readonly Surface[];
  readonly activeTabId: number;
}

export function paneActiveTab(pane: Pane): Surface | null {
  return pane.tabs.find((tab) => tab.id === pane.activeTabId) ?? pane.tabs[0] ?? null;
}

export function paneActiveIndex(pane: Pane): number | null {
  return paneIndexOf(pane, pane.activeTabId);
}

export function paneIndexOf(pane: Pane, surfaceId: number): number | null {
  const index = pane.tabs.findIndex((tab) => tab.id === surfaceId);
  return index === -1 ? null : index;
}

/**
 * Remove a tab. When it was the active one, its left neighbour becomes active, or the new first
 * tab when it was leftmost.
 */
export function paneRemoving(
  pane: Pane,
  surfaceId: number,
): { pane: Pane; removed: Surface | null } {
  const draft = { tabs: [...pane.tabs], activeTabId: pane.activeTabId };
  const removed = removeFromPane(draft, surfaceId);
  return { pane: removed === null ? pane : draft, removed };
}

// MARK: - Workspace

export interface Workspace {
  readonly id: string;
  readonly container: WorkspaceContainer;
  readonly root: PaneSplitNode;
  /** Tab stacks by leaf ID. */
  readonly panes: Readonly<Partial<Record<number, Pane>>>;
  readonly focusedLeafId: number;
  readonly nextLeafId: number;
  readonly nextSurfaceId: number;
}

export interface WorkspaceInit {
  readonly id: string;
  readonly container: WorkspaceContainer;
  readonly root: PaneSplitNode;
  /** Leaves missing here get a single empty tab. */
  readonly panes?: Readonly<Partial<Record<number, Pane>>>;
  readonly focusedLeafId: number;
  readonly nextLeafId: number;
  readonly nextSurfaceId?: number;
}

/**
 * Swift's `Workspace.init`: leaves missing from `panes` get a single empty tab, so a caller can
 * describe just the geometry plus whichever panes it cares about. The result is normalized.
 */
export function makeWorkspace(init: WorkspaceInit): Workspace {
  return normalizeWorkspace({
    id: init.id,
    container: init.container,
    root: init.root,
    panes: init.panes ?? {},
    focusedLeafId: init.focusedLeafId,
    nextLeafId: init.nextLeafId,
    nextSurfaceId: init.nextSurfaceId ?? 0,
  });
}

/**
 * A brand-new single-pane workspace holding one agent. The ID is derived from the agent so it is
 * stable across launches without any random or time-based input: restarting can never invent a
 * different grouping. `id` overrides the derived ID when that one is already taken
 * (`adoptedWorkspaceId`).
 */
export function adoptingWorkspace(
  agentId: string,
  container: WorkspaceContainer,
  id = `ws-${agentId}`,
): Workspace {
  return makeWorkspace({
    id,
    container,
    root: leaf(0),
    panes: { 0: { tabs: [{ id: 0, content: agentContent(agentId) }], activeTabId: 0 } },
    focusedLeafId: 0,
    nextLeafId: 1,
    nextSurfaceId: 1,
  });
}

const POINT_GUARD_PREFIX = 'ws-point-guard:';

/** The Point Guard navigation item owns a root workspace for each open project. */
export function isPointGuard(workspace: Pick<Workspace, 'id'>): boolean {
  return workspace.id.startsWith(POINT_GUARD_PREFIX);
}

export function pointGuardId(projectRoot: string): string {
  return POINT_GUARD_PREFIX + projectRoot;
}

export function pointGuardWorkspace(projectRoot: string): Workspace {
  return makeWorkspace({
    id: pointGuardId(projectRoot),
    container: PROJECT_ROOT,
    root: leaf(0),
    panes: { 0: { tabs: [{ id: 0, content: POINT_GUARD_CONTENT }], activeTabId: 0 } },
    focusedLeafId: 0,
    nextLeafId: 1,
    nextSurfaceId: 1,
  });
}

/**
 * Closing the final tab returns the built-in workspace to chat. Counters keep growing so an
 * outstanding spawn can never bind to a newly created tab with a recycled ID.
 */
export function resetToPointGuard(workspace: Workspace): Workspace {
  const d = toDraft(workspace);
  draftResetToPointGuard(d);
  return fromDraft(d);
}

// MARK: - Queries

export interface LocatedSurface {
  readonly leafId: number;
  readonly surface: Surface;
}

export function paneCount(workspace: Workspace): number {
  return leafCount(workspace.root);
}

export function tabCount(workspace: Workspace): number {
  return paneList(workspace).reduce((sum, pane) => sum + pane.tabs.length, 0);
}

/** Every tab in layout order: panes in tree order, tabs in stack order. */
export function workspaceSurfaces(workspace: Workspace): LocatedSurface[] {
  return allLeafIds(workspace.root).flatMap((leafId) =>
    (workspace.panes[leafId]?.tabs ?? []).map((surface) => ({ leafId, surface })),
  );
}

/** Agent IDs occupying tabs, in layout order. */
export function workspaceAgentIds(workspace: Workspace): string[] {
  return workspaceSurfaces(workspace).flatMap(({ surface }) => {
    const agentId = contentAgentId(surface.content);
    return agentId === null ? [] : [agentId];
  });
}

/** The agent that names this workspace in the sidebar: the first tab holding one. */
export function primaryAgentId(workspace: Workspace): string | null {
  return workspaceAgentIds(workspace)[0] ?? null;
}

export function focusedPane(workspace: Workspace): Pane | null {
  return workspace.panes[workspace.focusedLeafId] ?? null;
}

export function focusedSurface(workspace: Workspace): Surface | null {
  const pane = focusedPane(workspace);
  return pane === null ? null : paneActiveTab(pane);
}

export function focusedAgentId(workspace: Workspace): string | null {
  const surface = focusedSurface(workspace);
  return surface === null ? null : contentAgentId(surface.content);
}

export function containsAgent(workspace: Workspace, agentId: string): boolean {
  return workspaceAgentIds(workspace).includes(agentId);
}

/**
 * Nothing the user opened is left: no agent tab and no file tab, only empty tabs (or none). Such
 * a workspace is a ghost row and goes away, unless a spawn is pending into one of its empty tabs,
 * which the caller knows and this type does not.
 */
export function isGhost(workspace: Workspace): boolean {
  if (isPointGuard(workspace)) return false;
  return !workspaceSurfaces(workspace).some(({ surface }) => surface.content.kind !== 'empty');
}

/** The pane holding a tab. */
export function leafIdOfSurface(workspace: Workspace, surfaceId: number): number | null {
  return (
    workspaceSurfaces(workspace).find(({ surface }) => surface.id === surfaceId)?.leafId ?? null
  );
}

export function surfaceById(workspace: Workspace, surfaceId: number): Surface | null {
  return (
    workspaceSurfaces(workspace).find(({ surface }) => surface.id === surfaceId)?.surface ?? null
  );
}

/** The tab holding an agent, and the pane it is in. */
export function locationOfAgent(
  workspace: Workspace,
  agentId: string,
): { leafId: number; surfaceId: number } | null {
  const found = workspaceSurfaces(workspace).find(
    ({ surface }) => contentAgentId(surface.content) === agentId,
  );
  return found === undefined ? null : { leafId: found.leafId, surfaceId: found.surface.id };
}

export function workspacesEqual(a: Workspace, b: Workspace): boolean {
  return structurallyEqual(a, b);
}

// MARK: - Pane mutations

/**
 * Split a pane. The new pane starts with one tab showing `content` and takes focus. `leafId` is
 * the new pane's leaf ID, or `null` when there is no room for another pane.
 */
export function splitPane(
  workspace: Workspace,
  leafId: number,
  axis: PaneAxis,
  content: SurfaceContent = EMPTY_CONTENT,
): { workspace: Workspace; leafId: number | null } {
  const d = toDraft(workspace);
  const newLeafId = draftSplit(d, leafId, axis, content);
  return { workspace: newLeafId === null ? workspace : fromDraft(d), leafId: newLeafId };
}

/**
 * Remove a pane and every tab in it. `agentIds` are the agents those tabs held, so the caller can
 * kill them. Removing the last pane leaves the workspace with no panes at all, a state that cannot
 * be shown, so the caller drops the workspace instead.
 */
export function closePane(
  workspace: Workspace,
  leafId: number,
): { workspace: Workspace; agentIds: string[] } {
  const d = toDraft(workspace);
  const agentIds = draftClosePane(d, leafId);
  return { workspace: fromDraft(d), agentIds };
}

export function setSplitRatio(
  workspace: Workspace,
  ratio: number,
  firstLeafIdOfSplit: number,
): Workspace {
  return { ...workspace, root: settingRatio(workspace.root, ratio, firstLeafIdOfSplit) };
}

export type FocusDirection = 'up' | 'down' | 'left' | 'right';

export function focusAxisAndDirection(direction: FocusDirection): [PaneAxis, boolean] {
  switch (direction) {
    case 'up':
      return ['horizontal', false];
    case 'down':
      return ['horizontal', true];
    case 'left':
      return ['vertical', false];
    case 'right':
      return ['vertical', true];
  }
}

export function moveFocus(workspace: Workspace, direction: FocusDirection): Workspace {
  const [axis, forward] = focusAxisAndDirection(direction);
  const adjacent = findAdjacentLeaf(workspace.root, workspace.focusedLeafId, axis, forward);
  return adjacent === null ? workspace : { ...workspace, focusedLeafId: adjacent };
}

// MARK: - Tab mutations

/** Open a tab right after the pane's active one. It becomes active and its pane takes focus. */
export function newTab(
  workspace: Workspace,
  leafId: number,
  content: SurfaceContent = EMPTY_CONTENT,
): { workspace: Workspace; surfaceId: number | null } {
  const d = toDraft(workspace);
  const pane = d.panes.get(leafId);
  if (pane === undefined) return { workspace, surfaceId: null };
  const surface = draftMakeSurface(d, content);
  const insertAt = (paneActiveIndex(pane) ?? pane.tabs.length - 1) + 1;
  pane.tabs.splice(insertAt, 0, surface);
  pane.activeTabId = surface.id;
  d.focusedLeafId = leafId;
  return { workspace: fromDraft(d), surfaceId: surface.id };
}

/** Show a tab and focus its pane. */
export function selectTab(workspace: Workspace, surfaceId: number): Workspace {
  const d = toDraft(workspace);
  draftSelectTab(d, surfaceId);
  return fromDraft(d);
}

/** Show the tab at a 0-based position in a pane. Out-of-range positions do nothing. */
export function selectTabAt(workspace: Workspace, leafId: number, index: number): Workspace {
  const tab = workspace.panes[leafId]?.tabs[index];
  return tab === undefined || index < 0 ? workspace : selectTab(workspace, tab.id);
}

export function selectLastTab(workspace: Workspace, leafId: number): Workspace {
  const last = workspace.panes[leafId]?.tabs.at(-1);
  return last === undefined ? workspace : selectTab(workspace, last.id);
}

/** Step through a pane's tabs, wrapping at either end. */
export function cycleTab(workspace: Workspace, leafId: number, offset: number): Workspace {
  const pane = workspace.panes[leafId];
  if (pane === undefined || pane.tabs.length === 0) return workspace;
  const current = paneActiveIndex(pane) ?? 0;
  const count = pane.tabs.length;
  const target = pane.tabs[(((current + offset) % count) + count) % count];
  return target === undefined ? workspace : selectTab(workspace, target.id);
}

/**
 * Close a tab. `agentIds` holds the agent it held so the caller can kill it. A pane whose last
 * tab closes is removed, exactly as if the pane itself had been closed.
 */
export function closeTab(
  workspace: Workspace,
  surfaceId: number,
): { workspace: Workspace; agentIds: string[] } {
  const d = toDraft(workspace);
  const leafId = draftLeafIdOfSurface(d, surfaceId);
  if (leafId === null) return { workspace, agentIds: [] };
  const pane = d.panes.get(leafId);
  if (pane?.tabs.length === 1) {
    const agentIds = draftClosePane(d, leafId);
    return { workspace: fromDraft(d), agentIds };
  }
  const removed = pane === undefined ? null : removeFromPane(pane, surfaceId);
  const agentId = removed === null ? null : contentAgentId(removed.content);
  return { workspace: fromDraft(d), agentIds: agentId === null ? [] : [agentId] };
}

/**
 * Move a tab to a 0-based position in another (or the same) pane. The tab keeps its ID and
 * content (an agent in it keeps running) and becomes active where it lands. A source pane left
 * with no tabs collapses.
 */
export function moveTab(
  workspace: Workspace,
  surfaceId: number,
  targetLeafId: number,
  index: number | null = null,
): Workspace {
  const d = toDraft(workspace);
  return draftMoveTab(d, surfaceId, targetLeafId, index) ? fromDraft(d) : workspace;
}

/**
 * Move a tab to just before another tab, in that tab's pane: what dropping one tab onto another
 * does. Accounts for the moved tab leaving an earlier slot in the same pane.
 */
export function moveTabBefore(
  workspace: Workspace,
  surfaceId: number,
  targetSurfaceId: number,
): Workspace {
  if (surfaceId === targetSurfaceId) return workspace;
  const targetLeafId = leafIdOfSurface(workspace, targetSurfaceId);
  if (targetLeafId === null) return workspace;
  const targetPane = workspace.panes[targetLeafId];
  let index = targetPane === undefined ? null : paneIndexOf(targetPane, targetSurfaceId);
  if (targetPane === undefined || index === null) return workspace;
  if (leafIdOfSurface(workspace, surfaceId) === targetLeafId) {
    const from = paneIndexOf(targetPane, surfaceId);
    if (from !== null && from < index) index -= 1;
  }
  return moveTab(workspace, surfaceId, targetLeafId, index);
}

/**
 * Move a tab out of its pane into a new pane split off beside it (tmux's break-pane, within the
 * workspace). Does nothing for a pane's only tab or when the grid is full. `leafId` is the new
 * pane's leaf ID.
 */
export function breakTab(
  workspace: Workspace,
  surfaceId: number,
  axis: PaneAxis,
): { workspace: Workspace; leafId: number | null } {
  const d = toDraft(workspace);
  const sourceLeafId = draftLeafIdOfSurface(d, surfaceId);
  const source = sourceLeafId === null ? undefined : d.panes.get(sourceLeafId);
  if (sourceLeafId === null || source === undefined || source.tabs.length <= 1) {
    return { workspace, leafId: null };
  }
  if (!canSplit(d.root)) return { workspace, leafId: null };
  const splitResult = splittingLeaf(d.root, sourceLeafId, axis, d.nextLeafId);
  d.root = splitResult.root;
  d.nextLeafId = splitResult.nextId;
  const newLeafId = d.nextLeafId - 1;
  const surface = removeFromPane(source, surfaceId);
  if (surface === null) return { workspace, leafId: null };
  d.panes.set(newLeafId, { tabs: [surface], activeTabId: surface.id });
  d.focusedLeafId = newLeafId;
  return { workspace: fromDraft(d), leafId: newLeafId };
}

/** Change what a tab shows: a reservation being fulfilled, or the palette's choice. */
export function setSurfaceContent(
  workspace: Workspace,
  content: SurfaceContent,
  surfaceId: number,
): Workspace {
  const d = toDraft(workspace);
  return draftSetContent(d, content, surfaceId) ? fromDraft(d) : workspace;
}

/**
 * Remove every tab matching `predicate`, collapsing panes left empty. `survived` is `false` when
 * no pane survives, in which case the workspace must be dropped (or, for Point Guard, reset).
 */
export function removeSurfaces(
  workspace: Workspace,
  predicate: (surface: Surface) => boolean,
): { workspace: Workspace; survived: boolean } {
  const d = toDraft(workspace);
  const survived = draftRemoveSurfaces(d, predicate);
  return { workspace: fromDraft(d), survived };
}

// MARK: - Normalize

/** Re-establish internal consistency after any structural edit. */
export function normalizeWorkspace(workspace: Workspace): Workspace {
  const d = toDraft(workspace);
  draftNormalize(d);
  return fromDraft(d);
}

// MARK: - Drafts (private mutable mirrors of the immutable types)

interface DraftPane {
  tabs: Surface[];
  activeTabId: number;
}

interface Draft {
  id: string;
  container: WorkspaceContainer;
  root: PaneSplitNode;
  panes: Map<number, DraftPane>;
  focusedLeafId: number;
  nextLeafId: number;
  nextSurfaceId: number;
}

function paneList(workspace: Workspace): Pane[] {
  return Object.values(workspace.panes).filter((pane): pane is Pane => pane !== undefined);
}

function toDraft(workspace: Workspace): Draft {
  const panes = new Map<number, DraftPane>();
  for (const [key, pane] of Object.entries(workspace.panes)) {
    if (pane !== undefined)
      panes.set(Number(key), { tabs: [...pane.tabs], activeTabId: pane.activeTabId });
  }
  return {
    id: workspace.id,
    container: workspace.container,
    root: workspace.root,
    panes,
    focusedLeafId: workspace.focusedLeafId,
    nextLeafId: workspace.nextLeafId,
    nextSurfaceId: workspace.nextSurfaceId,
  };
}

function fromDraft(d: Draft): Workspace {
  const panes: Record<number, Pane> = {};
  for (const leafId of [...d.panes.keys()].sort((a, b) => a - b)) {
    const pane = d.panes.get(leafId);
    if (pane !== undefined) panes[leafId] = { tabs: [...pane.tabs], activeTabId: pane.activeTabId };
  }
  return {
    id: d.id,
    container: d.container,
    root: d.root,
    panes,
    focusedLeafId: d.focusedLeafId,
    nextLeafId: d.nextLeafId,
    nextSurfaceId: d.nextSurfaceId,
  };
}

function removeFromPane(pane: DraftPane, surfaceId: number): Surface | null {
  const index = pane.tabs.findIndex((tab) => tab.id === surfaceId);
  if (index === -1) return null;
  const [removed] = pane.tabs.splice(index, 1);
  if (pane.activeTabId === surfaceId) {
    const neighbour = pane.tabs[Math.max(0, index - 1)];
    if (neighbour !== undefined) pane.activeTabId = neighbour.id;
  }
  return removed ?? null;
}

function draftSurfaces(d: Draft): LocatedSurface[] {
  return allLeafIds(d.root).flatMap((leafId) =>
    (d.panes.get(leafId)?.tabs ?? []).map((surface) => ({ leafId, surface })),
  );
}

function draftLeafIdOfSurface(d: Draft, surfaceId: number): number | null {
  return draftSurfaces(d).find(({ surface }) => surface.id === surfaceId)?.leafId ?? null;
}

function draftMakeSurface(d: Draft, content: SurfaceContent): Surface {
  const surface = { id: d.nextSurfaceId, content };
  d.nextSurfaceId += 1;
  return surface;
}

function draftResetToPointGuard(d: Draft): void {
  const leafId = d.nextLeafId;
  d.nextLeafId += 1;
  d.root = leaf(leafId);
  const surface = draftMakeSurface(d, POINT_GUARD_CONTENT);
  d.panes = new Map([[leafId, { tabs: [surface], activeTabId: surface.id }]]);
  d.focusedLeafId = leafId;
  d.container = PROJECT_ROOT;
}

function draftSplit(
  d: Draft,
  leafId: number,
  axis: PaneAxis,
  content: SurfaceContent,
): number | null {
  if (!canSplit(d.root) || !d.panes.has(leafId)) return null;
  const result = splittingLeaf(d.root, leafId, axis, d.nextLeafId);
  d.root = result.root;
  d.nextLeafId = result.nextId;
  const newLeafId = d.nextLeafId - 1;
  const surface = draftMakeSurface(d, content);
  d.panes.set(newLeafId, { tabs: [surface], activeTabId: surface.id });
  d.focusedLeafId = newLeafId;
  return newLeafId;
}

function draftClosePane(d: Draft, leafId: number): string[] {
  const pane = d.panes.get(leafId);
  if (pane === undefined) return [];
  d.panes.delete(leafId);
  const sibling = siblingLeafId(d.root, leafId);
  const newRoot = removingLeaf(d.root, leafId);
  if (newRoot !== null) {
    d.root = newRoot;
    if (d.focusedLeafId === leafId) d.focusedLeafId = sibling ?? firstLeafId(d.root);
  }
  return pane.tabs.flatMap((tab) => {
    const agentId = contentAgentId(tab.content);
    return agentId === null ? [] : [agentId];
  });
}

function draftSelectTab(d: Draft, surfaceId: number): void {
  const leafId = draftLeafIdOfSurface(d, surfaceId);
  if (leafId === null) return;
  const pane = d.panes.get(leafId);
  if (pane !== undefined) pane.activeTabId = surfaceId;
  d.focusedLeafId = leafId;
}

function draftMoveTab(
  d: Draft,
  surfaceId: number,
  targetLeafId: number,
  index: number | null,
): boolean {
  const sourceLeafId = draftLeafIdOfSurface(d, surfaceId);
  if (sourceLeafId === null) return false;
  const target = d.panes.get(targetLeafId);
  const source = d.panes.get(sourceLeafId);
  if (target === undefined || source === undefined) return false;
  const surface = removeFromPane(source, surfaceId);
  if (surface === null) return false;

  const insertAt = Math.min(Math.max(0, index ?? target.tabs.length), target.tabs.length);
  target.tabs.splice(insertAt, 0, surface);
  target.activeTabId = surface.id;

  if (source.tabs.length === 0) {
    d.panes.delete(sourceLeafId);
    d.root = removingLeaf(d.root, sourceLeafId) ?? d.root;
  }
  d.focusedLeafId = targetLeafId;
  return true;
}

function draftSetContent(d: Draft, content: SurfaceContent, surfaceId: number): boolean {
  const leafId = draftLeafIdOfSurface(d, surfaceId);
  const pane = leafId === null ? undefined : d.panes.get(leafId);
  const index = pane?.tabs.findIndex((tab) => tab.id === surfaceId) ?? -1;
  if (pane === undefined || index === -1) return false;
  pane.tabs[index] = { id: surfaceId, content };
  return true;
}

function draftRemoveSurfaces(d: Draft, predicate: (surface: Surface) => boolean): boolean {
  const emptied = new Set<number>();
  for (const [leafId, pane] of d.panes) {
    const doomed = pane.tabs.filter(predicate).map((tab) => tab.id);
    if (doomed.length === 0) continue;
    for (const surfaceId of doomed) removeFromPane(pane, surfaceId);
    if (pane.tabs.length === 0) emptied.add(leafId);
  }
  if (emptied.size === 0) return true;
  for (const leafId of emptied) d.panes.delete(leafId);
  const pruned = removingLeaves(d.root, emptied);
  if (pruned === null) return false;
  d.root = pruned;
  return true;
}

function draftNormalize(d: Draft): void {
  if (isPointGuard(d)) d.container = PROJECT_ROOT;
  const ids = allLeafIds(d.root);
  const live = new Set(ids);
  for (const leafId of [...d.panes.keys()]) {
    if (!live.has(leafId)) d.panes.delete(leafId);
  }

  // Surface IDs must be unique across the workspace; a duplicate (hand-edited or corrupt file)
  // gets a fresh ID rather than silently aliasing another tab.
  const maxId = Math.max(
    -1,
    ...[...d.panes.values()].flatMap((pane) => pane.tabs.map((tab) => tab.id)),
  );
  d.nextSurfaceId = Math.max(d.nextSurfaceId, maxId + 1);
  const seen = new Set<number>();
  for (const leafId of ids) {
    const pane = d.panes.get(leafId) ?? { tabs: [], activeTabId: -1 };
    pane.tabs.forEach((tab, index) => {
      if (!seen.has(tab.id)) {
        seen.add(tab.id);
        return;
      }
      const wasActive = pane.activeTabId === tab.id;
      pane.tabs[index] = { id: d.nextSurfaceId, content: tab.content };
      seen.add(d.nextSurfaceId);
      if (wasActive) pane.activeTabId = d.nextSurfaceId;
      d.nextSurfaceId += 1;
    });
    if (pane.tabs.length === 0) {
      pane.tabs = [{ id: d.nextSurfaceId, content: EMPTY_CONTENT }];
      seen.add(d.nextSurfaceId);
      d.nextSurfaceId += 1;
    }
    const first = pane.tabs[0];
    if (!pane.tabs.some((tab) => tab.id === pane.activeTabId) && first !== undefined) {
      pane.activeTabId = first.id;
    }
    d.panes.set(leafId, pane);
  }

  if (!live.has(d.focusedLeafId)) d.focusedLeafId = ids[0] ?? 0;
  d.nextLeafId = Math.max(d.nextLeafId, Math.max(-1, ...ids) + 1);
}

// MARK: - LiveAgent

/** One live agent as the manifest reports it. */
export interface LiveAgent {
  readonly id: string;
  readonly container: WorkspaceContainer;
  /** The tab the agent was spawned for, as recorded on its manifest entry. */
  readonly placement: SpawnPlacement | null;
}

export function liveAgent(
  id: string,
  container: WorkspaceContainer,
  placement: SpawnPlacement | null = null,
): LiveAgent {
  return { id, container, placement };
}

/**
 * A Point Guard workspace's ID embeds the project key of the app that made it, and every app
 * re-keys it to its own on load. A placement another machine recorded names that machine's ID, so
 * it is re-keyed the same way.
 */
export function placementRekeyed(agent: LiveAgent, projectRoot: string): LiveAgent {
  if (agent.placement === null || !agent.placement.workspace.startsWith(POINT_GUARD_PREFIX)) {
    return agent;
  }
  return {
    ...agent,
    placement: { workspace: pointGuardId(projectRoot), surface: agent.placement.surface },
  };
}

/**
 * Flatten a project's manifest into the ordered agent list the reconciler expects. Root agents
 * first, then each worktree's agents, in the given order throughout.
 */
export function liveAgents(
  rootAgents: readonly AgentModel[],
  worktrees: readonly WorktreeModel[],
): LiveAgent[] {
  return [
    ...rootAgents.map((agent) => liveAgent(agent.id, PROJECT_ROOT, agent.placement)),
    ...worktrees.flatMap((worktree) =>
      worktree.agents.map((agent) =>
        liveAgent(agent.id, worktreeContainer(worktree.id), agent.placement),
      ),
    ),
  ];
}

// MARK: - Layout merge

/**
 * Folds a layout document another machine wrote into this app's layout, one workspace at a time.
 *
 * Two apps write the same `.pu/workspaces.json` and every save is conditional, so a save that
 * finds the document changed has to take the other app's edits without dropping its own. `base`
 * is the document as this app last read or wrote it; their changes are measured against it. Ours
 * are measured against `oursBase` (by default the same), which a caller can set to the base as its
 * own reconciler shows it, so grouping derived from the manifest is not an edit. A workspace this
 * app changed or added keeps this app's version (also when the other app changed it too), and
 * everything else takes theirs.
 */
export function mergeWorkspaceLayouts(
  base: readonly Workspace[],
  ours: readonly Workspace[],
  theirs: readonly Workspace[],
  oursBase: readonly Workspace[] | null = null,
): Workspace[] {
  const baseById = firstById(base);
  const oursBaseById = firstById(oursBase ?? base);
  const oursById = firstById(ours);
  const theirIds = new Set(theirs.map((workspace) => workspace.id));
  const differs = (workspace: Workspace, other: Workspace | undefined): boolean =>
    other === undefined || !workspacesEqual(workspace, other);

  const result: Workspace[] = [];
  for (const their of theirs) {
    const mine = oursById.get(their.id);
    if (mine !== undefined) {
      result.push(differs(mine, oursBaseById.get(their.id)) ? mine : their);
    } else if (differs(their, baseById.get(their.id))) {
      result.push(their); // removed here, changed there: keep it
    }
  }
  // Added here, or changed here and removed there.
  result.push(
    ...ours.filter(
      (workspace) =>
        !theirIds.has(workspace.id) && differs(workspace, oursBaseById.get(workspace.id)),
    ),
  );
  return result;
}

function firstById(workspaces: readonly Workspace[]): Map<string, Workspace> {
  const byId = new Map<string, Workspace>();
  for (const workspace of workspaces) {
    if (!byId.has(workspace.id)) byId.set(workspace.id, workspace);
  }
  return byId;
}

// MARK: - Reconciler

function placementKey(placement: SpawnPlacement): string {
  return `${String(placement.surface)}\u0000${placement.workspace}`;
}

/**
 * The single function that turns (stored layout, manifest) into the canonical workspace list.
 *
 * It is pure, total, and order-stable: the same inputs always yield the same output, so a restart
 * reproduces exactly the grouping the user last saw. Nothing else in the app may construct or
 * filter the sidebar's row set.
 *
 * Guarantees on the returned list:
 * 1. Every live agent occupies exactly one tab of exactly one workspace.
 * 2. No tab references an agent that is no longer in the manifest.
 * 3. Every workspace holds a live agent or a file tab, or is waiting on a spawn (`reserved`):
 *    no ghost rows.
 * 4. Each workspace's container matches where its agents actually live.
 * 5. An agent no tab holds yet goes into the empty tab it was spawned for, when that tab is still
 *    there, so every app showing the project places it the same way.
 */
export function reconcileWorkspaces(
  stored: readonly Workspace[],
  live: readonly LiveAgent[],
  reserved: ReadonlySet<string> = new Set(),
): Workspace[] {
  const containerByAgent = new Map<string, WorkspaceContainer>();
  for (const agent of live) {
    if (!containerByAgent.has(agent.id)) containerByAgent.set(agent.id, agent.container);
  }
  const storedAgentIds = new Set(stored.flatMap(workspaceAgentIds));
  const agentByPlacement = new Map<string, LiveAgent>();
  for (const agent of live) {
    if (storedAgentIds.has(agent.id) || agent.placement === null) continue;
    const key = placementKey(agent.placement);
    if (!agentByPlacement.has(key)) agentByPlacement.set(key, agent);
  }
  const placementWorkspaceIds = new Set(
    [...agentByPlacement.values()].map((agent) => agent.placement?.workspace),
  );

  const claimed = new Set<string>();
  const result: Workspace[] = [];
  const seenWorkspaceIds = new Set<string>();

  for (const original of stored) {
    // A duplicated workspace ID would make selection ambiguous; the first wins.
    if (seenWorkspaceIds.has(original.id)) continue;
    seenWorkspaceIds.add(original.id);
    const d = toDraft(original);
    const pointGuard = isPointGuard(d);

    // Drop tabs whose agent died, and tabs claiming an agent another tab already holds. Empty and
    // file tabs are deliberate and stay.
    const staleSurfaceIds = new Set<number>();
    for (const { surface } of draftSurfaces(d)) {
      const agentId = contentAgentId(surface.content);
      if (agentId === null) continue;
      const container = containerByAgent.get(agentId);
      if (
        container === undefined ||
        (pointGuard && container.kind !== 'projectRoot') ||
        claimed.has(agentId)
      ) {
        staleSurfaceIds.add(surface.id);
      } else {
        claimed.add(agentId);
      }
    }

    if (staleSurfaceIds.size > 0 && !draftRemoveSurfaces(d, (s) => staleSurfaceIds.has(s.id))) {
      if (!pointGuard) continue; // every pane went away; the workspace goes with it
      draftResetToPointGuard(d);
    }

    if (placementWorkspaceIds.has(d.id)) {
      for (const { surface } of draftSurfaces(d)) {
        if (surface.content.kind !== 'empty') continue;
        const agent = agentByPlacement.get(placementKey({ workspace: d.id, surface: surface.id }));
        if (agent === undefined || claimed.has(agent.id)) continue;
        const agentIds = draftAgentIds(d);
        if (agentIds.length > 0 && !containersEqual(d.container, agent.container)) continue;
        if (pointGuard && agent.container.kind !== 'projectRoot') continue;
        draftSetContent(d, agentContent(agent.id), surface.id);
        claimed.add(agent.id);
      }
    }

    // A workspace exists to hold what the user opened. One left with only empty tabs is a ghost
    // row, unless one of those tabs is waiting for its agent.
    const workspace = fromDraft(d);
    if (isGhost(workspace) && !reserved.has(workspace.id)) continue;

    const primary = draftAgentIds(d)[0];
    if (primary !== undefined) d.container = containerByAgent.get(primary) ?? d.container;
    draftNormalize(d);
    result.push(fromDraft(d));
  }

  // Any live agent no tab claimed becomes its own single-pane workspace. This is the step that
  // makes a loose sidebar item impossible: an agent the layout forgot still surfaces as a
  // workspace, never as a bare agent row.
  const takenIds = new Set(result.map((workspace) => workspace.id));
  for (const agent of live) {
    if (claimed.has(agent.id)) continue;
    claimed.add(agent.id);
    const id = adoptedWorkspaceId(agent.id, takenIds);
    takenIds.add(id);
    result.push(adoptingWorkspace(agent.id, agent.container, id));
  }

  return result;
}

/**
 * The ID for a workspace adopting `agentId`: `ws-<agentId>`, or when a kept workspace already uses
 * that ID (it once adopted the agent, which has since moved out of it), the first free one of
 * `ws-<agentId>-2`, `-3`, ... Swift reuses `ws-<agentId>` and so publishes two workspaces with one
 * ID (WIN-6.2 fix). Deterministic: the same kept IDs always give the same answer, and the adopted
 * workspace keeps its ID on the next reconcile because it then holds the agent.
 */
export function adoptedWorkspaceId(agentId: string, takenIds: ReadonlySet<string>): string {
  const base = `ws-${agentId}`;
  if (!takenIds.has(base)) return base;
  let suffix = 2;
  while (takenIds.has(`${base}-${String(suffix)}`)) suffix += 1;
  return `${base}-${String(suffix)}`;
}

function draftAgentIds(d: Draft): string[] {
  return draftSurfaces(d).flatMap(({ surface }) => {
    const agentId = contentAgentId(surface.content);
    return agentId === null ? [] : [agentId];
  });
}
