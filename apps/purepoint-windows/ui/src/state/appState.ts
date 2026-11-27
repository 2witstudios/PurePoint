// Port of apps/purepoint-macos/purepoint-macos/State/AppState.swift (WIN-6.0).
//
// The multi-project container: one ProjectState per open project, the machines reachable over
// SSH, Point Guard's folders, the active project and sidebar selection, pending selections and the
// daemon error banner. It follows the local daemon's registered projects, and owns the app-wide
// listeners for the backend's events, forwarding each to the project it names.
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in a Zustand store (`app.store`);
// `projectRevision` advances whenever any project's own store changes, so a view that projects
// `projectSnapshots()` re-renders. The sidebar consumes `projectSnapshots()` and
// `machineSnapshots()` (WIN-6.5's `ProjectSnapshot` / `MachineSnapshot`).
//
// Differences from Swift:
// - Agent signal notifications (WIN-7.7): the window's focus stands in for `NSApp.isActive` and
//   the key window (`setWindowFocused`), and a notification's route is checked against the
//   project's status as last reported, not a fresh `status` request (the status subscription
//   streams every change, so the reported status is current).
// - Machines (WIN-8.14) arrive through the injected `MachineDirectory` (the app's is
//   `TauriMachineDirectory`); the machine list stays empty without one. Remote projects' status,
//   grid and subscription events carry their machine's id and go to that machine's project.
// - The binary watcher that restarts an updated daemon is the backend's (WIN-5.3 lifecycle).
// - Canonical paths: the daemon reports the roots it registered and the Windows app opens them
//   as reported, so no `realpath` step is needed to match them.
// - Library screens' states (agents hub, schedules, triggers) are their own leaves (WIN-8.x).

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { Machine } from '../model/machine';
import { type AgentModel, agentSignalPresentation, isAgentAlive } from '../model/workspaceModel';
import { makeSignalAlertAgent, type SignalAlertAgent } from '../model/agentSignalPolicy';
import {
  type AgentSignalDestination,
  type SignalRoutingTarget,
  destinationMatches,
  isSignalVisible,
  signalRoutingTarget,
} from '../model/agentSignalRouting';
import { opaqueKey, signalQualificationKey } from '../model/agentSignalSnapshot';
import { isPointGuard, workspaceSurfaces } from '../model/workspace';
import type { AgentSignalNotifications } from '../services/agentSignalNotifications';
import { parseProjectKey, projectKey, projectKeyId } from '../model/projectKey';
import { ProjectOrderSaver, movingProject, sortedByProjectOrder } from '../model/projectOrder';
import type { MachineSnapshot, ProjectSnapshot, SidebarSelection } from '../model/sidebarItem';
import { selection as sidebarSelection } from '../model/sidebarItem';
import { updateActiveProject } from '../model/sidebarOutline';
import {
  loadWorkspaceSelection,
  saveWorkspaceSelection,
  type SelectionStorage,
} from '../model/workspaceSelection';
import { daemonBannerMessage, onDaemonState, type DaemonConnectionState } from '../services/daemon';
import { localDaemonClient, type DaemonRequester } from '../services/daemonConnection';
import {
  onGridEvent,
  onProjectsChanged,
  onStatusEvent,
  onSubscriptionState,
  startProjectsSubscription,
  stopProjectsSubscription,
  type GridEvent,
  type ProjectsChanged,
  type StatusEvent,
  type SubscriptionStateEvent,
} from '../services/subscriptions';
import { onManifestChanged, type ManifestChanged } from '../services/watchers';
import { DaemonWorkspaceService, type WorkspaceService } from '../services/workspaceService';
import type { CachedWorkspacePersistence } from './cachedWorkspacePersistence';
import { ProjectState, type ProjectEnvironment, type ProjectStateOwner } from './projectState';
import {
  resolvePendingWorkspaceSelection as resolvePending,
  type WorkspaceRegistry,
} from './workspaceRegistry';

// MARK: - Machines seam (WIN-8.14)

export type { Machine } from '../model/machine';

/**
 * What AppState asks of a machine's live state (Swift `MachineState`; the app's is
 * `LiveMachineState` in `machineState.ts`).
 */
export interface MachineState {
  readonly machine: Machine;
  readonly projects: readonly ProjectState[];
  /** The machine's daemon, through the backend's ssh relay. */
  readonly client: DaemonRequester;
  snapshot(): MachineSnapshot;
  update(from: Machine): void;
  connect(): void;
  /** `remember`: stay disconnected at the next launch (an explicit Disconnect). */
  disconnect(options?: { readonly keepUnsaved?: boolean; readonly remember?: boolean }): void;
  startDaemon(): void;
  showHiddenProjects(): void;
  dismissError(): void;
  /** Register a folder on the machine as a project (`init`); rejects with the daemon's refusal. */
  openProject(root: string): Promise<void>;
  hideProject(root: string): void;
  removeProject(root: string, suspendAgents: boolean): Promise<void>;
  moveProject(root: string, index: number): void;
  /** Swift `unsavedFileNames`: files with unsaved edits across the machine's projects. */
  readonly unsavedFileNames?: readonly string[];
  /** Swift `unsavedFileNames(in:)`. */
  unsavedFileNamesIn?(project: ProjectState): string[];
}

/** The machine registry and how machine states are made (Swift `MachineRegistryStore`). */
export interface MachineDirectory {
  readonly displayPath: string;
  load(): Promise<Machine[]>;
  add(name: string, sshHost: string, id?: string): Promise<Machine>;
  remove(id: string): Promise<void>;
  isUserDisconnected(id: string): boolean;
  makeState(machine: Machine, appState: AppState): MachineState;
  /** Watch the registry file; returns the stop function. */
  watch?(onChange: () => void): () => void;
}

// MARK: - Observable state

export interface AppStateFields {
  /** Local projects, in sidebar order. Remote projects belong to their machine. */
  readonly projects: readonly ProjectState[];
  /** Machines reachable over SSH, each with the projects its daemon has registered. */
  readonly machines: readonly MachineState[];
  /** Unknown until Pi reports its actual configured folder. */
  readonly pointGuardRoot: string | null;
  /** The active project's key (`ProjectKey` id): its root for local projects. */
  readonly activeProjectRoot: string | null;
  /** The shell starts on Dashboard before its first selection-change callback. */
  readonly activeSidebarSelection: SidebarSelection | null;
  readonly daemonError: string | null;
  /** The local daemon connection (the backend's `daemon-state`). */
  readonly daemonConnection: DaemonConnectionState;
  readonly showSettings: boolean;
  /** Agent ID a spawn just returned. Resolved to its workspace once the manifest lands. */
  readonly pendingSelectAgentId: string | null;
  readonly pendingSelectWorktreeId: string | null;
  /** Workspace to select once the sidebar has it: used to restore the last session. */
  readonly pendingSelectWorkspaceId: string | null;
  readonly pendingFocusAgentId: string | null;
  /** A notification's agent to select and focus, consumed by the shell (WIN-7.7). */
  readonly pendingSignalDestination: AgentSignalDestination | null;
  /** Advances when any project's (or machine's) own state changes. */
  readonly projectRevision: number;
}

/** Key/value storage (Swift `UserDefaults.standard`). */
export type AppStorage = SelectionStorage;

export interface AppStateOptions {
  readonly registry?: WorkspaceRegistry | null;
  /** The registry's cached persistence, opened per local project. */
  readonly persistence?: CachedWorkspacePersistence | null;
  readonly service?: WorkspaceService;
  readonly client?: DaemonRequester;
  /** How an opened project starts (tests pass one that does nothing). */
  readonly startProject?: (project: ProjectState) => void;
  /** Whether `restoreProjects` follows the local daemon's project list (off in tests). */
  readonly followsLocalDaemon?: boolean;
  readonly storage?: AppStorage | null;
  /** `%USERPROFILE%`, for `isTooBroadProjectRoot`. */
  readonly homeDirectory?: string | null;
  /** Whether a folder exists (an unmounted drive's project waits for the next report). */
  readonly pathExists?: (path: string) => boolean;
  readonly machines?: MachineDirectory | null;
  readonly projectEnvironment?: Partial<ProjectEnvironment>;
  /** Toasts and the taskbar badge (WIN-7.7); none unless given. */
  readonly signalNotifications?: AgentSignalNotifications | null;
}

const OPEN_PROJECTS_KEY = 'PurePointOpenProjects';
const SHOWN_POINT_GUARD_FOLDERS_KEY = 'PurePointShownPointGuardFolders';
const POINT_GUARD_FOLDERS_KEY = 'PurePointPointGuardFolders';

function defaultStorage(): AppStorage | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

function readList(storage: AppStorage | null, key: string): string[] | null {
  try {
    const raw = storage?.getItem(key) ?? null;
    if (raw === null) return null;
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed) ? parsed.filter((item) => typeof item === 'string') : null;
  } catch {
    return null;
  }
}

function writeList(storage: AppStorage | null, key: string, list: readonly string[] | null): void {
  try {
    if (list === null) storage?.removeItem(key);
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

/** A path with `\` separators, no trailing separator, lowercased (Windows paths ignore case). */
function comparablePath(path: string): string {
  const unified = path.replace(/\//g, '\\');
  const trimmed = /^[A-Za-z]:\\$/.test(unified) ? unified : unified.replace(/\\+$/, '');
  return trimmed.toLowerCase();
}

/**
 * Whether a path is too broad to open as a project: a filesystem or drive root, the home folder,
 * or an ancestor of it. Mirrors the daemon's `is_too_broad_project_root`.
 */
export function isTooBroadProjectRoot(path: string, homeDirectory: string | null): boolean {
  const canonical = comparablePath(path);
  if (canonical === '\\' || canonical === '' || /^[a-z]:\\?$/.test(canonical)) return true;
  if (homeDirectory !== null) {
    const home = comparablePath(homeDirectory);
    if (home === canonical || home.startsWith(canonical + '\\')) return true;
  }
  return false;
}

/** The path normalized the way `URL.standardizedFileURL.path` normalizes it: no `.`/`..`. */
function standardizedPath(path: string): string {
  const separator = path.includes('\\') || /^[A-Za-z]:/.test(path) ? '\\' : '/';
  const parts = path.split(/[\\/]+/);
  const out: string[] = [];
  for (const part of parts) {
    if (part === '.') continue;
    if (part === '..') {
      if (out.length > 1) out.pop();
      continue;
    }
    out.push(part);
  }
  const joined = out.join(separator);
  return joined.length > 1 ? joined.replace(/[\\/]+$/, '') : joined;
}

// MARK: - AppState

export class AppState implements ProjectStateOwner {
  readonly store: StoreApi<AppStateFields>;
  registry: WorkspaceRegistry | null;

  private readonly persistence: CachedWorkspacePersistence | null;
  private readonly service: WorkspaceService;
  private readonly daemon: DaemonRequester;
  private readonly startProjectFn: (project: ProjectState) => void;
  private readonly followsLocalDaemon: boolean;
  private readonly storage: AppStorage | null;
  private homeDirectory: string | null;
  private readonly pathExists: (path: string) => boolean;
  private readonly machineDirectory: MachineDirectory | null;
  private readonly projectEnvironment: Partial<ProjectEnvironment> | undefined;

  /** Folder-backed daemon state for Point Guard, kept out of the project sidebar. */
  private readonly pointGuardProjects = new Map<string, ProjectState>();
  /** Ownership survives project promotion and lazy workspace restoration. */
  private readonly pointGuardFolders = new Set<string>();
  /** Point Guard folders the user also opened as sidebar projects. */
  private readonly shownPointGuardFolders = new Set<string>();
  /** Whether the first local project list reopens last session's sidebar (and resumes it). */
  private resumesOnFirstLocalList = false;
  private followingLocalProjects = false;
  /** The local daemon's projects as last reported; null until the first report. */
  private lastLocalProjects: Set<string> | null = null;
  private localProjectOrder: readonly string[] = [];
  private readonly localOrderSaver: ProjectOrderSaver;
  private readonly projectUnsubscribers = new Map<ProjectState, () => void>();
  private stopMachineWatcher: (() => void) | null = null;
  private readonly signalNotifications: AgentSignalNotifications | null;
  /** Whether the main window has focus; assumed until the window reports otherwise. */
  private windowFocused = true;

  constructor(options: AppStateOptions = {}) {
    this.registry = options.registry ?? null;
    this.persistence = options.persistence ?? null;
    this.daemon = options.client ?? localDaemonClient;
    this.service = options.service ?? new DaemonWorkspaceService(this.daemon);
    this.startProjectFn =
      options.startProject ??
      ((project) => {
        project.startWatching();
      });
    this.followsLocalDaemon = options.followsLocalDaemon ?? true;
    this.storage = options.storage === undefined ? defaultStorage() : options.storage;
    this.homeDirectory = options.homeDirectory ?? null;
    this.pathExists = options.pathExists ?? (() => true);
    this.machineDirectory = options.machines ?? null;
    this.projectEnvironment = options.projectEnvironment;
    this.signalNotifications = options.signalNotifications ?? null;
    this.localOrderSaver = new ProjectOrderSaver(async (roots) => {
      const response = await this.daemon.send({
        type: 'reorder_projects',
        project_roots: [...roots],
      });
      return response.type === 'project_order';
    });
    this.store = createStore<AppStateFields>()(() => ({
      projects: [],
      machines: [],
      pointGuardRoot: null,
      activeProjectRoot: null,
      activeSidebarSelection: sidebarSelection.nav('dashboard'),
      daemonError: null,
      daemonConnection: { state: 'idle' },
      showSettings: false,
      pendingSelectAgentId: null,
      pendingSelectWorktreeId: null,
      pendingSelectWorkspaceId: null,
      pendingFocusAgentId: null,
      pendingSignalDestination: null,
      projectRevision: 0,
    }));
  }

  // MARK: Observable accessors

  private get s(): AppStateFields {
    return this.store.getState();
  }

  get projects(): readonly ProjectState[] {
    return this.s.projects;
  }

  get machines(): readonly MachineState[] {
    return this.s.machines;
  }

  get activeProjectRoot(): string | null {
    return this.s.activeProjectRoot;
  }

  set activeProjectRoot(root: string | null) {
    this.store.setState({ activeProjectRoot: root });
  }

  get activeSidebarSelection(): SidebarSelection | null {
    return this.s.activeSidebarSelection;
  }

  get daemonError(): string | null {
    return this.s.daemonError;
  }

  get pointGuardRoot(): string | null {
    return this.s.pointGuardRoot;
  }

  get pendingSelectWorkspaceId(): string | null {
    return this.s.pendingSelectWorkspaceId;
  }

  set pendingSelectWorkspaceId(id: string | null) {
    this.store.setState({ pendingSelectWorkspaceId: id });
  }

  get isLoaded(): boolean {
    return this.projects.length > 0;
  }

  // ProjectStateOwner

  setDaemonError(message: string | null): void {
    this.store.setState({ daemonError: message });
  }

  setActiveProjectRoot(root: string | null): void {
    this.activeProjectRoot = root;
  }

  setPendingSelectAgentId(agentId: string | null): void {
    this.store.setState({ pendingSelectAgentId: agentId });
  }

  setPendingSelectWorktreeId(worktreeId: string | null): void {
    this.store.setState({ pendingSelectWorktreeId: worktreeId });
  }

  setPendingFocusAgentId(agentId: string | null): void {
    this.store.setState({ pendingFocusAgentId: agentId });
  }

  setShowSettings(show: boolean): void {
    this.store.setState({ showSettings: show });
  }

  setPendingSignalDestination(destination: AgentSignalDestination | null): void {
    this.store.setState({ pendingSignalDestination: destination });
  }

  /** Remote projects, every machine's. */
  get remoteProjects(): ProjectState[] {
    return this.machines.flatMap((machine) => [...machine.projects]);
  }

  private get localManagedProjects(): ProjectState[] {
    const hidden = [...this.pointGuardProjects.values()].filter(
      (project) => !this.projects.some((open) => open.projectRoot === project.projectRoot),
    );
    return [...this.projects, ...hidden];
  }

  /** Every project the app follows: local, Point Guard folders, and each machine's. */
  private get managedProjects(): ProjectState[] {
    return [...this.localManagedProjects, ...this.remoteProjects];
  }

  /** The active project when it is local: library screens talk to the local daemon only. */
  get activeLocalProjectRoot(): string | null {
    const active = this.activeProjectRoot;
    if (active === null || parseProjectKey(active).machineId !== null) {
      return this.projects[0]?.projectRoot ?? null;
    }
    return active;
  }

  /** The project roots the library screens list and create in. */
  get libraryProjectRoots(): string[] {
    return this.projects.map((project) => project.projectRoot);
  }

  /** The agent in the focused pane of the on-screen workspace, or null. */
  get focusedAgentId(): string | null {
    const workspace = this.registry?.activeWorkspace ?? null;
    if (workspace === null) return null;
    const pane = workspace.panes[workspace.focusedLeafId];
    const surface = pane?.tabs.find((tab) => tab.id === pane.activeTabId) ?? null;
    return surface?.content.kind === 'agent' ? surface.content.agentId : null;
  }

  // MARK: Sidebar projections

  /** The local projects as the sidebar reads them. */
  projectSnapshots(): ProjectSnapshot[] {
    return this.projects.map((project) => project.snapshot());
  }

  /** The machines as the sidebar reads them. */
  machineSnapshots(): MachineSnapshot[] {
    return this.machines.map((machine) => machine.snapshot());
  }

  // MARK: Backend events

  /**
   * Listen for the backend's status, grid, subscription, manifest and daemon events and forward
   * each to the project it names. Resolves the function that stops listening.
   */
  async connect(): Promise<() => void> {
    const stops = await Promise.all([
      onStatusEvent((event) => {
        this.handleStatusEvent(event);
      }),
      onGridEvent((event) => {
        this.handleGridEvent(event);
      }),
      onSubscriptionState((event) => {
        this.handleSubscriptionState(event);
      }),
      onManifestChanged((event) => {
        this.handleManifestChanged(event);
      }),
      onProjectsChanged((event) => {
        this.handleProjectsChanged(event);
      }),
      onDaemonState((state) => {
        this.handleDaemonState(state);
      }),
    ]);
    return () => {
      for (const stop of stops) stop();
    };
  }

  /** Local daemons report bare roots; the status stream names the project by root. */
  private localProject(root: string): ProjectState | null {
    return this.localManagedProjects.find((project) => project.projectRoot === root) ?? null;
  }

  /** The project an event names: a machine's when it carries a machine id, else a local one. */
  private eventProject(root: string, machineId: string | null | undefined): ProjectState | null {
    if (machineId === null || machineId === undefined) return this.localProject(root);
    const machine = this.machines.find((candidate) => candidate.machine.id === machineId);
    return machine?.projects.find((project) => project.projectRoot === root) ?? null;
  }

  handleStatusEvent(event: StatusEvent): void {
    this.eventProject(event.projectRoot, event.machineId)?.applyStatusEvent(
      event.worktrees,
      event.agents,
    );
  }

  handleGridEvent(event: GridEvent): void {
    const machineId = event.machineId ?? null;
    this.registry?.handleRemoteCommand(
      event.command,
      projectKeyId(projectKey(event.projectRoot, machineId)),
    );
  }

  handleSubscriptionState(event: SubscriptionStateEvent): void {
    if (event.projectRoot === null) return;
    this.eventProject(event.projectRoot, event.machineId)?.handleSubscriptionState(event);
  }

  handleManifestChanged(event: ManifestChanged): void {
    this.localProject(event.projectRoot)?.handleManifestChanged();
  }

  handleProjectsChanged(event: ProjectsChanged): void {
    this.applyLocalProjects(event.projects);
  }

  handleDaemonState(state: DaemonConnectionState): void {
    this.store.setState({ daemonConnection: state });
    const banner = daemonBannerMessage(state);
    if (banner !== null) {
      this.setDaemonError(banner);
      this.daemonStateBanner = banner;
    } else if (state.state === 'connected') {
      // The daemon is back: drop the banner its earlier failure put up, but keep any
      // unrelated error shown since.
      if (this.daemonStateBanner !== null && this.daemonError === this.daemonStateBanner) {
        this.setDaemonError(null);
      }
      this.daemonStateBanner = null;
    }
  }

  /** The banner the last failed `daemon-state` showed, cleared once the daemon connects. */
  private daemonStateBanner: string | null = null;

  // MARK: Project management

  private makeProject(root: string): ProjectState {
    const project = new ProjectState({
      projectRoot: root,
      service: this.service,
      client: this.daemon,
      registry: this.registry,
      layout: this.persistence,
      ...(this.projectEnvironment === undefined ? {} : { environment: this.projectEnvironment }),
    });
    project.owner = this;
    return project;
  }

  private follow(project: ProjectState): void {
    if (this.projectUnsubscribers.has(project)) return;
    const stopProject = project.store.subscribe(() => {
      this.bumpRevision();
    });
    // The sidebar's channel badge (Swift observes `project.channel.unreadCount`).
    const stopChannel = project.channel.store.subscribe((state, previous) => {
      if (state.unreadCount !== previous.unreadCount) this.bumpRevision();
    });
    this.projectUnsubscribers.set(project, () => {
      stopProject();
      stopChannel();
    });
  }

  private unfollow(project: ProjectState): void {
    this.projectUnsubscribers.get(project)?.();
    this.projectUnsubscribers.delete(project);
  }

  /** A machine's own state changed (connection, actions, its project list). */
  machineChanged(): void {
    this.bumpRevision();
  }

  /** Follow a remote project's state for the sidebar, as local projects are followed. */
  followProject(project: ProjectState): void {
    this.follow(project);
  }

  unfollowProject(project: ProjectState): void {
    this.unfollow(project);
  }

  private bumpRevision(): void {
    this.store.setState({ projectRevision: this.s.projectRevision + 1 });
  }

  private startProject(project: ProjectState): void {
    this.follow(project);
    this.startProjectFn(project);
  }

  /** Open a project on this PC. */
  openProject(root: string, resumingSuspendedAgents = true): void {
    if (this.projects.some((project) => project.projectRoot === root)) return;
    const promoted = this.pointGuardProjects.get(root);
    this.pointGuardProjects.delete(root);
    const project = promoted ?? this.makeProject(root);
    project.owner = this;
    if (!resumingSuspendedAgents) project.keepsSuspendedAgentsOnNextOpen = true;
    this.store.setState({ projects: [...this.projects, project] });
    if (this.pointGuardFolders.has(root)) this.shownPointGuardFolders.add(root);
    this.startProject(project);
    this.initializeActiveProjectIfNeeded(root);
    this.persistOpenProjects();
  }

  /** Returns true when the root is no longer managed and its local files can be discarded. */
  closeProject(root: string): boolean {
    const key = parseProjectKey(root);
    if (key.machineId !== null) {
      // Removing a remote project only hides it here; nothing on the machine changes.
      const machine = this.machines.find((candidate) => candidate.machine.id === key.machineId);
      if (machine === undefined) return false;
      machine.hideProject(key.root);
      if (this.activeProjectRoot === root) {
        this.activeProjectRoot = this.projects[0]?.projectRoot ?? null;
      }
      return false;
    }
    const project = this.projects.find((candidate) => candidate.projectRoot === root);
    if (project === undefined) return false;
    this.store.setState({ projects: this.projects.filter((candidate) => candidate !== project) });
    if (this.isPointGuardFolder(root)) {
      this.pointGuardFolders.add(root);
      this.shownPointGuardFolders.delete(root);
      this.pointGuardProjects.set(root, project);
    } else {
      project.stopWatching();
      this.unfollow(project);
      this.registry?.forgetProject(root);
      void this.persistence?.close(root);
    }
    if (this.activeProjectRoot === root) {
      this.activeProjectRoot = this.projects[0]?.projectRoot ?? null;
    }
    this.persistOpenProjects();
    return this.projectState(root) === null;
  }

  /**
   * Whether Point Guard uses this local folder, so it must stay registered and followed even when
   * it is not in the sidebar.
   */
  isPointGuardFolder(root: string): boolean {
    return (
      root === this.pointGuardRoot ||
      this.pointGuardFolders.has(root) ||
      this.registry?.workspacesByProject[root]?.some(isPointGuard) === true
    );
  }

  /**
   * Remove a project for every device that shows it: its machine's daemon forgets it
   * (`remove_project`), suspending its running agents and keeping its files. A local folder Point
   * Guard still uses only leaves this PC's sidebar.
   */
  async removeProject(project: ProjectState): Promise<void> {
    if (project.isRemote) {
      const machine = this.machine(project);
      if (machine === null) return;
      await machine.removeProject(project.projectRoot, true);
      if (this.activeProjectRoot === project.projectId) {
        this.activeProjectRoot = this.projects[0]?.projectRoot ?? null;
      }
      return;
    }
    const root = project.projectRoot;
    if (!this.isPointGuardFolder(root)) {
      const response = await this.daemon.send({
        type: 'remove_project',
        project_root: root,
        suspend_agents: true,
      });
      if (response.type === 'error') throw new Error(response.message);
    }
    this.closeProject(root);
  }

  /** Whether a path is too broad to open as a project (`/`, a drive root, home or above). */
  isTooBroadProjectRoot(path: string): boolean {
    return isTooBroadProjectRoot(path, this.homeDirectory);
  }

  /** `%USERPROFILE%`, once the app has read it (the window starts before it is known). */
  setHomeDirectory(path: string | null): void {
    this.homeDirectory = path;
  }

  // MARK: Project order

  /** The projects listed beside `project`: the local ones, or its machine's. */
  siblingProjects(project: ProjectState): readonly ProjectState[] {
    if (project.machineId === null) return this.projects;
    return this.machines.find((m) => m.machine.id === project.machineId)?.projects ?? [];
  }

  /** Move a project among its siblings. The order is saved on the machine that owns it. */
  moveProject(project: ProjectState, index: number): void {
    const machine = this.machine(project);
    if (machine !== null) {
      machine.moveProject(project.projectRoot, index);
      return;
    }
    const next = movingProject(project.projectRoot, index, this.projects, (p) => p.projectRoot);
    if (next === null) return;
    this.store.setState({ projects: next });
    // On success the daemon pushes the new order to every device following it.
    void this.localOrderSaver.save(
      next.map((p) => p.projectRoot),
      () => {
        this.setDaemonError('Could not save the project order.');
        this.sortLocalProjects(this.localProjectOrder);
      },
    );
  }

  /**
   * Open the projects this PC shows. With a root (`--project-root`) only that one; otherwise the
   * local daemon's registered projects, followed from then on.
   */
  restoreProjects(projectRoot: string | null = null): void {
    // A list saved before the daemon owned it is opened once, which registers each project with
    // the daemon, and then deleted.
    const paths =
      projectRoot === null ? (readList(this.storage, OPEN_PROJECTS_KEY) ?? []) : [projectRoot];
    // Opening projects persists current state, so snapshot the saved lists first.
    const savedPointGuardFolders = readList(this.storage, POINT_GUARD_FOLDERS_KEY) ?? [];
    for (const root of savedPointGuardFolders) this.pointGuardFolders.add(root);
    for (const root of readList(this.storage, SHOWN_POINT_GUARD_FOLDERS_KEY) ?? []) {
      this.shownPointGuardFolders.add(root);
    }
    const notImported: string[] = [];
    for (const path of paths) {
      if (projectRoot === null && !this.pathExists(path)) {
        // On a drive that is not mounted yet: try again next launch.
        notImported.push(path);
        continue;
      }
      if (this.isTooBroadProjectRoot(path)) continue;
      this.openProject(path);
    }
    // The first list reopens last session's sidebar, unless that came from the legacy list.
    this.resumesOnFirstLocalList = paths.length === notImported.length;
    if (projectRoot === null) {
      writeList(this.storage, OPEN_PROJECTS_KEY, notImported.length === 0 ? null : notImported);
    }
    for (const root of savedPointGuardFolders) {
      if (!this.pathExists(root) || this.isTooBroadProjectRoot(root)) continue;
      this.ensurePointGuardProject(root);
    }
    if (projectRoot === null && this.followsLocalDaemon) this.followLocalProjects();
    // Workspace layouts load lazily, on the first manifest read for each project.
  }

  private followLocalProjects(): void {
    if (this.followingLocalProjects) return;
    this.followingLocalProjects = true;
    // The daemon holds the project list, so it runs even before any project opens.
    void startProjectsSubscription().catch((error: unknown) => {
      this.setDaemonError(errorMessage(error));
    });
  }

  /** Stop following the local daemon's projects. */
  stopFollowingLocalProjects(): void {
    if (!this.followingLocalProjects) return;
    this.followingLocalProjects = false;
    void stopProjectsSubscription().catch(() => false);
  }

  /**
   * Match the sidebar to the local daemon's projects. A project that left the list was removed (on
   * another device, or with `pu projects remove`) and closes here; one that joined opens, unless it
   * is a Point Guard folder the user has not opened.
   */
  applyLocalProjects(roots: readonly string[]): void {
    const registered = new Set(roots);
    for (const root of this.lastLocalProjects ?? []) {
      if (registered.has(root)) continue;
      const owner = this.projectState(root) ?? this.pointGuardProjects.get(root) ?? null;
      if (owner !== null && this.isPointGuardFolder(owner.projectRoot)) {
        // Removed elsewhere, but Point Guard works in this folder: register it again, leaving
        // the agents the removal suspended paused.
        owner.keepsSuspendedAgentsOnNextOpen = true;
        this.startProject(owner);
      } else if (this.projects.some((project) => project.projectRoot === root)) {
        this.closeProject(root);
      }
    }
    const resuming = this.resumesOnFirstLocalList;
    this.resumesOnFirstLocalList = false;
    for (const root of roots) {
      if (this.projects.some((project) => project.projectRoot === root)) continue;
      // A Point Guard folder the user has not opened stays out of the sidebar.
      if (this.pointGuardProjects.has(root) && !this.shownPointGuardFolders.has(root)) continue;
      if (!this.pathExists(root) || this.isTooBroadProjectRoot(root)) continue;
      this.openProject(root, resuming);
    }
    // The daemon's list is in its saved order, which every device shows, unless it may predate a
    // move this PC is still saving.
    this.localProjectOrder = roots;
    if (this.localOrderSaver.pending === 0) this.sortLocalProjects(roots);
    this.lastLocalProjects = registered;
  }

  private sortLocalProjects(roots: readonly string[]): void {
    const ordered = sortedByProjectOrder(this.projects, roots, (project) => project.projectRoot);
    if (ordered.some((project, index) => project !== this.projects[index])) {
      this.store.setState({ projects: ordered });
    }
  }

  /** Until the local daemon first reports its projects, a saved selection may name one of them. */
  private get awaitingLocalProjects(): boolean {
    return this.followingLocalProjects && this.lastLocalProjects === null;
  }

  /** The machine a remote project lives on; null for a local one. */
  machine(project: ProjectState): MachineState | null {
    if (project.machineId === null) return null;
    return this.machines.find((machine) => machine.machine.id === project.machineId) ?? null;
  }

  // MARK: Machines

  /** Read the machine registry, connect every machine not disconnected, and watch the file. */
  async loadMachines(): Promise<void> {
    await this.reloadMachines();
    const directory = this.machineDirectory;
    if (directory?.watch !== undefined && this.stopMachineWatcher === null) {
      this.stopMachineWatcher = directory.watch(() => {
        void this.reloadMachines();
      });
    }
  }

  /**
   * Re-read the registry and diff against the current machines: connect new ones, update changed
   * ones, and remove any no longer registered.
   */
  async reloadMachines(): Promise<void> {
    const directory = this.machineDirectory;
    if (directory === null) return;
    let registered: Machine[];
    try {
      registered = await directory.load();
    } catch (error) {
      this.setDaemonError(
        `Could not read machines from ${directory.displayPath}: ${errorMessage(error)}`,
      );
      return;
    }
    let machines = [...this.machines];
    for (const machine of registered) {
      const existing = machines.find((state) => state.machine.id === machine.id);
      if (existing !== undefined) {
        existing.update(machine);
      } else {
        const state = directory.makeState(machine, this);
        machines.push(state);
        if (!directory.isUserDisconnected(machine.id)) state.connect();
      }
    }
    // Remove machines no longer in the registry; unsaved tabs keep their projects unavailable.
    const ids = new Set(registered.map((machine) => machine.id));
    for (const state of machines.filter((candidate) => !ids.has(candidate.machine.id))) {
      state.disconnect({ keepUnsaved: true });
      machines = machines.filter((candidate) => candidate !== state);
      const active = this.activeProjectRoot;
      if (active !== null && parseProjectKey(active).machineId === state.machine.id) {
        this.activeProjectRoot = this.projects[0]?.projectRoot ?? null;
      }
    }
    this.store.setState({ machines });
  }

  async addMachine(name: string, sshHost: string, id?: string): Promise<MachineState | null> {
    const directory = this.machineDirectory;
    if (directory === null) return null;
    const machine = await directory.add(name, sshHost, id);
    // The watcher may have reloaded meanwhile and added it already: reuse that state.
    const existing = this.machines.find((state) => state.machine.id === machine.id);
    if (existing !== undefined) {
      existing.update(machine);
      if (!directory.isUserDisconnected(machine.id)) existing.connect();
      return existing;
    }
    const state = directory.makeState(machine, this);
    this.store.setState({ machines: [...this.machines, state] });
    state.connect();
    return state;
  }

  /** Forget a machine: its forward closes and it leaves the registry; its daemon is untouched. */
  removeMachine(state: MachineState): void {
    state.disconnect();
    this.store.setState({ machines: this.machines.filter((candidate) => candidate !== state) });
    const { id, name } = state.machine;
    const directory = this.machineDirectory;
    if (directory !== null) {
      void directory.remove(id).catch((error: unknown) => {
        this.setDaemonError(
          `Could not remove ${name} from ${directory.displayPath}: ${errorMessage(error)}`,
        );
      });
    }
    const active = this.activeProjectRoot;
    if (active !== null && parseProjectKey(active).machineId === id) {
      this.activeProjectRoot = this.projects[0]?.projectRoot ?? null;
    }
  }

  // MARK: Active project routing

  /**
   * Update `activeProjectRoot` and `activeSidebarSelection` for a sidebar selection. Workspace,
   * worktree, channel and project selections resolve to their owning project; nav and null keep
   * the last-known project.
   */
  updateActiveProject(selection: SidebarSelection | null): void {
    const next = updateActiveProject(
      {
        activeProjectRoot: this.activeProjectRoot,
        activeSidebarSelection: this.activeSidebarSelection,
      },
      selection,
      {
        projects: this.managedProjects.map((project) => project.snapshot()),
        workspacesByProject: this.registry?.workspacesByProject ?? {},
      },
    );
    this.store.setState(next);
  }

  /** Set the active project if none is: a newly opened project can take Ctrl+N at once. */
  initializeActiveProjectIfNeeded(root: string): void {
    if (this.activeProjectRoot === null) this.activeProjectRoot = root;
  }

  /** Pi's configured folder is independent of the selected coding project. */
  updatePointGuardRoot(cwd: string): void {
    this.store.setState({ pointGuardRoot: cwd === '' ? null : standardizedPath(cwd) });
  }

  private ensurePointGuardProject(root: string): void {
    this.pointGuardFolders.add(root);
    if (this.projects.some((project) => project.projectRoot === root)) {
      this.shownPointGuardFolders.add(root);
    }
    this.persistOpenProjects();
    if (this.projectState(root) !== null) return;
    const project = this.makeProject(root);
    this.pointGuardProjects.set(root, project);
    // Broad roots have no ProjectState owner for routing; skip daemon registration.
    if (this.isTooBroadProjectRoot(root)) return;
    this.startProject(project);
  }

  activatePointGuardWorkspace(): string | null {
    const registry = this.registry;
    if (registry === null) return null;
    const root = this.pointGuardRoot;
    if (root === null || this.isTooBroadProjectRoot(root)) {
      registry.activeWorkspaceId = null;
      return null;
    }
    this.ensurePointGuardProject(root);
    let id: string;
    try {
      id = registry.ensurePointGuardWorkspace(root);
    } catch {
      registry.activeWorkspaceId = null;
      return null;
    }
    registry.activate(id);
    return id;
  }

  /** Ctrl+N uses the same folder and palette as Point Guard splits. */
  newPointGuardTab(): void {
    const registry = this.registry;
    const id = this.activatePointGuardWorkspace();
    if (registry === null || id === null) return;
    const workspace = registry.workspace(id);
    if (workspace === null) return;
    registry.pendingPaletteSurfaceId = registry.newTab(id, workspace.focusedLeafId);
  }

  /**
   * The pane's own local definitions as well as definitions from visible projects. The owning root
   * comes first so a Point Guard-only session still queries global definitions.
   */
  paletteProjectRoots(workspaceId: string): string[] {
    const visibleRoots = this.projects.map((project) => project.projectRoot);
    // Definitions come from the local daemon, so a remote workspace adds no root of its own.
    const root = this.registry?.projectRoot(workspaceId) ?? null;
    if (root === null || parseProjectKey(root).machineId !== null) return visibleRoots;
    return [root, ...visibleRoots.filter((candidate) => candidate !== root)];
  }

  // MARK: Cross-project queries

  agent(id: string): AgentModel | null {
    for (const project of this.managedProjects) {
      const agent = project.agent(id);
      if (agent !== null) return agent;
    }
    return null;
  }

  projectStateForAgent(agentId: string): ProjectState | null {
    return this.managedProjects.find((project) => project.agent(agentId) !== null) ?? null;
  }

  projectStateForWorktree(worktreeId: string): ProjectState | null {
    return (
      this.managedProjects.find((project) =>
        project.worktrees.some((worktree) => worktree.id === worktreeId),
      ) ?? null
    );
  }

  /** The project with this key (`ProjectKey` id; a local project's id is its root). */
  projectState(id: string): ProjectState | null {
    return this.managedProjects.find((project) => project.projectId === id) ?? null;
  }

  projectStateForWorkspace(workspaceId: string): ProjectState | null {
    const root = this.registry?.projectRoot(workspaceId) ?? null;
    return root === null ? null : this.projectState(root);
  }

  agentIdForSession(sessionId: string): string | null {
    for (const project of this.managedProjects) {
      const agent = project.allAgents.find((candidate) => candidate.sessionId === sessionId);
      if (agent !== undefined) return agent.id;
    }
    return null;
  }

  worktreeIdForPath(path: string): string | null {
    const normalized = comparablePath(standardizedPath(path));
    for (const project of this.managedProjects) {
      const worktree = project.worktrees.find(
        (candidate) => comparablePath(standardizedPath(candidate.path)) === normalized,
      );
      if (worktree !== undefined) return worktree.id;
    }
    return null;
  }

  /** The directory a workspace's file panes are rooted at: its worktree, else the project root. */
  fileRoot(workspaceId: string): string | null {
    const registry = this.registry;
    const projectId = registry?.projectRoot(workspaceId) ?? null;
    if (registry === null || projectId === null) return null;
    const project = this.projectState(projectId);
    const container = registry.workspace(workspaceId)?.container;
    if (container?.kind === 'worktree') {
      const path = project?.worktrees.find((w) => w.id === container.worktreeId)?.path;
      if (path !== undefined) return path;
    }
    return project?.projectRoot ?? parseProjectKey(projectId).root;
  }

  /** The daemon owning an agent, so terminals attach to the right machine. */
  client(agentId: string): DaemonRequester | null {
    return this.projectStateForAgent(agentId)?.client ?? null;
  }

  // MARK: Agent signals (WIN-7.7)

  /** Every pane holding a surface, per project, for notification routing. */
  get signalRoutingTargets(): SignalRoutingTarget[] {
    const registry = this.registry;
    if (registry === null) return [];
    return Object.entries(registry.workspacesByProject).flatMap(([projectId, workspaces]) =>
      workspaces.flatMap((workspace, ordinal) =>
        workspaceSurfaces(workspace).map(({ leafId, surface }) => ({
          projectId,
          worktreeId:
            workspace.container.kind === 'worktree' ? workspace.container.worktreeId : null,
          workspaceId: workspace.id,
          agentId: surface.content.kind === 'agent' ? surface.content.agentId : null,
          leafId,
          surfaceId: surface.id,
          workspaceOrdinal: ordinal,
        })),
      ),
    );
  }

  /** The workspace whose panes are on screen for the sidebar's selection. */
  private get visibleSignalWorkspaceId(): string | null {
    const selection = this.activeSidebarSelection;
    if (selection?.kind === 'workspace') return selection.id;
    const active = this.registry?.activeWorkspace ?? null;
    if (selection?.kind === 'nav' && selection.item === 'dashboard' && active !== null) {
      return isPointGuard(active) ? active.id : null;
    }
    return null;
  }

  /** The window gained or lost focus: toasts post only while it is unfocused. */
  setWindowFocused(focused: boolean): void {
    if (this.windowFocused === focused) return;
    this.windowFocused = focused;
    this.signalNotifications?.windowFocusChanged(focused);
    this.updateAgentSignals();
  }

  /** The current agents as signal alerts, and the one on screen (Swift `updateAgentSignals`). */
  signalAlertAgents(nowMs: number = Date.now()): {
    agents: SignalAlertAgent[];
    foreground: string | null;
  } {
    let foreground: string | null = null;
    const targets = this.signalRoutingTargets;
    const visibleWorkspaceId = this.visibleSignalWorkspaceId;
    const agents = this.managedProjects.flatMap((project) =>
      project.allAgents.map((agent) => {
        const worktreeId = project.worktreeId(agent.id);
        const identity = opaqueKey([project.projectId, worktreeId, agent.id]);
        const presentation = agentSignalPresentation(agent, !project.isUnavailable, nowMs);
        const signal = agent.signal;
        let destination: AgentSignalDestination | null = null;
        let target: SignalRoutingTarget | null = null;
        if (signal !== null && presentation.eventId !== null) {
          const candidate: AgentSignalDestination = {
            projectId: project.projectId,
            worktreeId,
            agentId: agent.id,
            launchId: signal.identity.launch_id,
            generation: signal.identity.generation,
            nativeSessionId: signal.identity.native_session_id,
            eventId: presentation.eventId,
          };
          target = signalRoutingTarget(candidate, targets);
          if (target !== null && destinationMatches(candidate, signal, nowMs)) {
            destination = candidate;
          }
        }
        if (
          destination !== null &&
          target?.workspaceId === visibleWorkspaceId &&
          isSignalVisible(
            {
              appActive: this.windowFocused,
              keyWindow: this.windowFocused,
              settingsVisible: this.s.showSettings,
              selectedWorkspaceId: visibleWorkspaceId,
              activeWorkspaceId: this.registry?.activeWorkspaceId ?? null,
              focusedAgentId: this.focusedAgentId,
              activeProjectId: this.activeProjectRoot,
            },
            destination,
          )
        ) {
          foreground = identity;
        }
        return makeSignalAlertAgent({
          identity,
          launch: signal?.identity.launch_id ?? '',
          presentation,
          qualificationKey: signal === null ? null : signalQualificationKey(signal),
          destination,
        });
      }),
    );
    return { agents, foreground };
  }

  /** Project the agents' signals into the badge and toasts; the shell runs it every second. */
  updateAgentSignals(nowMs: number = Date.now()): void {
    const notifications = this.signalNotifications;
    if (notifications === null) return;
    const { agents, foreground } = this.signalAlertAgents(nowMs);
    notifications.open = (agent) => Promise.resolve(this.openSignal(agent));
    notifications.update(agents, foreground);
  }

  /** Whether a destination still names a live, reachable agent's same launch, session and event. */
  private signalDestinationIsCurrent(destination: AgentSignalDestination, nowMs: number): boolean {
    const project = this.projectState(destination.projectId);
    const agent = project?.agent(destination.agentId) ?? null;
    return (
      project !== null &&
      agent !== null &&
      destinationMatches(destination, agent.signal, nowMs) &&
      isAgentAlive(agent) &&
      !agent.suspended &&
      !project.isUnavailable &&
      signalRoutingTarget(destination, this.signalRoutingTargets) !== null
    );
  }

  /** Route a notification to its agent (Swift `signalNotifications.open`); whether it did. */
  openSignal(agent: SignalAlertAgent, nowMs: number = Date.now()): boolean {
    const destination = agent.destination;
    if (destination === null || !this.signalDestinationIsCurrent(destination, nowMs)) return false;
    this.setPendingSignalDestination(destination);
    return true;
  }

  /**
   * The pending notification destination's pane, consumed once (Swift ContentView
   * `selectSignalDestination`): the active project is set and the pane's tab selected and
   * focused. Ambiguous or expired destinations never fall back to a global ID lookup.
   */
  takeSignalDestination(nowMs: number = Date.now()): SignalRoutingTarget | null {
    const destination = this.s.pendingSignalDestination;
    if (destination === null) return null;
    this.setPendingSignalDestination(null);
    const registry = this.registry;
    if (registry === null || !this.signalDestinationIsCurrent(destination, nowMs)) return null;
    const target = signalRoutingTarget(destination, this.signalRoutingTargets);
    if (target === null) return null;
    this.activeProjectRoot = destination.projectId;
    registry.selectTab(target.workspaceId, target.surfaceId);
    registry.setFocus(target.workspaceId, target.leafId);
    return target;
  }

  // MARK: Lifecycle

  /**
   * Suspend this app's agents and shut the daemon down on quit, within five seconds. Only an
   * instance that launched the daemon stops anything: a second instance attached to a shared
   * daemon must leave the owner's agents running.
   */
  async shutdownWithSuspend(timeoutMs = 5000): Promise<void> {
    this.persistSelectedAgent();
    this.registry?.saveAll();
    for (const project of this.managedProjects) project.stopWatching();
    for (const machine of this.machines) machine.disconnect();
    this.stopMachineWatcher?.();
    this.stopMachineWatcher = null;
    this.stopFollowingLocalProjects();
    const roots = this.localManagedProjects.map((project) => project.projectRoot);
    const launched =
      this.s.daemonConnection.state === 'connected' && this.s.daemonConnection.launched;
    const work = (async () => {
      await this.persistence?.flush();
      if (!launched) return;
      for (const root of roots) {
        await this.daemon
          .send({ type: 'suspend', project_root: root, target: 'all' })
          .catch(() => null);
      }
      await this.daemon.send({ type: 'shutdown' }).catch(() => null);
    })();
    let timer: ReturnType<typeof setTimeout> | undefined;
    await Promise.race([
      work,
      new Promise<void>((resolve) => {
        timer = setTimeout(resolve, timeoutMs);
      }),
    ]);
    clearTimeout(timer);
  }

  // MARK: Selection persistence

  private persistSelectedAgent(): void {
    saveWorkspaceSelection(this.registry?.activeWorkspaceId ?? null, this.storage);
  }

  /**
   * Remember the workspace that was on screen so the UI can select it once it exists. Reconcile
   * may not have run for every project yet, so existence is not checked here.
   */
  restoreActiveWorkspace(): string | null {
    const saved = loadWorkspaceSelection(this.storage);
    if (saved === null) return null;
    this.pendingSelectWorkspaceId = saved;
    return saved;
  }

  /**
   * The pending workspace to select, once the registry has it. Cleared when it resolves, or when
   * every open local project has reconciled and it still names nothing (WIN-6.2's helper).
   */
  resolvePendingWorkspaceSelection(): string | null {
    const registry = this.registry;
    const pendingId = this.pendingSelectWorkspaceId;
    if (pendingId === null || registry === null) return null;
    const resolution = resolvePending(registry, {
      pendingId,
      awaitingLocalProjects: this.awaitingLocalProjects,
      localProjectIds: this.localManagedProjects.map((project) => project.projectId),
    });
    this.pendingSelectWorkspaceId = resolution.pendingId;
    if (resolution.activeProjectRoot !== null) {
      this.activeProjectRoot = resolution.activeProjectRoot;
    }
    return resolution.selectedId;
  }

  // MARK: Private

  /** The daemon keeps which projects exist; this PC only keeps which Point Guard folders it shows. */
  private persistOpenProjects(): void {
    writeList(this.storage, SHOWN_POINT_GUARD_FOLDERS_KEY, [...this.shownPointGuardFolders]);
    writeList(this.storage, POINT_GUARD_FOLDERS_KEY, [...this.pointGuardFolders]);
  }
}
