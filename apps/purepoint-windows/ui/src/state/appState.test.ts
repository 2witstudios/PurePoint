// AppState (WIN-6.0): project management, following the daemon's project list, active project
// routing, pending selections, event dispatch, machines and the sidebar projections.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const listeners = new Map<string, (event: { payload: unknown }) => void>();
const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, handler: (event: { payload: unknown }) => void) => {
    listeners.set(event, handler);
    return Promise.resolve(() => {
      listeners.delete(event);
    });
  },
}));

const { AppState, isTooBroadProjectRoot } = await import('./appState');
const { WorkspaceRegistry } = await import('./workspaceRegistry');
const { ManualScheduler, MemoryWorkspacePersistence } =
  await import('./workspaceRegistry.fixtures');
const { makeAgentModel } = await import('../model/workspaceModel');
type Request = import('../protocol').Request;
type Response = import('../protocol').Response;
type ProjectState = import('./projectState').ProjectState;
type MachineDirectory = import('./appState').MachineDirectory;
type MachineState = import('./appState').MachineState;
type Machine = import('./appState').Machine;

const tick = () =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });

function memoryStorage() {
  const map = new Map<string, string>();
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
    removeItem: (key: string) => {
      map.delete(key);
    },
  };
}

const agent = (id: string) =>
  makeAgentModel({
    id,
    name: id,
    agentType: 'claude',
    status: 'running',
    prompt: '',
    startedAt: '',
  });

function setup(answer: (request: Request) => Response = () => ({ type: 'ok' })) {
  const registry = new WorkspaceRegistry({
    persistence: new MemoryWorkspacePersistence(),
    scheduler: new ManualScheduler(),
  });
  const requests: Request[] = [];
  const started: string[] = [];
  const storage = memoryStorage();
  const app = new AppState({
    registry,
    client: {
      send: (request) => {
        requests.push(request);
        return Promise.resolve(answer(request));
      },
    },
    startProject: (project) => started.push(project.projectRoot),
    followsLocalDaemon: false,
    storage,
    homeDirectory: 'C:\\Users\\me',
  });
  return { app, registry, requests, started, storage };
}

beforeEach(() => {
  listeners.clear();
  invoke.mockReset();
  invoke.mockResolvedValue(true);
});

describe('AppState project management', () => {
  it('opens a project once, starts it and makes it active if none is', () => {
    const { app, started } = setup();
    expect(app.isLoaded).toBe(false);
    expect(app.activeSidebarSelection).toEqual({ kind: 'nav', item: 'dashboard' });
    app.openProject('C:\\a');
    app.openProject('C:\\a');
    app.openProject('C:\\b');
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\a', 'C:\\b']);
    expect(started).toEqual(['C:\\a', 'C:\\b']);
    expect(app.activeProjectRoot).toBe('C:\\a');
    expect(app.projects[0]?.owner).toBe(app);
  });

  it('closes a project: forgets it and moves the active project on', () => {
    const { app, registry } = setup();
    app.openProject('C:\\a');
    app.openProject('C:\\b');
    registry.reconcile('C:\\a', [agent('ag-1')], []);
    expect(app.closeProject('C:\\a')).toBe(true);
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\b']);
    expect(registry.workspacesByProject['C:\\a']).toBeUndefined();
    expect(app.activeProjectRoot).toBe('C:\\b');
    expect(app.closeProject('C:\\missing')).toBe(false);
  });

  it('keeps a Point Guard folder followed after it leaves the sidebar', () => {
    const { app } = setup();
    app.updatePointGuardRoot('C:\\pg\\.\\sub\\..');
    expect(app.pointGuardRoot).toBe('C:\\pg');
    app.openProject('C:\\pg');
    expect(app.isPointGuardFolder('C:\\pg')).toBe(true);
    expect(app.closeProject('C:\\pg')).toBe(false);
    expect(app.projectState('C:\\pg')).not.toBeNull();
    app.updatePointGuardRoot('');
    expect(app.pointGuardRoot).toBeNull();
  });

  it('removes a project through the daemon', async () => {
    const { app, requests } = setup();
    app.openProject('C:\\a');
    const project = app.projects[0] as ProjectState;
    await app.removeProject(project);
    expect(requests).toEqual([
      { type: 'remove_project', project_root: 'C:\\a', suspend_agents: true },
    ]);
    expect(app.projects).toEqual([]);
  });

  it('refuses roots that are too broad', () => {
    expect(isTooBroadProjectRoot('C:\\', null)).toBe(true);
    expect(isTooBroadProjectRoot('c:', null)).toBe(true);
    expect(isTooBroadProjectRoot('/', null)).toBe(true);
    expect(isTooBroadProjectRoot('C:\\Users', 'C:\\Users\\me')).toBe(true);
    expect(isTooBroadProjectRoot('c:/users/ME/', 'C:\\Users\\me')).toBe(true);
    expect(isTooBroadProjectRoot('C:\\Users\\me\\code', 'C:\\Users\\me')).toBe(false);
    expect(isTooBroadProjectRoot('C:\\Users\\meet', 'C:\\Users\\me')).toBe(false);
  });

  it('moves a project and saves the order, reverting when the save fails', async () => {
    const { app, requests } = setup(() => ({ type: 'error', code: 'X', message: 'no' }));
    for (const root of ['C:\\a', 'C:\\b', 'C:\\c']) app.openProject(root);
    app.applyLocalProjects(['C:\\a', 'C:\\b', 'C:\\c']);
    app.moveProject(app.projects[2] as ProjectState, 0);
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\c', 'C:\\a', 'C:\\b']);
    await tick();
    await tick();
    expect(requests[0]).toEqual({
      type: 'reorder_projects',
      project_roots: ['C:\\c', 'C:\\a', 'C:\\b'],
    });
    expect(app.daemonError).toBe('Could not save the project order.');
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\a', 'C:\\b', 'C:\\c']);
  });
});

describe('AppState following the daemon project list', () => {
  it('opens joined projects, closes removed ones and takes the daemon order', () => {
    const { app } = setup();
    app.applyLocalProjects(['C:\\a', 'C:\\b']);
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\a', 'C:\\b']);
    app.applyLocalProjects(['C:\\c', 'C:\\b']);
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\c', 'C:\\b']);
    app.applyLocalProjects(['C:\\b', 'C:\\c', 'C:\\']);
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\b', 'C:\\c']);
  });

  it('resumes agents only for the first list after a session restore', () => {
    const { app } = setup();
    app.restoreProjects();
    app.applyLocalProjects(['C:\\a']);
    app.applyLocalProjects(['C:\\a', 'C:\\b']);
    expect(app.projects[0]?.keepsSuspendedAgentsOnNextOpen).toBe(false);
    expect(app.projects[1]?.keepsSuspendedAgentsOnNextOpen).toBe(true);
  });

  it('imports the legacy open-projects list once', () => {
    const { app, storage } = setup();
    storage.setItem('PurePointOpenProjects', JSON.stringify(['C:\\legacy']));
    app.restoreProjects();
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\legacy']);
    expect(storage.getItem('PurePointOpenProjects')).toBeNull();
  });

  it('follows the daemon list through the projects subscription and its event', async () => {
    const registry = new WorkspaceRegistry({
      persistence: new MemoryWorkspacePersistence(),
      scheduler: new ManualScheduler(),
    });
    const app = new AppState({
      registry,
      client: { send: () => Promise.resolve({ type: 'ok' }) },
      startProject: () => {},
      storage: memoryStorage(),
    });
    const stop = await app.connect();
    app.restoreProjects();
    expect(invoke).toHaveBeenCalledWith('subscriptions_start_projects', undefined);
    listeners.get('projects-changed')?.({ payload: { projects: ['C:\\x'] } });
    expect(app.projects.map((p) => p.projectRoot)).toEqual(['C:\\x']);
    stop();
    expect(listeners.size).toBe(0);
  });
});

describe('AppState routing and selection', () => {
  it('routes sidebar selections to their project', () => {
    const { app, registry } = setup();
    app.openProject('C:\\a');
    app.openProject('C:\\b');
    registry.reconcile('C:\\b', [agent('ag-1')], []);
    app.updateActiveProject({ kind: 'workspace', id: 'ws-ag-1' });
    expect(app.activeProjectRoot).toBe('C:\\b');
    app.updateActiveProject({ kind: 'nav', item: 'agents' });
    expect(app.activeProjectRoot).toBe('C:\\b');
    expect(app.activeSidebarSelection).toEqual({ kind: 'nav', item: 'agents' });
    app.updateActiveProject({ kind: 'project', id: 'C:\\a' });
    expect(app.activeProjectRoot).toBe('C:\\a');
  });

  it('resolves a pending workspace once the registry has it, and drops one that never appears', () => {
    const { app, registry, storage } = setup();
    storage.setItem(
      'purepoint.workspaceSelection.v1',
      JSON.stringify({ activeWorkspaceId: 'ws-ag-1' }),
    );
    expect(app.restoreActiveWorkspace()).toBe('ws-ag-1');
    app.openProject('C:\\a');
    app.openProject('C:\\b');
    expect(app.resolvePendingWorkspaceSelection()).toBeNull();
    expect(app.pendingSelectWorkspaceId).toBe('ws-ag-1');
    registry.reconcile('C:\\b', [agent('ag-1')], []);
    expect(app.resolvePendingWorkspaceSelection()).toBe('ws-ag-1');
    expect(app.pendingSelectWorkspaceId).toBeNull();
    expect(app.activeProjectRoot).toBe('C:\\b');

    app.pendingSelectWorkspaceId = 'ws-gone';
    registry.reconcile('C:\\a', [], []);
    expect(app.resolvePendingWorkspaceSelection()).toBeNull();
    expect(app.pendingSelectWorkspaceId).toBeNull();
  });

  it('answers cross-project queries', () => {
    const { app } = setup();
    app.openProject('C:\\a');
    const project = app.projects[0] as ProjectState;
    project.applyStatusEvent(
      [
        {
          id: 'wt-1',
          name: 'wt',
          path: 'C:\\a\\.pu\\worktrees\\wt-1',
          branch: 'b',
          baseBranch: null,
          status: 'active',
          createdAt: '',
          agents: {
            w: {
              id: 'w',
              name: 'w',
              agentType: 'claude',
              status: 'running',
              prompt: null,
              startedAt: '',
              suspended: false,
              sessionId: 'sess-1',
            },
          },
        },
      ],
      [],
    );
    return tick().then(() => {
      expect(app.agent('w')?.id).toBe('w');
      expect(app.projectStateForAgent('w')).toBe(project);
      expect(app.projectStateForWorktree('wt-1')).toBe(project);
      expect(app.agentIdForSession('sess-1')).toBe('w');
      expect(app.worktreeIdForPath('c:/a/.pu/worktrees/wt-1/')).toBe('wt-1');
      expect(app.client('w')).toBe(project.client);
      expect(app.projectSnapshots()[0]).toMatchObject({
        projectId: 'C:\\a',
        hasLoadedWorktrees: false,
      });
      expect(app.store.getState().projectRevision).toBeGreaterThan(0);
    });
  });

  it('roots palette definitions at the pane project first', () => {
    const { app, registry } = setup();
    app.openProject('C:\\a');
    app.openProject('C:\\b');
    registry.reconcile('C:\\b', [agent('ag-1')], []);
    expect(app.paletteProjectRoots('ws-ag-1')).toEqual(['C:\\b', 'C:\\a']);
    expect(app.paletteProjectRoots('ws-none')).toEqual(['C:\\a', 'C:\\b']);
    expect(app.fileRoot('ws-ag-1')).toBe('C:\\b');
  });
});

describe('AppState backend events', () => {
  it('forwards status, manifest, subscription, grid and daemon events', async () => {
    const { app, registry } = setup();
    app.openProject('C:\\a');
    const project = app.projects[0] as ProjectState;
    const applied = vi.spyOn(project, 'applyStatusEvent');
    const manifest = vi.spyOn(project, 'handleManifestChanged').mockImplementation(() => {});
    const lost = vi.spyOn(project, 'handleSubscriptionState').mockImplementation(() => {});
    const grid = vi.spyOn(registry, 'handleRemoteCommand');
    await app.connect();
    listeners.get('status-event')?.({
      payload: { projectRoot: 'C:\\a', worktrees: [], agents: [] },
    });
    listeners.get('status-event')?.({
      payload: { projectRoot: 'C:\\z', worktrees: [], agents: [] },
    });
    listeners.get('manifest-changed')?.({ payload: { projectRoot: 'C:\\a' } });
    listeners.get('subscription-state')?.({
      payload: { kind: 'status', projectRoot: 'C:\\a', state: 'daemon_lost' },
    });
    listeners.get('grid-event')?.({
      payload: { projectRoot: 'C:\\a', command: { action: 'split' } },
    });
    listeners.get('daemon-state')?.({
      payload: { state: 'failed', kind: 'io', message: 'pipe broke' },
    });
    expect(applied).toHaveBeenCalledTimes(1);
    expect(manifest).toHaveBeenCalledTimes(1);
    expect(lost).toHaveBeenCalledTimes(1);
    expect(grid).toHaveBeenCalledWith({ action: 'split' }, 'C:\\a');
    expect(app.daemonError).toBe('pipe broke');
    expect(app.store.getState().daemonConnection.state).toBe('failed');
  });

  it('applies a grid event to the layout of the project it names (WIN-6.4)', async () => {
    const { app, registry } = setup();
    app.openProject('C:\\a');
    registry.reconcile('C:\\a', [agent('ag-1')], []);
    registry.activeWorkspaceId = 'ws-ag-1';
    await app.connect();

    listeners.get('grid-event')?.({
      payload: { projectRoot: 'C:\\b', command: { action: 'split', axis: 'v' } },
    });
    expect(Object.keys(registry.workspace('ws-ag-1')?.panes ?? {})).toHaveLength(1);

    listeners.get('grid-event')?.({
      payload: { projectRoot: 'C:\\a', command: { action: 'split', axis: 'v' } },
    });
    expect(Object.keys(registry.workspace('ws-ag-1')?.panes ?? {})).toHaveLength(2);
  });

  it('clears the daemon-failure banner once the daemon connects, but not other errors', () => {
    const { app } = setup();
    const failed = {
      state: 'failed',
      kind: 'not_running',
      message: 'Daemon is not running (x)',
    } as const;
    app.handleDaemonState(failed);
    expect(app.daemonError).toBe('Daemon is not running (x)');
    app.handleDaemonState({ state: 'starting' });
    expect(app.daemonError).toBe('Daemon is not running (x)');
    app.handleDaemonState({ state: 'connected', pid: 1, launched: true });
    expect(app.daemonError).toBeNull();

    app.handleDaemonState(failed);
    app.setDaemonError('Could not save the project order.');
    app.handleDaemonState({ state: 'connected', pid: 1, launched: true });
    expect(app.daemonError).toBe('Could not save the project order.');
  });

  it('suspends and shuts down on quit only when this app launched the daemon', async () => {
    const { app, requests } = setup();
    app.openProject('C:\\a');
    await app.shutdownWithSuspend();
    expect(requests).toEqual([]);
    app.handleDaemonState({ state: 'connected', pid: 1, launched: true });
    await app.shutdownWithSuspend();
    expect(requests).toEqual([
      { type: 'suspend', project_root: 'C:\\a', target: 'all' },
      { type: 'shutdown' },
    ]);
  });
});

describe('AppState machines', () => {
  function fakeDirectory(machines: Machine[]) {
    const states: { connected: number; disconnected: number; machine: Machine }[] = [];
    const directory: MachineDirectory = {
      displayPath: '%USERPROFILE%\\.pu\\machines.json',
      load: () => Promise.resolve([...machines]),
      add: (name, sshHost, id) => Promise.resolve({ id: id ?? 'new', name, sshHost }),
      remove: () => Promise.resolve(),
      isUserDisconnected: (id) => id === 'off',
      makeState: (machine) => {
        const record = { connected: 0, disconnected: 0, machine };
        states.push(record);
        const state: MachineState = {
          get machine() {
            return record.machine;
          },
          projects: [],
          snapshot: () => ({
            id: record.machine.id,
            name: record.machine.name,
            sshHost: record.machine.sshHost,
            state: { kind: 'disconnected' },
            statusText: '',
            actionError: null,
            isStartingDaemon: false,
            hiddenProjectCount: 0,
            hiddenCountRevision: 0,
            hasLoadedProjects: false,
            projects: [],
          }),
          update: (from) => {
            record.machine = from;
          },
          connect: () => {
            record.connected += 1;
          },
          disconnect: () => {
            record.disconnected += 1;
          },
          hideProject: () => {},
          removeProject: () => Promise.resolve(),
          moveProject: () => {},
          client: { send: () => Promise.reject(new Error('no daemon')) },
          startDaemon: () => {},
          showHiddenProjects: () => {},
          dismissError: () => {},
          openProject: () => Promise.resolve(),
        };
        return state;
      },
    };
    return { directory, states };
  }

  it('connects registered machines, updates changed ones and drops removed ones', async () => {
    const machines: Machine[] = [
      { id: 'm1', name: 'Box', sshHost: 'box' },
      { id: 'off', name: 'Off', sshHost: 'off' },
    ];
    const { directory, states } = fakeDirectory(machines);
    const app = new AppState({
      machines: directory,
      storage: memoryStorage(),
      client: { send: () => Promise.resolve({ type: 'ok' }) },
    });
    await app.loadMachines();
    expect(app.machines.map((m) => m.machine.id)).toEqual(['m1', 'off']);
    expect(states.map((s) => s.connected)).toEqual([1, 0]);
    machines.splice(0, 2, { id: 'm1', name: 'Renamed', sshHost: 'box' });
    await app.reloadMachines();
    expect(app.machines.map((m) => m.machine.name)).toEqual(['Renamed']);
    expect(states[1]?.disconnected).toBe(1);
    expect(app.machineSnapshots()[0]?.name).toBe('Renamed');
  });

  it('reports a registry that cannot be read', async () => {
    const { directory } = fakeDirectory([]);
    const app = new AppState({
      machines: { ...directory, load: () => Promise.reject(new Error('bad json')) },
      storage: memoryStorage(),
      client: { send: () => Promise.resolve({ type: 'ok' }) },
    });
    await app.reloadMachines();
    expect(app.daemonError).toBe(
      'Could not read machines from %USERPROFILE%\\.pu\\machines.json: bad json',
    );
  });

  it("routes a machine's status events to its project, not a local one with the same root", async () => {
    const { ProjectState: Project } = await import('./projectState');
    const remote = new Project({
      projectRoot: '/srv/app',
      machineId: 'm1',
      client: { send: () => Promise.resolve({ type: 'ok' }) },
    });
    const local = new Project({
      projectRoot: '/srv/app',
      client: { send: () => Promise.resolve({ type: 'ok' }) },
    });
    const { directory } = fakeDirectory([{ id: 'm1', name: 'Box', sshHost: 'box' }]);
    const app = new AppState({
      machines: {
        ...directory,
        makeState: (machine, owner) => ({
          ...directory.makeState(machine, owner),
          projects: [remote],
        }),
      },
      storage: memoryStorage(),
      client: { send: () => Promise.resolve({ type: 'ok' }) },
      startProject: () => undefined,
      followsLocalDaemon: false,
    });
    app.store.setState({ projects: [local] });
    await app.loadMachines();
    const remoteApply = vi.spyOn(remote, 'applyStatusEvent');
    const localApply = vi.spyOn(local, 'applyStatusEvent');
    app.handleStatusEvent({ projectRoot: '/srv/app', machineId: 'm1', worktrees: [], agents: [] });
    expect(remoteApply).toHaveBeenCalledOnce();
    expect(localApply).not.toHaveBeenCalled();
    app.handleStatusEvent({ projectRoot: '/srv/app', worktrees: [], agents: [] });
    expect(localApply).toHaveBeenCalledOnce();
    expect(app.projectState('machine:m1:/srv/app')).toBe(remote);
  });
});

describe('AppState agent signals (WIN-7.7)', () => {
  const NOW = 1792195201 * 1000;
  const signal = {
    schema_version: 1,
    identity: {
      agent_id: 'ag-1',
      worktree_id: null,
      launch_id: 'launch-1',
      generation: 1,
      native_session_id: 'native-1',
    },
    activity: 'working',
    pending_attention: [
      {
        request_id: 'request-1',
        kind: 'permission',
        event_id: 'event-1',
        turn_id: null,
        confidence: 'authoritative',
        actionability: { macos: false, ios: false },
      },
    ],
    outcome: null,
    observation: {
      state: 'current',
      last_received_at_ms: NOW - 1000,
      expires_at_ms: NOW + 30_000,
    },
    evidence: null,
  } as const;

  beforeEach(() => {
    vi.useFakeTimers({ now: NOW, toFake: ['Date'] });
    return () => {
      vi.useRealTimers();
    };
  });

  async function signalSetup() {
    const { AgentSignalNotifications } = await import('../services/agentSignalNotifications');
    const registry = new WorkspaceRegistry({
      persistence: new MemoryWorkspacePersistence(),
      scheduler: new ManualScheduler(),
    });
    const toasts: string[] = [];
    const badges: number[] = [];
    const notifications = new AgentSignalNotifications({
      storage: memoryStorage(),
      postToast: (toast) => {
        toasts.push(toast.body);
        return Promise.resolve();
      },
      setBadge: (count) => {
        badges.push(count);
        return Promise.resolve();
      },
    });
    const app = new AppState({
      registry,
      client: { send: () => Promise.resolve({ type: 'ok' }) },
      startProject: () => undefined,
      followsLocalDaemon: false,
      storage: memoryStorage(),
      signalNotifications: notifications,
    });
    app.openProject('C:\\a');
    const project = app.projects[0] as ProjectState;
    project.applyStatusEvent(
      [],
      [
        {
          id: 'ag-1',
          name: 'one',
          agent_type: 'claude',
          status: 'running',
          pid: 1,
          exit_code: null,
          idle_seconds: 0,
          worktree_id: null,
          started_at: '',
          session_id: 'native-1',
          suspended: false,
          signal: structuredClone(signal),
        } as unknown as import('../protocol').AgentStatusReport,
      ],
    );
    await tick();
    return { app, registry, toasts, badges };
  }

  it('routes a signal to the pane that holds its agent', async () => {
    const { app, registry } = await signalSetup();
    const workspaceId = registry.workspaceIdForAgent('ag-1') as string;
    expect(workspaceId).not.toBeNull();
    const { agents, foreground } = app.signalAlertAgents(NOW);
    expect(foreground).toBeNull();
    expect(agents).toHaveLength(1);
    expect(agents[0]?.presentation.label).toBe('Permission');
    expect(agents[0]?.destination).toMatchObject({
      projectId: 'C:\\a',
      agentId: 'ag-1',
      eventId: 'event-1',
    });
  });

  it('badges, toasts while unfocused, and routes on refocus', async () => {
    const { app, registry, toasts, badges } = await signalSetup();
    app.setWindowFocused(false);
    app.updateAgentSignals();
    expect(toasts).toEqual(['An agent needs attention.']);
    await tick();
    expect(badges).toEqual([1]);
    app.setWindowFocused(true);
    await tick();
    expect(app.store.getState().pendingSignalDestination).toMatchObject({ agentId: 'ag-1' });
    const target = app.takeSignalDestination(NOW);
    expect(target?.workspaceId).toBe(registry.workspaceIdForAgent('ag-1'));
    expect(app.activeProjectRoot).toBe('C:\\a');
    expect(app.store.getState().pendingSignalDestination).toBeNull();
  });

  it('treats the focused, selected agent as on screen and refuses an expired route', async () => {
    const { app, registry } = await signalSetup();
    const workspaceId = registry.workspaceIdForAgent('ag-1') as string;
    app.updateActiveProject({ kind: 'workspace', id: workspaceId });
    registry.activate(workspaceId);
    expect(app.signalAlertAgents(NOW).foreground).not.toBeNull();
    const agent = app.signalAlertAgents(NOW).agents[0];
    expect(agent !== undefined && app.openSignal(agent, NOW + 60_000)).toBe(false);
  });
});
