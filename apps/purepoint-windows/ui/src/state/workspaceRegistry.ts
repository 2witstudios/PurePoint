// Port of apps/purepoint-macos/purepoint-macos/State/WorkspaceRegistry.swift (WIN-6.2).
//
// The single source of truth for pane grouping across every open project. The sidebar, the detail
// view and the menu commands all read workspaces from here and nowhere else. There is no second
// list of agents to filter, hide or subtract: the row set *is* `workspaces(forProject)`, so a pane
// and a loose sidebar item for the same agent cannot coexist. Reconciling is the only writer of
// that list.
//
// Projects are keyed by their project key: the bare root for local projects,
// `machine:<uuid>:<root>` for remote ones.
//
// Shape: the observable state (what Swift's `@Observable` publishes) lives in a Zustand store,
// `registry.store`, that views subscribe to with `useStore`. Everything else Swift marks
// `@ObservationIgnored` (reservations, the retained manifest, save bookkeeping) stays private to
// the `WorkspaceRegistry` object. The decisions are the pure functions exported below and in
// `model/workspace.ts`; the class sequences them and performs the side effects (persistence through
// the injected `WorkspacePersistence`, the save debounce, killing agents).

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { GridCommand } from '../protocol';
import type { PaneAxis } from '../model/paneSplit';
import { allLeafIds } from '../model/paneSplit';
import {
  EMPTY_CONTENT,
  POINT_GUARD_CONTENT,
  agentContent,
  breakTab,
  closePane,
  closeTab,
  containsAgent,
  contentAgentId,
  cycleTab,
  dropTabOnEdge,
  fileContent,
  focusedAgentId,
  focusedSurface,
  isGhost,
  isPointGuard,
  leafIdOfSurface,
  liveAgent,
  liveAgents,
  mergeWorkspaceLayouts,
  moveFocus,
  moveTab,
  moveTabBefore,
  newTab,
  normalizeWorkspace,
  paneActiveTab,
  placementRekeyed,
  pointGuardId,
  pointGuardWorkspace,
  reconcileWorkspaces,
  removeSurfaces,
  resetToPointGuard,
  selectLastTab,
  selectTab,
  selectTabAt,
  setSplitRatio,
  setSurfaceContent,
  splitPane,
  workspaceAgentIds,
  workspaceSurfaces,
  workspacesEqual,
  type FocusDirection,
  type LiveAgent,
  type PaneEdge,
  type SurfaceContent,
  type Workspace,
  type WorkspaceContainer,
} from '../model/workspace';
import { structurallyEqual, type AgentModel, type WorktreeModel } from '../model/workspaceModel';
import type { WorkspacePersistence } from '../model/workspacePersistenceTypes';
import { FileTabStore, type FileTabKey } from './fileTabStore';

// MARK: - Observable state

export type WorkspacesByProject = Readonly<Record<string, readonly Workspace[]>>;

export interface WorkspaceRegistryState {
  /** Canonical workspaces per project key, in sidebar order. */
  readonly workspacesByProject: WorkspacesByProject;
  /**
   * The workspace whose grid is on screen. Persisted with its project's layout so `pu grid show`
   * can say which workspace commands without `--workspace` will reach.
   */
  readonly activeWorkspaceId: string | null;
  /** Set after a UI-initiated split or new tab so that empty tab opens the command palette. */
  readonly pendingPaletteSurfaceId: number | null;
  readonly layoutErrorsByProject: Readonly<Record<string, string>>;
}

// MARK: - Pure helpers

/**
 * A tab that has asked the daemon for an agent and is waiting for it to appear.
 *
 * Reservations are what make spawning deterministic. The tab records its claim *before* the daemon
 * writes the manifest, so when the new agent shows up the reconciler places it in that tab
 * directly. It is never briefly adopted into a workspace of its own, which is how a freshly split
 * pane used to flash as a second sidebar row.
 */
export interface SurfaceReservation {
  readonly workspaceId: string;
  readonly surfaceId: number;
  readonly container: WorkspaceContainer;
}

const REMOTE_PREFIX = 'machine:';
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** Swift `ProjectKey(id:).isRemote`: a well-formed `machine:<uuid>:<root>` key. */
export function isRemoteProjectKey(id: string): boolean {
  if (!id.startsWith(REMOTE_PREFIX)) return false;
  const rest = id.slice(REMOTE_PREFIX.length);
  const colon = rest.indexOf(':');
  return colon !== -1 && UUID.test(rest.slice(0, colon));
}

function containerKey(container: WorkspaceContainer): string {
  return container.kind === 'worktree' ? `worktree:${container.worktreeId}` : 'root';
}

/**
 * Hold back agents that no tab has claimed while a tab in their container is waiting for its spawn
 * response. Which of them belongs to the reservation is only known once the response binds an ID,
 * so adopting any of them now could hand the pane to an unrelated agent or surface the real one as
 * a workspace of its own.
 */
export function deferringReservedAgents(
  reservations: readonly SurfaceReservation[],
  stored: readonly Workspace[],
  live: readonly LiveAgent[],
): LiveAgent[] {
  if (reservations.length === 0) return [...live];
  const reservedContainers = new Set(reservations.map((r) => containerKey(r.container)));
  const placed = new Set(stored.flatMap(workspaceAgentIds));
  return live.filter(
    (agent) => placed.has(agent.id) || !reservedContainers.has(containerKey(agent.container)),
  );
}

/** `live` plus the agents an adopted document placed before the manifest listed them. */
export function withUnconfirmed(
  live: readonly LiveAgent[],
  unconfirmed: readonly LiveAgent[],
): LiveAgent[] {
  const known = new Set(live.map((agent) => agent.id));
  return [...live, ...unconfirmed.filter((agent) => !known.has(agent.id))];
}

/** Every file tab in the layout: the sessions `FileTabStore` keeps. */
export function fileTabKeys(workspacesByProject: WorkspacesByProject): FileTabKey[] {
  return Object.values(workspacesByProject).flatMap((list) =>
    list.flatMap((workspace) =>
      workspaceSurfaces(workspace)
        .filter(({ surface }) => surface.content.kind === 'file')
        .map(({ surface }) => ({ workspaceId: workspace.id, surfaceId: surface.id })),
    ),
  );
}

/**
 * Bind an agent to a tab across a project's workspaces: the binding wins over any other tab still
 * claiming the same agent, and a workspace left with no pane by that goes away.
 */
export function bindingAgent(
  list: readonly Workspace[],
  agentId: string,
  workspaceId: string,
  surfaceId: number,
): Workspace[] {
  const stored: Workspace[] = [];
  for (const original of list) {
    let workspace = original;
    if (workspace.id === workspaceId) {
      workspace = removeSurfaces(
        workspace,
        (s) => s.id !== surfaceId && contentAgentId(s.content) === agentId,
      ).workspace;
      workspace = setSurfaceContent(workspace, agentContent(agentId), surfaceId);
    } else if (containsAgent(workspace, agentId)) {
      const result = removeSurfaces(workspace, (s) => contentAgentId(s.content) === agentId);
      if (!result.survived) continue;
      workspace = result.workspace;
    }
    stored.push(normalizeWorkspace(workspace));
  }
  return stored;
}

/** Swift `GridCommand` `axis`: `h` is a top/bottom split, anything else left/right. */
function gridAxis(value: string | undefined): PaneAxis {
  return (value ?? 'v') === 'h' ? 'horizontal' : 'vertical';
}

function gridDirection(value: string): FocusDirection {
  switch (value) {
    case 'up':
      return 'up';
    case 'down':
      return 'down';
    case 'left':
      return 'left';
    default:
      return 'right';
  }
}

/**
 * `record` with `key` set to `value` (in place, keeping key order; new keys go last), or removed
 * when `value` is `null`.
 */
function withEntry<T>(
  record: Readonly<Record<string, T>>,
  key: string,
  value: T | null,
): Record<string, T> {
  const entries: [string, T][] = [];
  let found = false;
  for (const [existing, current] of Object.entries(record)) {
    if (existing !== key) entries.push([existing, current]);
    else if (value !== null) {
      found = true;
      entries.push([key, value]);
    }
  }
  if (!found && value !== null) entries.push([key, value]);
  return Object.fromEntries(entries);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Thrown while a remote project's layout is still being read through its host. */
class RemoteLayoutPending extends Error {
  constructor() {
    super('remote layout pending');
  }
}

// MARK: - Save scheduling

/** The save debounce (Swift coalesces writes for one second). */
export const SAVE_DEBOUNCE_MS = 1000;

/** Timer seam for the save debounce; tests pass one that never fires. */
export interface SaveScheduler {
  schedule(run: () => void, delayMs: number): unknown;
  cancel(handle: unknown): void;
}

const TIMER_SCHEDULER: SaveScheduler = {
  schedule: (run, delayMs) => setTimeout(run, delayMs),
  cancel: (handle) => {
    clearTimeout(handle as ReturnType<typeof setTimeout>);
  },
};

/** Swift `DispatchWorkItem`: `perform` runs the body even after `cancel`. */
interface SaveWorkItem {
  perform(): void;
  cancel(): void;
}

export type RemoteSaver = (
  workspaces: readonly Workspace[],
  activeWorkspaceId: string | null,
) => void;

export interface WorkspaceRegistryOptions {
  readonly persistence: WorkspacePersistence;
  /** Defaults to `persistence.loadForRegistry`. */
  readonly loadWorkspaces?: (projectRoot: string) => Workspace[];
  readonly scheduler?: SaveScheduler;
}

// MARK: - Registry

export class WorkspaceRegistry {
  readonly store: StoreApi<WorkspaceRegistryState>;
  /** Editor and navigator state of every open file tab, kept while the tab is off screen. */
  readonly fileTabs = new FileTabStore();
  /** Invoked with (projectRoot, agentId) when a tab holding an agent is closed. */
  onCloseAgent: ((projectRoot: string, agentId: string) => void) | null = null;

  private readonly persistence: WorkspacePersistence;
  private readonly loadWorkspaces: (projectRoot: string) => Workspace[];
  private readonly scheduler: SaveScheduler;

  private readonly reservationsByProject = new Map<string, SurfaceReservation[]>();
  /**
   * The latest live-agent list each project's manifest produced, kept so a pane binding can
   * reconcile immediately instead of waiting for the next manifest change.
   */
  private readonly liveByProject = new Map<string, LiveAgent[]>();
  private readonly loadedProjects = new Set<string>();
  private readonly saveWorkItems = new Map<string, SaveWorkItem>();
  /**
   * Projects with a layout change not yet written: only these are flushed at quit, so a remote
   * layout another machine changed meanwhile is not rewritten from a stale copy.
   */
  private readonly pendingSaves = new Set<string>();
  /** Remote projects persist through their machine's daemon rather than the local disk. */
  private readonly remoteSavers = new Map<string, RemoteSaver>();
  /**
   * Modification time of each loaded local project's layout document as this app last read or
   * wrote it (`null`: there was none). Another machine reaching this one over SSH writes the same
   * file, so a different time on disk is a change to adopt, never to overwrite.
   */
  private readonly localLayoutStamps = new Map<string, number | null>();
  /**
   * Each project's layout document as this app last read or wrote it, decoded: the base for
   * merging a document another machine wrote.
   */
  private readonly layoutBases = new Map<string, Workspace[]>();
  /**
   * Agents a document adopted from another machine places in tabs before this app's manifest lists
   * them. They count as live until the next manifest, so their tabs (and the split holding them)
   * are not dropped as dead in the meantime.
   */
  private readonly unconfirmedAgents = new Map<string, LiveAgent[]>();

  constructor(options: WorkspaceRegistryOptions) {
    this.persistence = options.persistence;
    this.loadWorkspaces =
      options.loadWorkspaces ?? ((root) => options.persistence.loadForRegistry(root));
    this.scheduler = options.scheduler ?? TIMER_SCHEDULER;
    this.store = createStore<WorkspaceRegistryState>()(() => ({
      workspacesByProject: {},
      activeWorkspaceId: null,
      pendingPaletteSurfaceId: null,
      layoutErrorsByProject: {},
    }));
  }

  // MARK: Observable state accessors

  get workspacesByProject(): WorkspacesByProject {
    return this.store.getState().workspacesByProject;
  }

  get layoutErrorsByProject(): Readonly<Record<string, string>> {
    return this.store.getState().layoutErrorsByProject;
  }

  get activeWorkspaceId(): string | null {
    return this.store.getState().activeWorkspaceId;
  }

  set activeWorkspaceId(id: string | null) {
    const old = this.activeWorkspaceId;
    if (id === old) return;
    this.store.setState({ activeWorkspaceId: id });
    for (const changed of [old, id]) {
      if (changed === null) continue;
      const root = this.projectRoot(changed);
      if (root !== null) this.scheduleSave(root);
    }
  }

  get pendingPaletteSurfaceId(): number | null {
    return this.store.getState().pendingPaletteSurfaceId;
  }

  set pendingPaletteSurfaceId(id: number | null) {
    this.store.setState({ pendingPaletteSurfaceId: id });
  }

  /** The setter as a method, for views that may not assign to a prop's fields. */
  markPendingPalette(surfaceId: number | null): void {
    this.pendingPaletteSurfaceId = surfaceId;
  }

  private setWorkspaces(projectRoot: string, list: readonly Workspace[] | null): void {
    const next = withEntry(this.workspacesByProject, projectRoot, list);
    this.store.setState({ workspacesByProject: next });
    this.fileTabs.retainOnly(fileTabKeys(next));
  }

  private setLayoutError(projectRoot: string, message: string | null): void {
    const current = this.layoutErrorsByProject;
    if (message === null && !Object.hasOwn(current, projectRoot)) return;
    this.store.setState({ layoutErrorsByProject: withEntry(current, projectRoot, message) });
  }

  private isLoaded(projectRoot: string): boolean {
    return this.loadedProjects.has(projectRoot);
  }

  /**
   * A remote project's layout is read asynchronously through its host; until it arrives (see
   * `registerRemoteProject`) nothing is saved, so no layout is invented and written.
   */
  private isAwaitingRemoteLayout(projectRoot: string): boolean {
    return isRemoteProjectKey(projectRoot) && !this.isLoaded(projectRoot);
  }

  // MARK: Remote and external layouts

  /** Adopt a remote project's restored layout and route its saves through `save`. */
  registerRemoteProject(projectId: string, restored: Workspace[], save: RemoteSaver): void {
    this.remoteSavers.set(projectId, save);
    if (this.isLoaded(projectId)) return;
    this.loadedProjects.add(projectId);
    this.setLayoutError(projectId, null);
    this.setWorkspaces(projectId, restored);
    this.layoutBases.set(projectId, restored);
    const live = this.liveByProject.get(projectId);
    if (live !== undefined) this.publish(projectId, restored, live);
  }

  /**
   * The project's layout document changed under this app (a conditional save hit a conflict, or
   * another machine wrote this one's file): merge it with this app's layout instead of overwriting
   * it, fold in the live agents, and write back whatever this app adds.
   */
  adoptExternalLayout(projectId: string, workspaces: Workspace[]): void {
    if (!this.isLoaded(projectId)) return;
    this.saveWorkItems.get(projectId)?.cancel();
    this.saveWorkItems.delete(projectId);
    this.pendingSaves.delete(projectId);
    // Ours are measured against the base as the reconciler would show it now: the grouping it
    // derives from the manifest comes out the same everywhere, so it is not an edit here. `ours`
    // was published with the unconfirmed agents, so the base is reconciled with them too.
    const base = this.layoutBases.get(projectId) ?? [];
    const live = this.liveByProject.get(projectId);
    const merged = mergeWorkspaceLayouts(
      base,
      this.decoded(this.workspaces(projectId), projectId),
      workspaces,
      live === undefined
        ? null
        : this.decoded(reconcileWorkspaces(base, this.withUnconfirmed(live, projectId)), projectId),
    );
    if (live !== undefined) {
      const known = new Set(live.map((agent) => agent.id));
      this.unconfirmedAgents.set(
        projectId,
        merged.flatMap((workspace) =>
          workspaceAgentIds(workspace)
            .filter((id) => !known.has(id))
            .map((id) => liveAgent(id, workspace.container)),
        ),
      );
    }
    this.layoutBases.set(projectId, workspaces);
    this.setWorkspaces(projectId, merged);
    this.pruneActiveSelection();
    if (live !== undefined) this.publish(projectId, merged, live);
    if (!structurallyEqual(this.decoded(this.workspaces(projectId), projectId), workspaces)) {
      this.scheduleSave(projectId);
    }
  }

  /** A save of `workspaces` through the project's machine succeeded: it is now the base. */
  layoutWritten(projectId: string, workspaces: Workspace[]): void {
    if (!this.isLoaded(projectId)) return;
    this.layoutBases.set(projectId, this.decoded(workspaces, projectId));
  }

  /** `workspaces` as a document read back would give them, so they compare with one. */
  private decoded(workspaces: readonly Workspace[], projectId: string): Workspace[] {
    const data = this.persistence.documentData(workspaces, null);
    if (data === null) return [...workspaces];
    return this.persistence.workspaces(data, projectId);
  }

  /**
   * A remote layout read failed (anything but a missing file). Nothing is saved for the project
   * until a retry succeeds and `registerRemoteProject` runs.
   */
  setRemoteLayoutError(projectId: string, message: string | null): void {
    if (this.isLoaded(projectId)) return;
    this.setLayoutError(
      projectId,
      message === null ? null : "Could not restore this project's layout: " + message,
    );
  }

  /**
   * Adopt a local project's layout document if something other than this app wrote it since this
   * app last read or wrote it: a machine that opened the project over SSH. Cheap when nothing
   * changed: one stat.
   */
  adoptLocalLayoutIfChanged(projectRoot: string): void {
    if (isRemoteProjectKey(projectRoot) || !this.isLoaded(projectRoot)) return;
    if (!this.localLayoutStamps.has(projectRoot)) return;
    const known = this.localLayoutStamps.get(projectRoot) ?? null;
    if (this.persistence.localModifiedMs(projectRoot) === known) return;
    let read;
    try {
      read = this.persistence.readLocal(
        this.persistence.filePath(projectRoot),
        this.persistence.maxRemoteLayoutBytes,
      );
    } catch {
      return;
    }
    if (read.truncated || this.persistence.isNewerThanSupported(read.data)) return;
    this.localLayoutStamps.set(projectRoot, read.modifiedMs);
    this.adoptExternalLayout(projectRoot, this.persistence.workspaces(read.data, projectRoot));
  }

  /** Names of file tabs in this project's workspaces holding unsaved edits. */
  unsavedFileNames(projectRoot: string): string[] {
    return this.workspaces(projectRoot).flatMap((workspace) =>
      workspaceSurfaces(workspace).flatMap(({ surface }) => {
        if (surface.content.kind !== 'file') return [];
        const session = this.fileTabs.existingSession(workspace.id, surface.id);
        if (session === null || !session.hasUnsavedChanges) return [];
        return [session.editor.currentFile?.name ?? 'a file'];
      }),
    );
  }

  /** Load a local project's layout on first sight. `true` when it loaded now. */
  private loadProjectIfNeeded(projectRoot: string): boolean {
    if (this.isLoaded(projectRoot)) return false;
    if (isRemoteProjectKey(projectRoot)) throw new RemoteLayoutPending();
    try {
      // Stamped before the read: a write landing in between then shows as a change.
      const stamp = this.persistence.localModifiedMs(projectRoot);
      const restored = this.loadWorkspaces(projectRoot);
      this.setWorkspaces(projectRoot, restored);
      this.loadedProjects.add(projectRoot);
      this.localLayoutStamps.set(projectRoot, stamp);
      this.layoutBases.set(projectRoot, restored);
      this.setLayoutError(projectRoot, null);
      return true;
    } catch (error) {
      this.setLayoutError(
        projectRoot,
        "Could not restore this project's layout: " + errorMessage(error),
      );
      throw error;
    }
  }

  // MARK: Queries

  workspaces(projectRoot: string): readonly Workspace[] {
    return this.workspacesByProject[projectRoot] ?? [];
  }

  workspace(id: string): Workspace | null {
    for (const list of Object.values(this.workspacesByProject)) {
      const match = list.find((workspace) => workspace.id === id);
      if (match !== undefined) return match;
    }
    return null;
  }

  /** The project holding a workspace. */
  projectRoot(workspaceId: string): string | null {
    for (const [root, list] of Object.entries(this.workspacesByProject)) {
      if (list.some((workspace) => workspace.id === workspaceId)) return root;
    }
    return null;
  }

  /** The workspace holding an agent. Exactly one does, by construction. */
  workspaceIdForAgent(agentId: string): string | null {
    for (const list of Object.values(this.workspacesByProject)) {
      const match = list.find((workspace) => containsAgent(workspace, agentId));
      if (match !== undefined) return match.id;
    }
    return null;
  }

  get activeWorkspace(): Workspace | null {
    const id = this.activeWorkspaceId;
    return id === null ? null : this.workspace(id);
  }

  /**
   * The project's Point Guard workspace, created when missing. Uses the same registry and
   * persistence as agent workspaces; the navigation item is this workspace's sidebar entry, so it
   * does not also appear under the project. Throws when the layout cannot be restored.
   */
  ensurePointGuardWorkspace(projectRoot: string): string {
    const didLoad = this.loadProjectIfNeeded(projectRoot);
    // Restore/retry must apply the retained manifest. Ordinary visits must not replay a snapshot
    // that can predate a spawn response already bound to one of these tabs.
    const live = this.liveByProject.get(projectRoot);
    if (didLoad && live !== undefined) {
      this.publish(projectRoot, this.workspaces(projectRoot), live);
    }
    const id = pointGuardId(projectRoot);
    const existing = this.workspaces(projectRoot).find((workspace) => workspace.id === id);
    if (existing !== undefined) {
      if (
        !workspaceSurfaces(existing).some(({ surface }) => surface.content.kind === 'pointGuard')
      ) {
        this.mutate(existing.id, (w) => newTab(w, w.focusedLeafId, POINT_GUARD_CONTENT).workspace);
      }
      return existing.id;
    }
    const workspace = pointGuardWorkspace(projectRoot);
    this.setWorkspaces(projectRoot, [...this.workspaces(projectRoot), workspace]);
    this.scheduleSave(projectRoot);
    return workspace.id;
  }

  // MARK: Reconcile

  /**
   * Fold a project's manifest into the stored layout and publish the canonical result. Called on
   * every manifest change and status push. The layout is loaded on first sight of a project.
   */
  reconcile(
    projectRoot: string,
    rootAgents: readonly AgentModel[],
    worktrees: readonly WorktreeModel[],
  ): void {
    const live = liveAgents(rootAgents, worktrees).map((agent) =>
      placementRekeyed(agent, projectRoot),
    );
    // A failed restore must still retain the current manifest for a later Retry.
    this.liveByProject.set(projectRoot, live);
    if (this.isAwaitingRemoteLayout(projectRoot)) {
      // Show the agents now, in a default layout. Nothing is saved until the machine's layout has
      // been read (no saver is registered), and that layout then replaces it.
      this.publish(projectRoot, this.workspaces(projectRoot), live);
      return;
    }
    try {
      this.loadProjectIfNeeded(projectRoot);
    } catch {
      return;
    }
    // A machine working on this project over SSH may have just written the tab this new agent was
    // spawned for; take its layout first so the agent lands there.
    this.adoptLocalLayoutIfChanged(projectRoot);
    // This manifest is current: an agent a document names that it lacks has exited.
    this.unconfirmedAgents.delete(projectRoot);
    this.publish(projectRoot, this.workspaces(projectRoot), live);
  }

  private withUnconfirmed(live: readonly LiveAgent[], projectRoot: string): LiveAgent[] {
    return withUnconfirmed(live, this.unconfirmedAgents.get(projectRoot) ?? []);
  }

  /** Reconcile `stored` against `live` and publish the result if it changed. */
  private publish(
    projectRoot: string,
    stored: readonly Workspace[],
    live: readonly LiveAgent[],
  ): void {
    const withHeld = this.withUnconfirmed(live, projectRoot);
    const reconciled = reconcileWorkspaces(
      stored,
      deferringReservedAgents(this.reservationsByProject.get(projectRoot) ?? [], stored, withHeld),
      this.reservedWorkspaceIds(projectRoot),
    );
    const current = this.workspacesByProject[projectRoot];
    if (current !== undefined && structurallyEqual(reconciled, current)) return;
    this.setWorkspaces(projectRoot, reconciled);
    this.pruneActiveSelection();
    this.scheduleSave(projectRoot);
  }

  private reservedWorkspaceIds(projectRoot: string): Set<string> {
    return new Set((this.reservationsByProject.get(projectRoot) ?? []).map((r) => r.workspaceId));
  }

  /** Drop the active selection if its workspace no longer exists. */
  private pruneActiveSelection(): void {
    const id = this.activeWorkspaceId;
    if (id !== null && this.workspace(id) === null) this.activeWorkspaceId = null;
  }

  // MARK: Reservations

  /** Claim a tab for the agent a spawn request is about to create. */
  reserveSurface(projectRoot: string, workspaceId: string, surfaceId: number): void {
    const target = this.workspace(workspaceId);
    if (target === null) return;
    this.reservationsByProject.set(projectRoot, [
      ...(this.reservationsByProject.get(projectRoot) ?? []),
      { workspaceId, surfaceId, container: target.container },
    ]);
  }

  /** The spawn response arrived first: bind the agent to its tab immediately. */
  fulfillReservation(
    projectRoot: string,
    workspaceId: string,
    surfaceId: number,
    agentId: string,
  ): void {
    this.dropReservation(projectRoot, workspaceId, surfaceId);
    this.setAgent(agentId, workspaceId, surfaceId);
  }

  /**
   * The spawn failed or completed: stop holding the tab, and let any agents that were held back
   * behind the reservation surface.
   */
  releaseReservation(projectRoot: string, workspaceId: string, surfaceId: number): void {
    this.dropReservation(projectRoot, workspaceId, surfaceId);
    const live = this.liveByProject.get(projectRoot);
    if (live !== undefined) this.publish(projectRoot, this.workspaces(projectRoot), live);
  }

  private dropReservation(projectRoot: string, workspaceId: string, surfaceId: number): void {
    const reservations = this.reservationsByProject.get(projectRoot);
    if (reservations === undefined) return;
    const index = reservations.findIndex(
      (r) => r.workspaceId === workspaceId && r.surfaceId === surfaceId,
    );
    const next = index === -1 ? reservations : reservations.filter((_, i) => i !== index);
    if (next.length === 0) this.reservationsByProject.delete(projectRoot);
    else this.reservationsByProject.set(projectRoot, next);
  }

  // MARK: Pane mutations

  split(workspaceId: string, leafId: number, axis: PaneAxis): void {
    this.mutate(workspaceId, (w) => splitPane(w, leafId, axis).workspace);
  }

  /** Close a pane and every tab in it, killing the agents they held. */
  closePane(workspaceId: string, leafId: number): void {
    const pane = this.workspace(workspaceId)?.panes[leafId];
    if (pane === undefined) return;
    this.removeTabs(
      workspaceId,
      pane.tabs.map((tab) => tab.id),
      (w) => {
        const result = closePane(w, leafId);
        return { workspace: result.workspace, killed: result.agentIds };
      },
    );
  }

  /** Close a whole workspace (its sidebar row), killing every agent in it. */
  closeWorkspace(workspaceId: string): void {
    const target = this.workspace(workspaceId);
    if (target === null) return;
    this.removeTabs(
      workspaceId,
      workspaceSurfaces(target).map(({ surface }) => surface.id),
      (w) => ({
        workspace: removeSurfaces(w, () => true).workspace,
        killed: workspaceAgentIds(w),
      }),
    );
  }

  setRatio(ratio: number, workspaceId: string, splitFirstLeafId: number): void {
    this.mutate(workspaceId, (w) => setSplitRatio(w, ratio, splitFirstLeafId));
  }

  setFocus(workspaceId: string, leafId: number): void {
    this.mutate(workspaceId, (w) =>
      allLeafIds(w.root).includes(leafId) ? { ...w, focusedLeafId: leafId } : w,
    );
  }

  moveFocus(workspaceId: string, direction: FocusDirection): void {
    this.mutate(workspaceId, (w) => moveFocus(w, direction));
  }

  /**
   * Show a workspace. When its focused pane is not showing an agent, focus the first pane that is:
   * clicking a sidebar row opens a single-pane workspace straight into its terminal. Which tab
   * each pane shows is left alone.
   */
  activate(workspaceId: string): void {
    this.activeWorkspaceId = workspaceId;
    const target = this.workspace(workspaceId);
    if (target === null || isPointGuard(target) || focusedAgentId(target) !== null) return;
    const leafId = allLeafIds(target.root).find((id) => {
      const pane = target.panes[id];
      const tab = pane === undefined ? null : paneActiveTab(pane);
      return tab !== null && contentAgentId(tab.content) !== null;
    });
    if (leafId !== undefined) this.setFocus(workspaceId, leafId);
  }

  // MARK: Tab mutations

  /** Open a tab after the pane's active one. Returns the new tab's ID. */
  newTab(
    workspaceId: string,
    leafId: number,
    content: SurfaceContent = EMPTY_CONTENT,
  ): number | null {
    let surfaceId: number | null = null;
    this.mutate(workspaceId, (w) => {
      const result = newTab(w, leafId, content);
      surfaceId = result.surfaceId;
      return result.workspace;
    });
    return surfaceId;
  }

  selectTab(workspaceId: string, surfaceId: number): void {
    this.mutate(workspaceId, (w) => selectTab(w, surfaceId));
  }

  /** Select by 0-based position in a pane. */
  selectTabAt(workspaceId: string, leafId: number, index: number): void {
    this.mutate(workspaceId, (w) => selectTabAt(w, leafId, index));
  }

  selectLastTab(workspaceId: string, leafId: number): void {
    this.mutate(workspaceId, (w) => selectLastTab(w, leafId));
  }

  cycleTab(workspaceId: string, leafId: number, offset: number): void {
    this.mutate(workspaceId, (w) => cycleTab(w, leafId, offset));
  }

  /**
   * Close a tab, killing the agent it held. Its pane goes with its last tab, and the workspace
   * goes with its last pane, so neither lingers as an empty shell.
   */
  closeTab(workspaceId: string, surfaceId: number): void {
    const target = this.workspace(workspaceId);
    if (target === null || leafIdOfSurface(target, surfaceId) === null) return;
    this.removeTabs(workspaceId, [surfaceId], (w) => {
      const result = closeTab(w, surfaceId);
      return { workspace: result.workspace, killed: result.agentIds };
    });
  }

  /** Move a tab to a 0-based position in a pane (`null` appends). The agent keeps running. */
  moveTab(
    workspaceId: string,
    surfaceId: number,
    toLeafId: number,
    index: number | null = null,
  ): void {
    this.mutate(workspaceId, (w) => moveTab(w, surfaceId, toLeafId, index));
  }

  /** Move a tab to just before another tab: a tab dropped onto a tab. */
  moveTabBefore(workspaceId: string, surfaceId: number, targetSurfaceId: number): void {
    this.mutate(workspaceId, (w) => moveTabBefore(w, surfaceId, targetSurfaceId));
  }

  breakTab(workspaceId: string, surfaceId: number, axis: PaneAxis): void {
    this.mutate(workspaceId, (w) => breakTab(w, surfaceId, axis).workspace);
  }

  /** Move a tab into a new pane split off a pane's edge: a tab dropped on that edge. */
  dropTabOnEdge(
    workspaceId: string,
    surfaceId: number,
    targetLeafId: number,
    edge: PaneEdge,
  ): void {
    this.mutate(workspaceId, (w) => dropTabOnEdge(w, surfaceId, targetLeafId, edge).workspace);
  }

  /** Show the file navigator in a tab (optionally opened on a file). No daemon involved. */
  openFile(workspaceId: string, surfaceId: number, path: string | null): void {
    this.mutate(workspaceId, (w) => setSurfaceContent(w, fileContent(path), surfaceId));
  }

  /**
   * Bind an agent to a tab (`null` empties it). The binding wins over any other tab still claiming
   * the same agent, and the project is reconciled straight away so selection and persistence see
   * one consistent result.
   */
  setAgent(agentId: string | null, workspaceId: string, surfaceId: number): void {
    if (agentId === null) {
      this.mutate(workspaceId, (w) => setSurfaceContent(w, EMPTY_CONTENT, surfaceId));
      return;
    }
    const projectRoot = this.projectRoot(workspaceId);
    if (projectRoot === null) return;
    const list = this.workspaces(projectRoot);
    const target = list.find((workspace) => workspace.id === workspaceId);
    if (target === undefined || leafIdOfSurface(target, surfaceId) === null) return;

    const known = this.liveByProject.get(projectRoot)?.find((agent) => agent.id === agentId);
    if (isPointGuard(target) && known !== undefined && known.container.kind !== 'projectRoot') {
      return;
    }

    const stored = bindingAgent(list, agentId, workspaceId, surfaceId);
    // The spawn response can beat the manifest; the agent is real even if the retained list has
    // not seen it yet, so it must not be pruned as dead.
    const live = [...(this.liveByProject.get(projectRoot) ?? [])];
    if (!live.some((agent) => agent.id === agentId))
      live.push(liveAgent(agentId, target.container));
    this.publish(projectRoot, stored, live);
  }

  /**
   * Shared path for closing tabs, panes and workspaces: apply `body`, release reservations on the
   * removed tabs, drop the workspace when nothing the user opened is left (the reconciler's rule,
   * applied now rather than at the next manifest change), and kill the removed agents.
   */
  private removeTabs(
    workspaceId: string,
    surfaceIds: readonly number[],
    body: (workspace: Workspace) => { workspace: Workspace; killed: string[] },
  ): void {
    const projectRoot = this.projectRoot(workspaceId);
    if (projectRoot === null) return;
    const list = [...this.workspaces(projectRoot)];
    const index = list.findIndex((workspace) => workspace.id === workspaceId);
    const original = list[index];
    if (original === undefined) return;

    const { workspace: changed, killed } = body(original);
    let workspace = changed;
    for (const surfaceId of surfaceIds) this.dropReservation(projectRoot, workspaceId, surfaceId);
    if (isPointGuard(workspace) && Object.keys(workspace.panes).length === 0) {
      workspace = resetToPointGuard(workspace);
    }
    if (isGhost(workspace) && !this.reservedWorkspaceIds(projectRoot).has(workspaceId)) {
      list.splice(index, 1);
      if (this.activeWorkspaceId === workspaceId) this.activeWorkspaceId = null;
    } else {
      list[index] = normalizeWorkspace(workspace);
    }

    this.setWorkspaces(projectRoot, list);
    this.scheduleSave(projectRoot);

    for (const agentId of killed) this.onCloseAgent?.(projectRoot, agentId);
  }

  private mutate(workspaceId: string, body: (workspace: Workspace) => Workspace): void {
    const projectRoot = this.projectRoot(workspaceId);
    if (projectRoot === null) return;
    const list = [...this.workspaces(projectRoot)];
    const index = list.findIndex((workspace) => workspace.id === workspaceId);
    const before = list[index];
    if (before === undefined) return;

    const after = normalizeWorkspace(body(before));
    // Writing observed state re-renders every view that reads it, so skip no-ops (e.g. focusing
    // the pane that is already focused).
    if (workspacesEqual(after, before)) return;
    list[index] = after;
    this.setWorkspaces(projectRoot, list);
    this.scheduleSave(projectRoot);
  }

  // MARK: Remote commands

  /**
   * Grid commands from `pu grid ...` address panes and tabs. They apply to the workspace they
   * name, else to whichever workspace is on screen for that project. Tab positions arrive 1-based
   * (as `pu grid show` prints them); tab IDs are surface IDs.
   */
  handleRemoteCommand(command: GridCommand, sourceProjectRoot: string): void {
    if (command.action === 'get_layout') return; // the daemon answers this from the file directly
    const workspaceId = command.workspace_id ?? this.activeWorkspaceId;
    if (workspaceId === null || this.projectRoot(workspaceId) !== sourceProjectRoot) return;
    const current = this.workspace(workspaceId);
    if (current === null) return;

    const pane = (leafId: number | null | undefined): number | null => {
      const target = leafId ?? current.focusedLeafId;
      return current.panes[target] === undefined ? null : target;
    };
    const activeTabId = (leafId: number): number | null => {
      const found = current.panes[leafId];
      return found === undefined ? null : (paneActiveTab(found)?.id ?? null);
    };

    switch (command.action) {
      case 'split': {
        const target = pane(command.leaf_id);
        if (target !== null) this.split(workspaceId, target, gridAxis(command.axis));
        return;
      }
      case 'close': {
        const target = pane(command.leaf_id);
        if (target !== null) this.closePane(workspaceId, target);
        return;
      }
      case 'focus':
        if (command.leaf_id !== undefined && command.leaf_id !== null) {
          this.setFocus(workspaceId, command.leaf_id);
        } else if (command.direction !== undefined && command.direction !== null) {
          this.moveFocus(workspaceId, gridDirection(command.direction));
        }
        return;
      case 'set_agent': {
        const target = pane(command.leaf_id);
        const surfaceId = target === null ? null : activeTabId(target);
        if (surfaceId !== null) this.setAgent(command.agent_id, workspaceId, surfaceId);
        return;
      }
      case 'new_tab': {
        const target = pane(command.leaf_id);
        const surfaceId = target === null ? null : this.newTab(workspaceId, target);
        if (surfaceId === null) return;
        if (command.agent_id !== undefined && command.agent_id !== null) {
          this.setAgent(command.agent_id, workspaceId, surfaceId);
        }
        return;
      }
      case 'select_tab': {
        const target = pane(command.leaf_id);
        if (target === null) return;
        if (command.index !== undefined && command.index !== null) {
          this.selectTabAt(workspaceId, target, command.index - 1);
        } else if (command.direction !== undefined && command.direction !== null) {
          this.cycleTab(workspaceId, target, command.direction === 'prev' ? -1 : 1);
        }
        return;
      }
      case 'close_tab':
        if (command.tab_id !== undefined && command.tab_id !== null) {
          this.closeTab(workspaceId, command.tab_id);
        } else {
          const target = pane(command.leaf_id);
          const surfaceId = target === null ? null : activeTabId(target);
          if (surfaceId !== null) this.closeTab(workspaceId, surfaceId);
        }
        return;
      case 'move_tab':
        this.moveTab(
          workspaceId,
          command.tab_id,
          command.to_leaf,
          command.index === undefined || command.index === null ? null : command.index - 1,
        );
        return;
      case 'break_tab': {
        const surfaceId = command.tab_id ?? focusedSurface(current)?.id ?? null;
        if (surfaceId !== null) this.breakTab(workspaceId, surfaceId, gridAxis(command.axis));
        return;
      }
    }
  }

  // MARK: Persistence

  /** Debounced write (one second coalesce). */
  private scheduleSave(projectRoot: string): void {
    this.saveWorkItems.get(projectRoot)?.cancel();
    const snapshot = this.workspaces(projectRoot);
    const active = this.activeWorkspaceIdIn(projectRoot);
    const remoteSave = this.remoteSavers.get(projectRoot);
    if (remoteSave === undefined && isRemoteProjectKey(projectRoot)) return;
    this.pendingSaves.add(projectRoot);
    const perform = (): void => {
      this.pendingSaves.delete(projectRoot);
      if (remoteSave !== undefined) remoteSave(snapshot, active);
      else this.saveLocal(snapshot, projectRoot, active);
    };
    const handle = this.scheduler.schedule(perform, SAVE_DEBOUNCE_MS);
    this.saveWorkItems.set(projectRoot, {
      perform,
      cancel: () => {
        this.scheduler.cancel(handle);
      },
    });
  }

  /** Whether a layout change of this project is waiting out the save debounce. */
  hasPendingSave(projectRoot: string): boolean {
    return this.pendingSaves.has(projectRoot);
  }

  /** Write a project's pending layout change now rather than after the debounce. */
  saveNow(projectRoot: string): void {
    const item = this.saveWorkItems.get(projectRoot);
    if (!this.pendingSaves.has(projectRoot) || item === undefined) return;
    this.saveWorkItems.delete(projectRoot);
    item.perform();
    item.cancel();
  }

  /** Flush pending writes: called on quit. */
  saveAll(): void {
    for (const [projectRoot, workspaces] of Object.entries(this.workspacesByProject)) {
      this.saveWorkItems.get(projectRoot)?.cancel();
      const active = this.activeWorkspaceIdIn(projectRoot);
      const pending = this.pendingSaves.delete(projectRoot);
      const remoteSave = this.remoteSavers.get(projectRoot);
      if (remoteSave !== undefined) {
        // Unchanged since the last write: rewriting could revert another machine's edit.
        if (pending) remoteSave(workspaces, active);
      } else if (!isRemoteProjectKey(projectRoot)) {
        this.saveLocal(workspaces, projectRoot, active);
      }
    }
  }

  /**
   * Write a local project's layout, unless another machine changed the file since this app last
   * read or wrote it: then adopt that layout instead of overwriting it.
   */
  private saveLocal(
    workspaces: readonly Workspace[],
    projectRoot: string,
    activeWorkspaceId: string | null,
    retryAfterConflict = true,
  ): void {
    const data = this.persistence.documentData(workspaces, activeWorkspaceId);
    if (data === null) return;
    const expected = this.localLayoutStamps.has(projectRoot)
      ? (this.localLayoutStamps.get(projectRoot) ?? null)
      : this.persistence.localModifiedMs(projectRoot);
    const outcome = this.persistence.saveLocal(data, projectRoot, expected);
    switch (outcome.kind) {
      case 'written':
        this.localLayoutStamps.set(projectRoot, outcome.modifiedMs);
        this.layoutBases.set(projectRoot, this.decoded(workspaces, projectRoot));
        return;
      case 'conflict': {
        this.localLayoutStamps.set(projectRoot, outcome.modifiedMs);
        this.adoptExternalLayout(
          projectRoot,
          this.persistence.workspaces(outcome.document, projectRoot),
        );
        // Write the merge now rather than after the debounce: the caller may be a spawn that
        // needs its tab on disk, or quit.
        if (!retryAfterConflict || !this.pendingSaves.delete(projectRoot)) return;
        this.saveWorkItems.get(projectRoot)?.cancel();
        this.saveWorkItems.delete(projectRoot);
        this.saveLocal(
          this.workspaces(projectRoot),
          projectRoot,
          this.activeWorkspaceIdIn(projectRoot),
          false,
        );
        return;
      }
      case 'skipped':
        return;
    }
  }

  private activeWorkspaceIdIn(projectRoot: string): string | null {
    const id = this.activeWorkspaceId;
    if (id === null || !this.workspaces(projectRoot).some((workspace) => workspace.id === id)) {
      return null;
    }
    return id;
  }

  forgetProject(projectRoot: string): void {
    const pending = this.saveWorkItems.get(projectRoot);
    if (
      this.pendingSaves.has(projectRoot) &&
      pending !== undefined &&
      this.remoteSavers.has(projectRoot)
    ) {
      // A remote layout change still waiting out the debounce is written now rather than
      // dropped; the caller flushes the saver before closing the forward.
      pending.perform();
    }
    this.saveWorkItems.get(projectRoot)?.cancel();
    this.saveWorkItems.delete(projectRoot);
    this.pendingSaves.delete(projectRoot);
    this.setWorkspaces(projectRoot, null);
    this.reservationsByProject.delete(projectRoot);
    this.liveByProject.delete(projectRoot);
    this.setLayoutError(projectRoot, null);
    this.loadedProjects.delete(projectRoot);
    this.remoteSavers.delete(projectRoot);
    this.localLayoutStamps.delete(projectRoot);
    this.layoutBases.delete(projectRoot);
    this.unconfirmedAgents.delete(projectRoot);
    this.pruneActiveSelection();
  }
}

// MARK: - Pending workspace selection (AppState)

export interface PendingWorkspaceSelection {
  /** The workspace to select once the registry has it. */
  readonly pendingId: string | null;
  /** Local projects are still being opened, so a missing workspace may yet appear. */
  readonly awaitingLocalProjects: boolean;
  /** Keys of the open local projects whose layouts the selection waits for. */
  readonly localProjectIds: readonly string[];
}

export interface PendingWorkspaceResolution {
  /** The workspace to select now, or `null`. */
  readonly selectedId: string | null;
  /** The pending ID after this call: cleared once it resolves or can no longer resolve. */
  readonly pendingId: string | null;
  /** The project to make active, when the resolved workspace is not Point Guard. */
  readonly activeProjectRoot: string | null;
}

/**
 * Swift `AppState.resolvePendingWorkspaceSelection` over the registry (WIN-6.0's AppState calls
 * this). The pending ID is cleared when it resolves, or when every open project has reconciled and
 * it still names nothing: an unreachable machine may never load its layouts, so remote projects
 * must not hold the selection.
 */
export function resolvePendingWorkspaceSelection(
  registry: WorkspaceRegistry,
  pending: PendingWorkspaceSelection,
): PendingWorkspaceResolution {
  const { pendingId } = pending;
  if (pendingId === null) return { selectedId: null, pendingId: null, activeProjectRoot: null };
  const root = registry.projectRoot(pendingId);
  if (root !== null) {
    const workspace = registry.workspace(pendingId);
    const pointGuard = workspace !== null && isPointGuard(workspace);
    return { selectedId: pendingId, pendingId: null, activeProjectRoot: pointGuard ? null : root };
  }
  const allReconciled =
    !pending.awaitingLocalProjects &&
    pending.localProjectIds.every((id) => Object.hasOwn(registry.workspacesByProject, id));
  return { selectedId: null, pendingId: allReconciled ? null : pendingId, activeProjectRoot: null };
}
