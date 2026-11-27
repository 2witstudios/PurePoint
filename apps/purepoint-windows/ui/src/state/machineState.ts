// Port of apps/purepoint-macos/purepoint-macos/State/MachineState.swift (WIN-8.14).
//
// One machine in the sidebar: its connection state and the projects its daemon has registered,
// each a full `ProjectState` whose daemon, files and git are on that machine (reached through the
// backend's ssh relay). `TauriMachineDirectory` is AppState's `MachineDirectory`: the registry in
// `%USERPROFILE%\.pu\machines.json` and how a machine's state is made.
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in `machine.store`; every change also
// tells the owner (`machineChanged`), whose `projectRevision` re-renders the sidebar. Remote
// projects are followed by the owner like local ones (`followProject`).
//
// Differences from Swift:
// - The project list is polled (`inventory`, every 30 s while the daemon runs) and re-read after
//   an open or a removal; the Mac also streams `subscribe_projects` through its forward. A project
//   another device adds appears at the next poll.
// - Disconnect has no forward to close: pending layout saves are flushed (bounded) and probing
//   stops. Quit's synchronous flush is the backend's to do (each remote save is its own relay).
// - The editor windows of worktree and checkout views (`OpenEditors`) are not ported yet, so
//   unsaved file names come from the registry's file tabs only.
// - Preferences (hidden projects, user-disconnected machines) live in the webview's
//   `localStorage` under the Mac's `UserDefaults` keys.

import { createStore, type StoreApi } from 'zustand/vanilla';
import {
  MACHINE_REGISTRY_DISPLAY_PATH,
  type Machine,
  connectionStatesEqual,
  machineStatusText,
} from '../model/machine';
import { ProjectOrderSaver, movingProject, sortedByProjectOrder } from '../model/projectOrder';
import {
  type MachineConnectionState,
  type MachineSnapshot,
  isDaemonRunning,
} from '../model/sidebarItem';
import type { DaemonRequester } from '../services/daemonConnection';
import {
  MachineConnection,
  addMachineToRegistry,
  loadMachineRegistry,
  machineDaemonClient,
  removeMachineFromRegistry,
  watchMachineRegistry,
} from '../services/machines';
import { DaemonWorkspaceService } from '../services/workspaceService';
import type { AppStorage, MachineDirectory, MachineState } from './appState';
import { openMachineProject, removeMachineProject } from './machineStateOpenProject';
import { ProjectState, type ProjectEnvironment, type ProjectStateOwner } from './projectState';
import type { RemoteLayoutSaver } from './remoteLayoutSaver';
import type { WorkspaceRegistry } from './workspaceRegistry';

/** What a machine reaches in AppState (Swift's `weak var appState`). */
export interface MachineStateOwner extends ProjectStateOwner {
  readonly registry: WorkspaceRegistry | null;
  /** The machine's own state changed: the sidebar re-renders. */
  machineChanged(): void;
  /** Re-render when a remote project's own state changes, as for local ones. */
  followProject(project: ProjectState): void;
  unfollowProject(project: ProjectState): void;
  initializeActiveProjectIfNeeded(root: string): void;
}

/** What `MachineState` needs of its connection (`MachineConnection`; tests stand in). */
export interface MachineConnectionLike {
  connect(): void;
  disconnect(): void;
  refreshNow(): void;
  startDaemon(): Promise<string | null>;
}

export interface MachineStateFields {
  readonly machine: Machine;
  readonly state: MachineConnectionState;
  readonly projects: readonly ProjectState[];
  /** The latest failure of an explicit action (start daemon, project discovery). */
  readonly actionError: string | null;
  readonly isStartingDaemon: boolean;
  /** Bumped when the hidden set changes so the sidebar menu re-reads the count. */
  readonly hiddenCountRevision: number;
  /**
   * Whether the machine has reported its project list at least once. `projects` is empty both
   * before the first inventory reply and for a machine with nothing registered.
   */
  readonly hasLoadedProjects: boolean;
}

export interface LiveMachineStateOptions {
  readonly machine: Machine;
  readonly owner: MachineStateOwner;
  readonly makeConnection?: (
    machine: Machine,
    onState: (state: MachineConnectionState) => void,
  ) => MachineConnectionLike;
  readonly makeClient?: (machineId: string) => DaemonRequester;
  readonly storage?: AppStorage | null;
  readonly projectEnvironment?: Partial<ProjectEnvironment>;
  /** How often the project list is re-read while the daemon is up. */
  readonly refreshIntervalMs?: number;
}

/** How often the project list is re-read while the daemon is up (Swift `refreshInterval`). */
export const PROJECT_REFRESH_INTERVAL_MS = 30_000;
/** How long a disconnect waits for pending layout saves. */
const DISCONNECT_FLUSH_MS = 2_000;

const USER_DISCONNECTED_KEY = 'PurePointDisconnectedMachines';

function hiddenKey(id: string): string {
  return 'PurePointHiddenRemoteProjects.' + id.toLowerCase();
}

function defaultStorage(): AppStorage | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

function readList(storage: AppStorage | null, key: string): string[] {
  try {
    const raw = storage?.getItem(key) ?? null;
    const parsed = raw === null ? null : (JSON.parse(raw) as unknown);
    return Array.isArray(parsed) ? parsed.filter((item) => typeof item === 'string') : [];
  } catch {
    return [];
  }
}

function writeList(storage: AppStorage | null, key: string, list: readonly string[]): void {
  try {
    if (list.length === 0) storage?.removeItem(key);
    else storage?.setItem(key, JSON.stringify(list));
  } catch {
    // Not worth failing over.
  }
}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

/** Machines the user disconnected stay disconnected across launches. */
export function isUserDisconnected(id: string, storage: AppStorage | null = defaultStorage()) {
  return readList(storage, USER_DISCONNECTED_KEY).includes(id.toLowerCase());
}

function setUserDisconnected(id: string, value: boolean, storage: AppStorage | null): void {
  const ids = new Set(readList(storage, USER_DISCONNECTED_KEY));
  if (value) ids.add(id.toLowerCase());
  else ids.delete(id.toLowerCase());
  writeList(storage, USER_DISCONNECTED_KEY, [...ids]);
}

export class LiveMachineState implements MachineState {
  readonly store: StoreApi<MachineStateFields>;
  readonly client: DaemonRequester;

  private readonly owner: MachineStateOwner;
  private readonly makeConnection: NonNullable<LiveMachineStateOptions['makeConnection']>;
  private readonly makeClient: (machineId: string) => DaemonRequester;
  private readonly storage: AppStorage | null;
  private readonly projectEnvironment: Partial<ProjectEnvironment> | undefined;
  private readonly refreshIntervalMs: number;
  private connection: MachineConnectionLike;
  private connectionGeneration = 0;
  private hiddenRoots: Set<string>;
  private refreshTimer: ReturnType<typeof setInterval> | null = null;
  private isUserDisconnecting = false;
  /** Bumped by each local move, so a project list fetched before it can't undo it. */
  private orderRevision = 0;
  private readonly orderSaver: ProjectOrderSaver;
  private pendingDisconnect: Promise<void> = Promise.resolve();

  constructor(options: LiveMachineStateOptions) {
    this.owner = options.owner;
    this.storage = options.storage === undefined ? defaultStorage() : options.storage;
    this.makeClient = options.makeClient ?? machineDaemonClient;
    this.makeConnection =
      options.makeConnection ?? ((machine, onState) => new MachineConnection(machine, onState));
    this.projectEnvironment = options.projectEnvironment;
    this.refreshIntervalMs = options.refreshIntervalMs ?? PROJECT_REFRESH_INTERVAL_MS;
    this.client = this.makeClient(options.machine.id);
    this.hiddenRoots = new Set(readList(this.storage, hiddenKey(options.machine.id)));
    this.store = createStore<MachineStateFields>()(() => ({
      machine: options.machine,
      state: { kind: 'disconnected' },
      projects: [],
      actionError: null,
      isStartingDaemon: false,
      hiddenCountRevision: 0,
      hasLoadedProjects: false,
    }));
    this.connection = this.newConnection();
    this.orderSaver = new ProjectOrderSaver(async (roots) => {
      const response = await this.client.send({
        type: 'reorder_projects',
        project_roots: [...roots],
      });
      return response.type === 'project_order';
    });
  }

  // MARK: Observable accessors

  private get s(): MachineStateFields {
    return this.store.getState();
  }

  private set(fields: Partial<MachineStateFields>): void {
    this.store.setState(fields);
    this.owner.machineChanged();
  }

  get machine(): Machine {
    return this.s.machine;
  }

  get id(): string {
    return this.s.machine.id;
  }

  get state(): MachineConnectionState {
    return this.s.state;
  }

  get projects(): readonly ProjectState[] {
    return this.s.projects;
  }

  get actionError(): string | null {
    return this.s.actionError;
  }

  set actionError(message: string | null) {
    this.set({ actionError: message });
  }

  get isStartingDaemon(): boolean {
    return this.s.isStartingDaemon;
  }

  get hasLoadedProjects(): boolean {
    return this.s.hasLoadedProjects;
  }

  get isConnectedOrConnecting(): boolean {
    return this.state.kind !== 'disconnected';
  }

  /** Whether this machine answers for the rows it shows (Swift `reportsCompleteProjects`). */
  get reportsCompleteProjects(): boolean {
    return isDaemonRunning(this.state) && this.hasLoadedProjects;
  }

  get statusText(): string {
    return machineStatusText(this.state, this.projects.length);
  }

  /** Projects hidden from the sidebar; their daemon still has them registered. */
  get hiddenProjectCount(): number {
    return this.hiddenRoots.size;
  }

  snapshot(): MachineSnapshot {
    const s = this.s;
    return {
      id: s.machine.id,
      name: s.machine.name,
      sshHost: s.machine.sshHost,
      state: s.state,
      statusText: this.statusText,
      actionError: s.actionError,
      isStartingDaemon: s.isStartingDaemon,
      hiddenProjectCount: this.hiddenProjectCount,
      hiddenCountRevision: s.hiddenCountRevision,
      hasLoadedProjects: s.hasLoadedProjects,
      projects: s.projects.map((project) => project.snapshot()),
    };
  }

  private newConnection(): MachineConnectionLike {
    this.connectionGeneration += 1;
    const generation = this.connectionGeneration;
    return this.makeConnection(this.machine, (state) => {
      // Ignore callbacks from a superseded connection.
      if (generation === this.connectionGeneration) this.apply(state);
    });
  }

  /**
   * Update from a fresh registry read: refresh the name, and when the SSH host changed,
   * reconnect on the new host. Called by `AppState.reloadMachines()`.
   */
  update(from: Machine): void {
    if (from.id !== this.machine.id) return;
    const hostChanged = from.sshHost !== this.machine.sshHost;
    this.set({ machine: from });
    if (!hostChanged) return;
    const wasActive = this.state.kind !== 'disconnected';
    // Preserve unsaved projects across the host change — the user didn't initiate this.
    this.disconnect({ keepUnsaved: true });
    this.connection = this.newConnection();
    if (wasActive && !isUserDisconnected(this.id, this.storage)) this.connect();
  }

  // MARK: Connection

  connect(): void {
    setUserDisconnected(this.id, false, this.storage);
    this.isUserDisconnecting = false;
    const connection = this.connection;
    void this.pendingDisconnect.then(() => {
      if (!this.isUserDisconnecting) connection.connect();
    });
  }

  /**
   * Stop following the machine. The remote daemon and its agents keep running (MACH-004).
   * Layout changes still pending are written first (bounded). Callers confirm discarding unsaved
   * file tabs before an explicit disconnect. With `keepUnsaved`, projects with open edits are
   * retained as unavailable instead of dropped; with `remember`, the machine stays disconnected
   * at the next launch.
   */
  disconnect(options: { readonly keepUnsaved?: boolean; readonly remember?: boolean } = {}): void {
    if (options.remember === true) setUserDisconnected(this.id, true, this.storage);
    this.isUserDisconnecting = true;
    this.stopRefreshing();
    const savers = this.removeAllProjects(options.keepUnsaved ?? false);
    this.set({ state: { kind: 'disconnected' } });
    const connection = this.connection;
    connection.disconnect();
    const previous = this.pendingDisconnect;
    this.pendingDisconnect = previous.then(() => LiveMachineState.flush(savers));
  }

  private static async flush(savers: readonly RemoteLayoutSaver[]): Promise<void> {
    await Promise.all(savers.map((saver) => saver.flush(DISCONNECT_FLUSH_MS).catch(() => false)));
  }

  /** Bring back every hidden project (they reappear with the next project refresh). */
  showHiddenProjects(): void {
    this.hiddenRoots.clear();
    writeList(this.storage, hiddenKey(this.id), []);
    this.set({ hiddenCountRevision: this.s.hiddenCountRevision + 1 });
    if (isDaemonRunning(this.state)) void this.refreshProjects();
  }

  /** Explicit user action: start the daemon on the machine over ssh (MACH-003). */
  startDaemon(): void {
    if (this.isStartingDaemon) return;
    this.set({ isStartingDaemon: true, actionError: null });
    void this.connection.startDaemon().then((error) => {
      this.set({
        isStartingDaemon: false,
        ...(error === null
          ? {}
          : { actionError: `Could not start the daemon on ${this.machine.name}: ${error}` }),
      });
    });
  }

  /** Clear the action error (the sidebar's Dismiss Error). */
  dismissError(): void {
    this.actionError = null;
  }

  private apply(next: MachineConnectionState): void {
    // Reports still in flight from a connection the user closed must not reopen it.
    if (this.isUserDisconnecting && next.kind !== 'disconnected') return;
    if (connectionStatesEqual(next, this.state)) return;
    const wasRunning = isDaemonRunning(this.state);
    this.set({ state: next });
    if (isDaemonRunning(next)) {
      if (!wasRunning) this.startRefreshing();
      return;
    }
    this.stopRefreshing();
    // An incompatible daemon gets no further requests; a stopped one has nothing to stream.
    // Projects come back with the daemon. While only the SSH link is down (connecting/offline)
    // they stay, as do projects with unsaved file tabs, which are shown as unavailable instead.
    if (next.kind === 'connected' || next.kind === 'disconnected') this.removeAllProjects(true);
  }

  // MARK: Projects

  private startRefreshing(): void {
    this.stopRefreshing();
    void this.refreshProjects();
    this.refreshTimer = setInterval(() => {
      void this.refreshProjects();
    }, this.refreshIntervalMs);
  }

  private stopRefreshing(): void {
    if (this.refreshTimer !== null) clearInterval(this.refreshTimer);
    this.refreshTimer = null;
  }

  /** Discover the daemon's registered projects (`inventory`, kind projects, all roots). */
  async refreshProjects(): Promise<void> {
    const revision = this.orderRevision;
    let response;
    try {
      response = await this.client.send({ type: 'inventory', kind: 'projects' });
    } catch (error) {
      if (isDaemonRunning(this.state)) {
        this.actionError = `Could not list projects on ${this.machine.name}: ${errorMessage(error)}`;
      }
      return;
    }
    if (!isDaemonRunning(this.state)) return;
    if (response.type === 'inventory') {
      if (this.actionError !== null) this.actionError = null;
      // Listed before a move this app made: the next refresh brings the saved order.
      if (revision !== this.orderRevision) return;
      this.setProjects(response.projects.map((entry) => entry.project_root));
    } else if (response.type === 'error') {
      this.actionError = `Could not list projects on ${this.machine.name}: ${response.message}`;
    }
  }

  private setProjects(roots: readonly string[]): void {
    const current = this.projects;
    let visible = roots.filter((root) => !this.hiddenRoots.has(root));
    if (this.orderSaver.pending > 0) {
      // The list may predate a move still being saved: keep the order on screen.
      const order = current.map((project) => project.projectRoot);
      visible = sortedByProjectOrder(visible, order, (root) => root);
    }
    const wanted = new Set(visible);
    const kept: ProjectState[] = [];
    for (const project of current) {
      if (wanted.has(project.projectRoot)) continue;
      if (this.hasUnsavedTabs(project)) {
        // Gone from the daemon, but its unsaved edits are still open here.
        this.markUnavailable(project);
        kept.push(project);
      } else {
        this.forget(project);
      }
    }
    const next: ProjectState[] = [];
    const seen = new Set<string>();
    // One row per root: a repeated root would otherwise build two sidebar nodes with one identity.
    for (const root of visible) {
      if (seen.has(root)) continue;
      seen.add(root);
      next.push(current.find((project) => project.projectRoot === root) ?? this.makeProject(root));
    }
    next.push(...kept);
    const added = next.filter((candidate) => !current.includes(candidate));
    const restored = next.filter(
      (project) => project.isUnavailable && wanted.has(project.projectRoot),
    );
    const changed =
      next.length !== current.length || next.some((project, index) => project !== current[index]);
    this.set({ ...(changed ? { projects: next } : {}), hasLoadedProjects: true });
    for (const project of added) project.startWatching();
    for (const project of restored) {
      project.isUnavailable = false;
      project.startWatching();
    }
  }

  /**
   * Move a project to `index` among this machine's projects and save the order on the machine,
   * so every device lists its projects the same way.
   */
  moveProject(root: string, index: number): void {
    const next = movingProject(root, index, this.projects, (project) => project.projectRoot);
    if (next === null) return;
    this.set({ projects: next });
    this.orderRevision += 1;
    void this.orderSaver.save(
      next.map((project) => project.projectRoot),
      () => {
        // Show the order the machine kept.
        void this.refreshProjects().then(() => {
          this.actionError = `Could not save the project order on ${this.machine.name}.`;
        });
      },
    );
  }

  private makeProject(root: string): ProjectState {
    const project = new ProjectState({
      projectRoot: root,
      machineId: this.id,
      client: this.client,
      service: new DaemonWorkspaceService(this.client),
      registry: this.owner.registry,
      ...(this.projectEnvironment === undefined ? {} : { environment: this.projectEnvironment }),
    });
    project.owner = this.owner;
    this.owner.followProject(project);
    return project;
  }

  /**
   * Hide a project on this PC only. Nothing on the machine is deleted or stopped, and other
   * devices still show it; `removeProject` removes it from the machine instead.
   */
  hideProject(root: string): void {
    this.hiddenRoots.add(root);
    writeList(this.storage, hiddenKey(this.id), [...this.hiddenRoots]);
    const project = this.projects.find((candidate) => candidate.projectRoot === root);
    if (project !== undefined) this.forget(project);
    this.set({
      hiddenCountRevision: this.s.hiddenCountRevision + 1,
      projects: this.projects.filter((candidate) => candidate !== project),
    });
  }

  /** Show a hidden project again; it reappears with the next project refresh. */
  unhideProject(root: string): void {
    if (!this.hiddenRoots.delete(root)) return;
    writeList(this.storage, hiddenKey(this.id), [...this.hiddenRoots]);
    this.set({ hiddenCountRevision: this.s.hiddenCountRevision + 1 });
  }

  /** Open a folder on this machine as a project (`MachineState+OpenProject`). */
  openProject(root: string): Promise<void> {
    return openMachineProject(this, root, this.client);
  }

  /** Remove a project from the machine for every device (`remove_project`). */
  removeProject(root: string, suspendAgents: boolean): Promise<void> {
    return removeMachineProject(this, root, suspendAgents, this.client);
  }

  /** The owner, for the open-project helpers. */
  get appState(): MachineStateOwner {
    return this.owner;
  }

  /**
   * Drop every project, returning the savers whose queued layout writes should be flushed. With
   * `keepingUnsaved`, a project holding unsaved file tabs stays (unwatched, shown as unavailable).
   */
  removeAllProjects(keepingUnsaved: boolean): RemoteLayoutSaver[] {
    const savers: RemoteLayoutSaver[] = [];
    const kept: ProjectState[] = [];
    for (const project of this.projects) {
      if (keepingUnsaved && this.hasUnsavedTabs(project)) {
        this.markUnavailable(project);
        kept.push(project);
      } else {
        const saver = this.forget(project);
        if (saver !== null) savers.push(saver);
      }
    }
    if (kept.length !== this.projects.length) this.set({ projects: kept });
    return savers;
  }

  /** Names of files with unsaved edits in this machine's projects. */
  get unsavedFileNames(): string[] {
    const names: string[] = [];
    for (const project of this.projects) {
      for (const name of this.unsavedFileNamesIn(project)) {
        if (!names.includes(name)) names.push(name);
      }
    }
    return names;
  }

  unsavedFileNamesIn(project: ProjectState): string[] {
    return this.owner.registry?.unsavedFileNames(project.projectId) ?? [];
  }

  private hasUnsavedTabs(project: ProjectState): boolean {
    return this.unsavedFileNamesIn(project).length > 0;
  }

  private markUnavailable(project: ProjectState): void {
    if (project.isUnavailable) return;
    project.stopWatching();
    project.isUnavailable = true;
  }

  private forget(project: ProjectState): RemoteLayoutSaver | null {
    project.stopWatching();
    this.owner.registry?.forgetProject(project.projectId);
    this.owner.unfollowProject(project);
    return project.remoteLayoutSaver;
  }
}

// MARK: - The machine directory (Swift `MachineRegistryStore` + `MachineState.init`)

export interface TauriMachineDirectoryOptions {
  readonly storage?: AppStorage | null;
  readonly projectEnvironment?: Partial<ProjectEnvironment>;
}

/** AppState's machines: `machines.json` through the backend, and live machine states. */
export class TauriMachineDirectory implements MachineDirectory {
  readonly displayPath = MACHINE_REGISTRY_DISPLAY_PATH;
  private readonly storage: AppStorage | null;
  private readonly projectEnvironment: Partial<ProjectEnvironment> | undefined;

  constructor(options: TauriMachineDirectoryOptions = {}) {
    this.storage = options.storage === undefined ? defaultStorage() : options.storage;
    this.projectEnvironment = options.projectEnvironment;
  }

  load(): Promise<Machine[]> {
    return loadMachineRegistry();
  }

  add(name: string, sshHost: string, id?: string): Promise<Machine> {
    return addMachineToRegistry(name, sshHost, id);
  }

  async remove(id: string): Promise<void> {
    await removeMachineFromRegistry(id);
  }

  isUserDisconnected(id: string): boolean {
    return isUserDisconnected(id, this.storage);
  }

  makeState(machine: Machine, appState: MachineStateOwner): MachineState {
    return new LiveMachineState({
      machine,
      owner: appState,
      storage: this.storage,
      ...(this.projectEnvironment === undefined
        ? {}
        : { projectEnvironment: this.projectEnvironment }),
    });
  }

  watch(onChange: () => void): () => void {
    let stopped = false;
    let stop: (() => void) | null = null;
    void watchMachineRegistry(onChange).then(
      (unwatch) => {
        if (stopped) unwatch();
        else stop = unwatch;
      },
      (error: unknown) => {
        console.warn('PurePoint: could not watch the machine registry', error);
      },
    );
    return () => {
      stopped = true;
      stop?.();
    };
  }
}
