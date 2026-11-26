// Port of apps/purepoint-macos/purepoint-macos/State/ProjectState.swift (WIN-6.0).
//
// Per-project state: root agents, worktrees, manifest watching and daemon interaction. AppState
// holds one per open project.
//
// Shape (DEC-14): the fields Swift's `@Observable` publishes (`rootAgents`, `worktrees`,
// `hasLoadedWorktrees`, `isUnavailable`) live in a Zustand store, `project.store`; everything Swift
// marks `@ObservationIgnored` stays private to the object. Swift's singletons (the daemon
// lifecycle, manifest watchers, subscriptions) are injected as a `ProjectEnvironment` so tests can
// stand in for the daemon.
//
// The backend's status stream and manifest watcher raise app-wide Tauri events, so AppState owns
// the listeners and forwards each event to its project: `applyStatusEvent`,
// `handleSubscriptionState` and `handleManifestChanged`.
//
// Differences from Swift:
// - The layout watcher (`.pu/workspaces.json`) is folded into `refresh`, which runs on every manifest
//   change: it re-stats the document through the cached persistence adapter and adopts a change.
// - `channel` (ChannelState, WIN-8.10) polls through the project's daemon client; its unread count
//   feeds `snapshot().channelUnread`, and AppState follows `channel.store` for it.
// - A remote project is followed through its machine's daemon client (WIN-8.14); its layout loads
//   and saves through that daemon's `fs_*` requests.

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { AgentStatusReport, Request, Response, WorktreeEntry } from '../protocol';
import type { CommandPaletteResult } from '../model/commandPalette';
import { lastPathComponent, projectKey, projectKeyId } from '../model/projectKey';
import type { ProjectSnapshot, SidebarSelection } from '../model/sidebarItem';
import {
  loadRemoteLayout,
  workspacesFromText,
  type LayoutHostIO,
} from '../model/workspacePersistence';
import {
  agentDisplayName,
  structurallyEqual,
  type AgentModel,
  type WorktreeModel,
} from '../model/workspaceModel';
import { containerWorktreeId, surfaceById } from '../model/workspace';
import { daemonLayoutHostIO } from '../services/fs';
import {
  DaemonClientError,
  localDaemonClient,
  type DaemonRequester,
} from '../services/daemonConnection';
import {
  startProjectSubscriptions,
  stopProjectSubscriptions,
  type SubscriptionStateEvent,
} from '../services/subscriptions';
import { sendReleasingWatchers, unwatch, watchManifest, type WatchId } from '../services/watchers';
import {
  DaemonWorkspaceService,
  workspaceSnapshot,
  type WorkspaceService,
  type WorkspaceSnapshot,
} from '../services/workspaceService';
import type { CachedWorkspacePersistence } from './cachedWorkspacePersistence';
import { ChannelState } from './channelState';
import { RemoteLayoutSaver } from './remoteLayoutSaver';
import { resolveSpawnTarget } from './spawnTargetResolver';
import type { WorkspaceRegistry } from './workspaceRegistry';

// MARK: - Seams

/** What ProjectState reaches through Swift singletons. */
export interface ProjectEnvironment {
  startSubscriptions(projectRoot: string): Promise<void>;
  stopSubscriptions(projectRoot: string): Promise<unknown>;
  watchManifest(projectRoot: string): Promise<WatchId>;
  unwatch(id: WatchId): Promise<unknown>;
  /** Sends a request, releasing the watchers of a worktree it removes (WIN-3.13). */
  sendReleasingWatchers(
    request: Request,
    send: (request: Request) => Promise<Response>,
  ): Promise<Response>;
  sleep(ms: number): Promise<void>;
  now(): number;
  log(message: string): void;
}

export const DEFAULT_PROJECT_ENVIRONMENT: ProjectEnvironment = {
  startSubscriptions: startProjectSubscriptions,
  stopSubscriptions: stopProjectSubscriptions,
  watchManifest,
  unwatch,
  sendReleasingWatchers,
  sleep: (ms) =>
    new Promise((resolve) => {
      setTimeout(resolve, ms);
    }),
  now: () => Date.now(),
  log: (message) => {
    console.warn(message);
  },
};

/** The AppState members a project reaches (Swift's `weak var appState`). */
export interface ProjectStateOwner {
  setDaemonError(message: string | null): void;
  projectState(id: string): ProjectState | null;
  setActiveProjectRoot(root: string | null): void;
  setPendingSelectAgentId(agentId: string | null): void;
  setPendingSelectWorktreeId(worktreeId: string | null): void;
}

export interface ProjectStateFields {
  readonly rootAgents: readonly AgentModel[];
  readonly worktrees: readonly WorktreeModel[];
  /**
   * Whether a snapshot of this project's manifest has been applied at least once. `worktrees` is
   * empty both before that and for a project that genuinely has none.
   */
  readonly hasLoadedWorktrees: boolean;
  /**
   * A remote project kept in the sidebar while its machine is unreachable, because it has file
   * tabs with unsaved edits. Nothing is watched until the daemon is back.
   */
  readonly isUnavailable: boolean;
}

export interface ProjectStateOptions {
  readonly projectRoot: string;
  /** The machine that owns the project; null for this PC. */
  readonly machineId?: string | null;
  readonly service?: WorkspaceService;
  readonly registry?: WorkspaceRegistry | null;
  /** The project's daemon. Defaults to the local daemon. */
  readonly client?: DaemonRequester;
  /** Local projects: the registry's cached persistence, opened before the first reconcile. */
  readonly layout?: CachedWorkspacePersistence | null;
  /** Remote projects: where the layout document is read and written. */
  readonly layoutHost?: LayoutHostIO;
  readonly environment?: Partial<ProjectEnvironment>;
  /** The project's channel; defaults to one over `client`, scoped by the project's key. */
  readonly channel?: ChannelState;
}

/** How long to keep retrying daemon start + init before surfacing an error. */
export const DAEMON_CONNECT_BUDGET_MS = 20_000;
/** The delayed resubscribe after a remote daemon was lost. */
export const REMOTE_RESUBSCRIBE_DELAY_MS = 5_000;

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

type Token = object;

const withoutSignal = (agent: AgentModel): AgentModel =>
  agent.signal === null ? agent : { ...agent, signal: null };

// MARK: - ProjectState

export class ProjectState {
  /** The root on the machine that owns the project: what daemon requests and file paths use. */
  readonly projectRoot: string;
  /** Identity `(machine, root)` as a string: keys the registry, sidebar and selection. */
  readonly projectId: string;
  readonly machineId: string | null;
  readonly store: StoreApi<ProjectStateFields>;
  readonly client: DaemonRequester;
  /** Remote projects write their layout through their machine; quit and disconnect flush it. */
  readonly remoteLayoutSaver: RemoteLayoutSaver | null;
  readonly channel: ChannelState;

  registry: WorkspaceRegistry | null;
  owner: ProjectStateOwner | null = null;
  /**
   * Leaves suspended agents paused on the next open: a project another device or the CLI added
   * was not suspended by this app's quit, so its benched agents stay benched.
   */
  keepsSuspendedAgentsOnNextOpen = false;

  private readonly service: WorkspaceService;
  private readonly layout: CachedWorkspacePersistence | null;
  private readonly layoutHost: LayoutHostIO | null;
  private readonly env: ProjectEnvironment;

  private openTask: Token | null = null;
  private refreshTask: Token | null = null;
  private remoteLayoutTask: Token | null = null;
  private resubscribeTask: Token | null = null;
  private manifestWatch: Promise<WatchId | null> | null = null;
  private subscribed = false;
  /** Resolves once the local layout document has been read (the registry may reconcile). */
  private layoutReady: Promise<void> = Promise.resolve();
  /**
   * Agents killed locally whose removal the daemon has not written to the manifest yet. Without
   * this, a refresh landing in that window would re-adopt the dead agent into a new workspace.
   */
  private readonly killedAgentIds = new Set<string>();

  constructor(options: ProjectStateOptions) {
    this.projectRoot = options.projectRoot;
    this.machineId = options.machineId ?? null;
    this.projectId = projectKeyId(projectKey(this.projectRoot, this.machineId));
    this.client = options.client ?? localDaemonClient;
    this.service = options.service ?? new DaemonWorkspaceService(this.client);
    this.registry = options.registry ?? null;
    this.layout = this.machineId === null ? (options.layout ?? null) : null;
    this.layoutHost =
      this.machineId === null ? null : (options.layoutHost ?? daemonLayoutHostIO(this.client));
    this.remoteLayoutSaver =
      this.layoutHost === null
        ? null
        : RemoteLayoutSaver.forHost(this.projectRoot, this.layoutHost);
    this.env = { ...DEFAULT_PROJECT_ENVIRONMENT, ...options.environment };
    const client = this.client;
    this.channel =
      options.channel ??
      new ChannelState({
        projectRoot: this.projectRoot,
        // Swift: local projects scope their drafts and read marks by root, remote ones by key.
        ...(this.machineId === null ? {} : { defaultsScope: this.projectId }),
        sendRequest: (request) => client.send(request),
      });
    this.store = createStore<ProjectStateFields>()(() => ({
      rootAgents: [],
      worktrees: [],
      hasLoadedWorktrees: false,
      isUnavailable: false,
    }));
  }

  // MARK: Observable accessors

  get id(): string {
    return this.projectId;
  }

  get isRemote(): boolean {
    return this.machineId !== null;
  }

  get projectName(): string {
    return lastPathComponent(this.projectRoot);
  }

  get rootAgents(): readonly AgentModel[] {
    return this.store.getState().rootAgents;
  }

  get worktrees(): readonly WorktreeModel[] {
    return this.store.getState().worktrees;
  }

  get hasLoadedWorktrees(): boolean {
    return this.store.getState().hasLoadedWorktrees;
  }

  get isUnavailable(): boolean {
    return this.store.getState().isUnavailable;
  }

  set isUnavailable(value: boolean) {
    this.store.setState({ isUnavailable: value });
  }

  /** Stands in for the daemon in tests, as Swift's `markWorktreesLoadedForTesting`. */
  markWorktreesLoadedForTesting(): void {
    this.store.setState({ hasLoadedWorktrees: true });
  }

  /** What the sidebar reads (WIN-6.5's `ProjectSnapshot`). */
  snapshot(): ProjectSnapshot {
    const s = this.store.getState();
    return {
      projectId: this.projectId,
      projectRoot: this.projectRoot,
      rootAgents: s.rootAgents,
      worktrees: s.worktrees,
      hasLoadedWorktrees: s.hasLoadedWorktrees,
      isUnavailable: s.isUnavailable,
      channelUnread: this.channel.unreadCount,
      layoutError: this.registry?.layoutErrorsByProject[this.projectId] ?? null,
    };
  }

  // MARK: Lifecycle

  startWatching(): void {
    if (this.isRemote) {
      this.startWatchingRemote();
      return;
    }
    const root = this.projectRoot;
    // One open only, whether or not it succeeds.
    const keepsSuspendedAgents = this.keepsSuspendedAgentsOnNextOpen;
    this.keepsSuspendedAgentsOnNextOpen = false;

    const task: Token = {};
    this.openTask = task;
    this.refreshTask = null;
    this.stopManifestWatch();
    this.store.setState({ hasLoadedWorktrees: false });
    const layout = this.layout;
    if (layout !== null && !layout.isOpen(this.projectId)) {
      this.layoutReady = layout.open(this.projectId);
    }
    const current = () => this.openTask === task;

    void (async () => {
      // Retry with backoff: right after an update relaunch the previous daemon can still be
      // draining its agents and holding the daemon lock, so the first attempt fails. Bounded by
      // wall-clock time, not attempts.
      const deadline = this.env.now() + DAEMON_CONNECT_BUDGET_MS;
      let attempt = 0;
      for (;;) {
        try {
          // The backend ensures a daemon before every request.
          const response = await this.client.send({ type: 'init', project_root: root });
          if (!current()) return;
          if (response.type === 'error') {
            // At its connection limit: transient, so back off and retry.
            if (response.code === 'BUSY') throw new DaemonClientError('busy');
            this.owner?.setDaemonError(response.message);
            return;
          }
          break;
        } catch (error) {
          if (!current()) return;
          attempt += 1;
          if (this.env.now() >= deadline) {
            this.owner?.setDaemonError(errorMessage(error));
            return;
          }
          this.env.log(
            `[Daemon] connect attempt ${String(attempt)} failed: ${errorMessage(error)}; retrying`,
          );
          await this.env.sleep(Math.min(attempt, 4) * 1000);
          if (!current()) return;
        }
      }

      this.manifestWatch = this.env.watchManifest(root).then(
        (id) => id,
        () => null,
      );
      this.channel.startBackground();
      this.startSubscriptions();
      // Resume from the snapshot taken after init, not after a fixed delay: a slow first load
      // would otherwise resume against the agents from before init and leave new ones paused.
      try {
        const snapshot = await this.service.loadWorkspace(root);
        await this.layoutReady;
        if (!current()) return;
        this.apply(snapshot);
        this.store.setState({ hasLoadedWorktrees: true });
      } catch (error) {
        if (!current()) return;
        this.owner?.setDaemonError(errorMessage(error));
        return;
      }
      if (!keepsSuspendedAgents) this.resumeSuspendedAgents();
    })();
  }

  /**
   * A remote daemon is never started or initialized from here (MACH-003): the machine's connection
   * owns reachability. Load the layout through the host, then read status like a local project.
   */
  private startWatchingRemote(): void {
    const task: Token = {};
    this.openTask = task;
    this.refreshTask = null;
    this.resubscribeTask = null;
    this.store.setState({ hasLoadedWorktrees: false });
    this.startRemoteLayoutRestore();
    this.channel.startBackground();
    void (async () => {
      try {
        const snapshot = await this.service.loadWorkspace(this.projectRoot);
        if (this.openTask !== task) return;
        this.apply(snapshot);
        this.store.setState({ hasLoadedWorktrees: true });
      } catch (error) {
        if (this.openTask !== task) return;
        // The machine row reports reachability; the status stream retries on its own.
        this.env.log(`[Remote ${this.projectId}] status failed: ${errorMessage(error)}`);
      }
    })();
  }

  /** (Re)start reading the remote layout: Retry Layout tries now instead of after the backoff. */
  retryRemoteLayout(): void {
    if (!this.isRemote || this.isUnavailable) return;
    this.startRemoteLayoutRestore();
  }

  private startRemoteLayoutRestore(): void {
    this.remoteLayoutTask = null;
    const registry = this.registry;
    const saver = this.remoteLayoutSaver;
    const host = this.layoutHost;
    if (registry === null || saver === null || host === null) return;
    const task: Token = {};
    this.remoteLayoutTask = task;
    void ProjectState.restoreRemoteLayout({
      projectRoot: this.projectRoot,
      projectId: this.projectId,
      host,
      registry,
      saver,
      isCancelled: () => this.remoteLayoutTask !== task,
      sleep: (ms) => this.env.sleep(ms),
    });
  }

  /**
   * Read a remote layout through its host until it arrives, then adopt it and route the project's
   * saves to `saver`. A failed read (anything but a missing file) surfaces as a layout error and is
   * retried with backoff; the saver is not registered meanwhile, so no empty layout can be saved
   * over the real one.
   */
  static async restoreRemoteLayout(options: {
    projectRoot: string;
    projectId: string;
    host: LayoutHostIO;
    registry: WorkspaceRegistry;
    saver: RemoteLayoutSaver;
    isCancelled: () => boolean;
    sleep: (ms: number) => Promise<void>;
    retryDelayMs?: (attempt: number) => number;
  }): Promise<void> {
    const { projectRoot, projectId, host, registry, saver, isCancelled } = options;
    const retryDelayMs = options.retryDelayMs ?? ((attempt) => Math.min(2 * attempt, 30) * 1000);
    let attempt = 0;
    while (!isCancelled()) {
      try {
        const layout = await loadRemoteLayout(host, projectRoot, projectId);
        if (isCancelled()) return;
        saver.loaded(layout.modifiedMs);
        saver.onConflict((document, modified) => {
          registry.adoptExternalLayout(projectId, workspacesFromText(document, projectId));
          saver.loaded(modified);
        });
        registry.registerRemoteProject(projectId, layout.workspaces, (workspaces, active) => {
          saver.save(workspaces, active, () => {
            registry.layoutWritten(projectId, [...workspaces]);
          });
        });
        return;
      } catch (error) {
        if (isCancelled()) return;
        attempt += 1;
        registry.setRemoteLayoutError(projectId, errorMessage(error));
        await options.sleep(retryDelayMs(attempt));
      }
    }
  }

  stopWatching(): void {
    this.invalidateSignals();
    this.channel.shutdown();
    this.openTask = null;
    this.remoteLayoutTask = null;
    this.resubscribeTask = null;
    this.refreshTask = null;
    this.stopManifestWatch();
    if (this.subscribed) {
      this.subscribed = false;
      void this.env.stopSubscriptions(this.projectRoot).catch(() => false);
    }
  }

  private stopManifestWatch(): void {
    const watch = this.manifestWatch;
    this.manifestWatch = null;
    void watch?.then((id) => (id === null ? false : this.env.unwatch(id).catch(() => false)));
  }

  private startSubscriptions(): void {
    this.subscribed = true;
    void this.env.startSubscriptions(this.projectRoot).catch((error: unknown) => {
      this.env.log(`[Subscriptions ${this.projectId}] ${errorMessage(error)}`);
    });
  }

  // MARK: Events (forwarded by AppState)

  /** A `status-event` for this project: the full status, on subscribe and after every change. */
  applyStatusEvent(
    worktrees: readonly WorktreeEntry[],
    agents: readonly AgentStatusReport[],
  ): void {
    const snapshot = workspaceSnapshot(worktrees, agents);
    const task = this.openTask;
    void this.layoutReady.then(() => {
      if (task !== null && this.openTask !== task) return;
      this.apply(snapshot);
    });
  }

  /** `subscription-state` for this project's status stream. */
  handleSubscriptionState(event: SubscriptionStateEvent): void {
    if (event.kind !== 'status' || event.state !== 'daemon_lost') return;
    // The daemon is gone (crashed, or exited with the instance that launched it). startWatching
    // inits and resumes, so this project's agents come back on the new one.
    this.invalidateSignals();
    if (!this.isRemote) {
      this.startWatching();
      return;
    }
    // Never start a remote daemon; resubscribe once it answers again.
    const task: Token = {};
    this.resubscribeTask = task;
    void this.env.sleep(REMOTE_RESUBSCRIBE_DELAY_MS).then(() => {
      if (this.resubscribeTask === task) this.startWatching();
    });
  }

  /** `manifest-changed` for this project. */
  handleManifestChanged(): void {
    this.refresh();
  }

  // MARK: Data

  private invalidateSignals(): void {
    const s = this.store.getState();
    this.store.setState({
      rootAgents: s.rootAgents.map(withoutSignal),
      worktrees: s.worktrees.map((worktree) => ({
        ...worktree,
        agents: worktree.agents.map(withoutSignal),
      })),
    });
  }

  refresh(): void {
    const root = this.projectRoot;
    // Swift's layout watcher: the document may have changed (or appeared) under this app.
    const layout = this.layout;
    if (layout !== null && layout.isOpen(this.projectId)) {
      void layout.refresh(this.projectId).then((changed) => {
        if (changed) this.registry?.adoptLocalLayoutIfChanged(this.projectId);
      });
    }
    const task: Token = {};
    this.refreshTask = task;
    void (async () => {
      try {
        const snapshot = await this.service.loadWorkspace(root);
        await this.layoutReady;
        if (this.refreshTask !== task) return;
        this.apply(snapshot);
        this.store.setState({ hasLoadedWorktrees: true });
      } catch (error) {
        if (this.refreshTask !== task) return;
        this.owner?.setDaemonError(errorMessage(error));
      }
    })();
  }

  // MARK: Queries

  agent(id: string): AgentModel | null {
    for (const worktree of this.worktrees) {
      const agent = worktree.agents.find((candidate) => candidate.id === id);
      if (agent !== undefined) return agent;
    }
    return this.rootAgents.find((candidate) => candidate.id === id) ?? null;
  }

  get allAgents(): AgentModel[] {
    return [...this.worktrees.flatMap((worktree) => worktree.agents), ...this.rootAgents];
  }

  worktreeId(agentId: string): string | null {
    return (
      this.worktrees.find((worktree) => worktree.agents.some((agent) => agent.id === agentId))
        ?.id ?? null
    );
  }

  // MARK: Agent operations

  createAgent(options: {
    agent: string;
    prompt: string;
    name?: string | null;
    isWorktree?: boolean;
    selection: SidebarSelection | null;
    command?: string | null;
  }): void {
    const target = resolveSpawnTarget({
      isWorktree: options.isWorktree ?? false,
      selection: options.selection,
      worktreeIdForWorkspace: (id) => {
        const workspace = this.registry?.workspace(id) ?? null;
        return workspace === null ? null : containerWorktreeId(workspace.container);
      },
    });
    this.sendDaemonRequest(
      {
        type: 'spawn',
        project_root: this.projectRoot,
        prompt: options.prompt,
        agent: options.agent,
        name: options.name ?? null,
        root: target.root,
        worktree: target.worktree,
        command: options.command ?? null,
      },
      (response) => {
        if (response.type !== 'spawn_result') return;
        if (this.owner?.projectState(this.projectId) != null) {
          this.owner.setActiveProjectRoot(this.projectId);
        }
        this.owner?.setPendingSelectAgentId(response.agent_id);
      },
    );
  }

  /**
   * Spawn an agent into a specific tab. The tab is reserved before the request goes out, so
   * whichever arrives first (the manifest write or the spawn response) the agent lands in this tab.
   * The request also names the tab, so any other Mac or PC showing this project places it there.
   */
  spawnAgentForSurface(options: {
    agent: string;
    prompt: string;
    workspaceId: string;
    surfaceId: number;
    name?: string | null;
    command?: string | null;
  }): void {
    const registry = this.registry;
    const workspace = registry?.workspace(options.workspaceId) ?? null;
    if (registry === null || workspace === null) return;
    const root = this.projectId;
    const { workspaceId, surfaceId } = options;
    const spawnWorktree = containerWorktreeId(workspace.container);

    registry.reserveSurface(root, workspaceId, surfaceId);
    const request: Request = {
      type: 'spawn',
      project_root: this.projectRoot,
      prompt: options.prompt,
      agent: options.agent,
      name: options.name ?? null,
      root: spawnWorktree === null,
      worktree: spawnWorktree,
      command: options.command ?? null,
      placement: { workspace: workspaceId, surface: surfaceId },
    };

    // Another app showing this project must see the new tab before the agent appears, or it has
    // nowhere to put it: write the layout first.
    registry.saveNow(root);
    const saver = this.remoteLayoutSaver;
    if (saver === null) {
      void (async () => {
        await this.layout?.flush();
        this.sendSurfaceSpawn(request, workspaceId, surfaceId);
      })();
      return;
    }
    void (async () => {
      // A save that met another machine's write merges and queues the merge; write that too.
      for (let pass = 0; pass < 2; pass += 1) {
        await saver.flush(5000);
        if (!registry.hasPendingSave(root)) break;
        registry.saveNow(root);
      }
      // The tab may have been closed, or the project closed, while the layout was written.
      const current = registry.workspace(workspaceId);
      const surface = current === null ? null : surfaceById(current, surfaceId);
      if (surface?.content.kind !== 'empty') {
        registry.releaseReservation(root, workspaceId, surfaceId);
        return;
      }
      this.sendSurfaceSpawn(request, workspaceId, surfaceId);
    })();
  }

  private sendSurfaceSpawn(request: Request, workspaceId: string, surfaceId: number): void {
    const root = this.projectId;
    const registry = this.registry;
    this.sendDaemonRequest(
      request,
      (response) => {
        if (response.type === 'spawn_result') {
          registry?.fulfillReservation(root, workspaceId, surfaceId, response.agent_id);
        } else {
          registry?.releaseReservation(root, workspaceId, surfaceId);
        }
      },
      () => {
        registry?.releaseReservation(root, workspaceId, surfaceId);
      },
    );
  }

  createWorktree(name: string | null): void {
    this.sendDaemonRequest(
      { type: 'create_worktree', project_root: this.projectRoot, name },
      (response) => {
        if (response.type !== 'create_worktree_result') return;
        if (this.owner?.projectState(this.projectId) != null) {
          this.owner.setActiveProjectRoot(this.projectId);
        }
        this.owner?.setPendingSelectWorktreeId(response.worktree_id);
      },
    );
  }

  /**
   * Swift `handlePaletteResult(_:selection:hub:)`. `runLocalSwarm` stands in for the agents hub
   * (WIN-8.7); without it a local swarm runs through the daemon like a remote one.
   */
  handlePaletteResult(
    result: CommandPaletteResult,
    selection: SidebarSelection | null,
    runLocalSwarm?: (projectRoot: string, name: string) => Promise<void>,
  ): void {
    switch (result.kind) {
      case 'spawnBuiltIn':
        this.createAgent({
          agent: result.variant.id,
          prompt: result.prompt ?? '',
          name: result.name,
          isWorktree: result.variant.kind === 'worktree',
          selection,
        });
        return;
      case 'spawnAgentDef':
        this.createAgent({
          agent: result.def.agent_type,
          prompt: result.prompt ?? result.def.inline_prompt ?? '',
          name: result.name,
          selection,
          command: result.def.command ?? null,
        });
        return;
      case 'runSwarm':
        if (this.isRemote || runLocalSwarm === undefined) {
          // The swarm runs on this project's machine, resolved by its daemon.
          this.sendDaemonCommand({
            type: 'run_swarm',
            project_root: this.projectRoot,
            swarm_name: result.def.name,
            vars: {},
          });
        } else {
          void runLocalSwarm(this.projectRoot, result.def.name);
        }
        return;
      case 'createWorktree':
        this.createWorktree(result.name);
        return;
      case 'openFilePane':
        return; // Only offered by pane-targeted palettes, which handle it themselves.
    }
  }

  /** Eagerly remove an agent from the local model, then kill it. Prevents a sidebar flash. */
  removeAndKillAgent(agentId: string): void {
    this.killedAgentIds.add(agentId);
    const s = this.store.getState();
    this.store.setState({
      rootAgents: s.rootAgents.filter((agent) => agent.id !== agentId),
      worktrees: s.worktrees.map((worktree) =>
        worktree.agents.some((agent) => agent.id === agentId)
          ? { ...worktree, agents: worktree.agents.filter((agent) => agent.id !== agentId) }
          : worktree,
      ),
    });
    this.reconcile();
    this.killAgent(agentId);
  }

  killAgent(agentId: string): void {
    this.killedAgentIds.add(agentId);
    // The agent stays hidden while the request is pending; if the daemon refuses, it is still
    // alive and must come back.
    this.sendDaemonRequest(
      { type: 'kill', project_root: this.projectRoot, target: { agent: agentId } },
      () => {},
      () => {
        this.killedAgentIds.delete(agentId);
        this.refresh();
      },
    );
  }

  renameAgent(agentId: string, name: string): void {
    this.sendDaemonCommand({
      type: 'rename',
      project_root: this.projectRoot,
      agent_id: agentId,
      name,
    });
  }

  killAllAgents(): void {
    this.sendDaemonCommand({ type: 'kill', project_root: this.projectRoot, target: 'all' });
  }

  deleteWorktree(worktreeId: string): void {
    this.sendDaemonCommand({
      type: 'delete_worktree',
      project_root: this.projectRoot,
      worktree_id: worktreeId,
    });
  }

  killWorktreeAgents(worktreeId: string): void {
    this.sendDaemonCommand({
      type: 'kill',
      project_root: this.projectRoot,
      target: { worktree: worktreeId },
    });
  }

  // MARK: Resume

  private resumeSuspendedAgents(): void {
    for (const agent of this.allAgents) {
      if (!agent.suspended) continue;
      const name = agentDisplayName(agent);
      void this.client
        .send({ type: 'resume', project_root: this.projectRoot, agent_id: agent.id })
        .then(
          (response) => {
            if (response.type === 'error') {
              this.owner?.setDaemonError(`Resume failed for ${name}: ${response.message}`);
            }
          },
          () => {},
        );
    }
  }

  /**
   * The one place manifest data enters this project's state. Reconciling on the same edge that
   * delivers the agents keeps panes and rows in step.
   */
  private apply(snapshot: WorkspaceSnapshot): void {
    let filteredRoot: readonly AgentModel[] = snapshot.rootAgents;
    let filteredWorktrees: readonly WorktreeModel[] = snapshot.worktrees;
    if (this.killedAgentIds.size > 0) {
      // A kill the manifest has caught up on can stop being suppressed.
      const stillPresent = new Set(
        [...snapshot.rootAgents, ...snapshot.worktrees.flatMap((w) => w.agents)].map((a) => a.id),
      );
      for (const id of [...this.killedAgentIds]) {
        if (!stillPresent.has(id)) this.killedAgentIds.delete(id);
      }
      filteredRoot = snapshot.rootAgents.filter((agent) => !this.killedAgentIds.has(agent.id));
      filteredWorktrees = snapshot.worktrees.map((worktree) => ({
        ...worktree,
        agents: worktree.agents.filter((agent) => !this.killedAgentIds.has(agent.id)),
      }));
    }
    this.merge(filteredRoot, filteredWorktrees);
    this.reconcile();
  }

  /** Replace only what changed, so unchanged lists keep their identity for subscribers. */
  private merge(rootAgents: readonly AgentModel[], worktrees: readonly WorktreeModel[]): void {
    const s = this.store.getState();
    const next: { -readonly [K in keyof ProjectStateFields]?: ProjectStateFields[K] } = {};
    if (!structurallyEqual(s.worktrees, worktrees)) next.worktrees = worktrees;
    if (!structurallyEqual(s.rootAgents, rootAgents)) next.rootAgents = rootAgents;
    if (Object.keys(next).length > 0) this.store.setState(next);
  }

  private reconcile(): void {
    // A local layout that has not been read yet would surface as a restore error; the snapshot
    // reconciles once it has.
    if (this.layout !== null && !this.layout.isOpen(this.projectId)) return;
    this.registry?.reconcile(this.projectId, this.rootAgents, this.worktrees);
  }

  // MARK: Daemon requests

  private send(request: Request): Promise<Response> {
    return this.env.sendReleasingWatchers(request, (r) => this.client.send(r));
  }

  /** Fire-and-forget daemon command with standard error handling. */
  private sendDaemonCommand(request: Request): void {
    this.sendDaemonRequest(request, () => {});
  }

  /** Daemon request with response routing and standard error handling. */
  private sendDaemonRequest(
    request: Request,
    onSuccess: (response: Response) => void,
    onFailure?: () => void,
  ): void {
    void this.send(request).then(
      (response) => {
        if (response.type === 'error') {
          this.owner?.setDaemonError(response.message);
          onFailure?.();
        } else {
          onSuccess(response);
        }
      },
      (error: unknown) => {
        this.owner?.setDaemonError(errorMessage(error));
        onFailure?.();
      },
    );
  }
}
