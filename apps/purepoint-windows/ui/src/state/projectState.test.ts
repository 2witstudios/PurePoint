// ProjectState (WIN-6.0): open sequence, status-event application, kill suppression, spawns and
// the remote layout restore, with the daemon and backend events stood in for.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const { ProjectState } = await import('./projectState');
const { WorkspaceRegistry } = await import('./workspaceRegistry');
const { ManualScheduler, MemoryWorkspacePersistence } =
  await import('./workspaceRegistry.fixtures');
const { CachedWorkspacePersistence } = await import('./cachedWorkspacePersistence');
const { RemoteLayoutSaver } = await import('./remoteLayoutSaver');
type Request = import('../protocol').Request;
type Response = import('../protocol').Response;
type AgentStatusReport = import('../protocol').AgentStatusReport;
type WorktreeEntry = import('../protocol').WorktreeEntry;
type ProjectEnvironment = import('./projectState').ProjectEnvironment;
type ProjectStateOwner = import('./projectState').ProjectStateOwner;
type LayoutHostIO = import('../model/workspacePersistence').LayoutHostIO;

const root = 'C:\\work\\app';

const tick = () =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });

async function until(condition: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !condition(); i += 1) await tick();
  expect(condition()).toBe(true);
}

function report(id: string, extra: Partial<AgentStatusReport> = {}): AgentStatusReport {
  return {
    id,
    name: id,
    agent_type: 'claude',
    status: 'running',
    pid: null,
    exit_code: null,
    idle_seconds: null,
    worktree_id: null,
    started_at: '',
    suspended: false,
    ...extra,
  };
}

function worktreeEntry(id: string, agentIds: string[]): WorktreeEntry {
  return {
    id,
    name: id,
    path: `${root}\\.pu\\worktrees\\${id}`,
    branch: `pu/${id}`,
    baseBranch: null,
    status: 'active',
    createdAt: '',
    agents: Object.fromEntries(
      agentIds.map((agentId) => [
        agentId,
        {
          id: agentId,
          name: agentId,
          agentType: 'claude',
          status: 'running' as const,
          prompt: null,
          startedAt: '',
          suspended: false,
        },
      ]),
    ),
  };
}

/** A daemon answering from a script; every request is recorded. */
function daemon(answer: (request: Request) => Response | Promise<Response>) {
  const requests: Request[] = [];
  return {
    requests,
    client: {
      send: (request: Request) => {
        requests.push(request);
        return Promise.resolve(answer(request));
      },
    },
  };
}

function status(agents: AgentStatusReport[] = [], worktrees: WorktreeEntry[] = []): Response {
  return { type: 'status_report', worktrees, agents };
}

function owner(): ProjectStateOwner & {
  errors: (string | null)[];
  active: string | null;
  pendingAgent: string | null;
  pendingWorktree: string | null;
  project: unknown;
} {
  const o = {
    errors: [] as (string | null)[],
    active: null as string | null,
    pendingAgent: null as string | null,
    pendingWorktree: null as string | null,
    project: null as unknown,
    setDaemonError(message: string | null) {
      o.errors.push(message);
    },
    projectState() {
      return o.project as never;
    },
    setActiveProjectRoot(r: string | null) {
      o.active = r;
    },
    setPendingSelectAgentId(id: string | null) {
      o.pendingAgent = id;
    },
    setPendingSelectWorktreeId(id: string | null) {
      o.pendingWorktree = id;
    },
  };
  return o;
}

function environment(overrides: Partial<ProjectEnvironment> = {}) {
  const calls: string[] = [];
  let now = 0;
  const env: ProjectEnvironment = {
    startSubscriptions: (r) => {
      calls.push(`subscribe ${r}`);
      return Promise.resolve();
    },
    stopSubscriptions: (r) => {
      calls.push(`unsubscribe ${r}`);
      return Promise.resolve(true);
    },
    watchManifest: (r) => {
      calls.push(`watch ${r}`);
      return Promise.resolve(7);
    },
    unwatch: (id) => {
      calls.push(`unwatch ${String(id)}`);
      return Promise.resolve(true);
    },
    sendReleasingWatchers: (request, send) => {
      calls.push(`send ${request.type}`);
      return send(request);
    },
    sleep: (ms) => {
      now += ms;
      return Promise.resolve();
    },
    now: () => now,
    log: () => {},
    ...overrides,
  };
  return { env, calls };
}

function memoryHost(): LayoutHostIO {
  return {
    stat: () => Promise.resolve(null),
    read: () => Promise.resolve(null),
    write: () => Promise.resolve(1),
  };
}

function setup(answer: (request: Request) => Response | Promise<Response>) {
  const { client, requests } = daemon(answer);
  const layout = new CachedWorkspacePersistence({ host: memoryHost() });
  const registry = new WorkspaceRegistry({ persistence: layout, scheduler: new ManualScheduler() });
  const { env, calls } = environment();
  const project = new ProjectState({
    projectRoot: root,
    client,
    registry,
    layout,
    environment: env,
  });
  const o = owner();
  o.project = project;
  project.owner = o;
  return { project, registry, layout, requests, calls, owner: o };
}

describe('ProjectState', () => {
  it('inits, watches, subscribes, applies the first snapshot and resumes suspended agents', async () => {
    const { project, registry, requests, calls } = setup((request) => {
      if (request.type === 'init') return { type: 'init_result', created: false };
      if (request.type === 'status') return status([report('ag-1', { suspended: true })]);
      return { type: 'resume_result', agent_id: 'ag-1', status: 'running' };
    });
    project.startWatching();
    await until(() => requests.some((r) => r.type === 'resume'));
    // The channel's background poll (WIN-8.10) reads alongside the workspace requests.
    expect(requests.some((r) => r.type === 'channel_read')).toBe(true);
    expect(requests.map((r) => r.type).filter((type) => type !== 'channel_read')).toEqual([
      'init',
      'status',
      'resume',
    ]);
    expect(calls).toEqual([`watch ${root}`, `subscribe ${root}`]);
    expect(project.hasLoadedWorktrees).toBe(true);
    expect(project.rootAgents.map((a) => a.id)).toEqual(['ag-1']);
    expect(registry.workspaces(root).map((w) => w.id)).toEqual(['ws-ag-1']);
    expect(project.snapshot()).toMatchObject({
      projectId: root,
      channelUnread: 0,
      layoutError: null,
    });
  });

  it('keeps suspended agents benched when asked, once', async () => {
    const { project, requests } = setup((request) =>
      request.type === 'status'
        ? status([report('ag-1', { suspended: true })])
        : { type: 'init_result', created: false },
    );
    project.keepsSuspendedAgentsOnNextOpen = true;
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    await tick();
    expect(requests.some((r) => r.type === 'resume')).toBe(false);
    expect(project.keepsSuspendedAgentsOnNextOpen).toBe(false);
  });

  it('retries a busy daemon with backoff and surfaces a refusal', async () => {
    let attempts = 0;
    const busy = setup((request) => {
      if (request.type !== 'init') return status();
      attempts += 1;
      return attempts < 3
        ? { type: 'error', code: 'BUSY', message: 'busy' }
        : { type: 'init_result', created: true };
    });
    busy.project.startWatching();
    await until(() => busy.project.hasLoadedWorktrees);
    expect(attempts).toBe(3);

    const refused = setup(() => ({ type: 'error', code: 'BAD_ROOT', message: 'no such root' }));
    refused.project.startWatching();
    await until(() => refused.owner.errors.length > 0);
    expect(refused.owner.errors).toEqual(['no such root']);
  });

  it('gives up after the connect budget', async () => {
    const { project, owner: o } = setup(() => Promise.reject(new Error('pipe closed')));
    project.startWatching();
    await until(() => o.errors.length > 0);
    expect(o.errors).toEqual(['pipe closed']);
  });

  it('applies status events and drops a killed agent until the manifest catches up', async () => {
    const { project, registry } = setup((request) =>
      request.type === 'status'
        ? status()
        : request.type === 'kill'
          ? { type: 'kill_result', killed: ['ag-2'], exit_codes: {}, skipped: [] }
          : { type: 'init_result', created: false },
    );
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    project.applyStatusEvent([worktreeEntry('wt-1', ['ag-2'])], [report('ag-1')]);
    await until(() => project.worktrees.length === 1);
    expect(project.allAgents.map((a) => a.id)).toEqual(['ag-2', 'ag-1']);
    expect(project.worktreeId('ag-2')).toBe('wt-1');

    project.removeAndKillAgent('ag-2');
    expect(project.agent('ag-2')).toBeNull();
    // A status push from before the kill landed must not bring it back.
    project.applyStatusEvent([worktreeEntry('wt-1', ['ag-2'])], [report('ag-1')]);
    await tick();
    expect(project.agent('ag-2')).toBeNull();
    expect(registry.workspaceIdForAgent('ag-2')).toBeNull();
    // Once the manifest drops it, a new agent with that id would show again.
    project.applyStatusEvent([worktreeEntry('wt-1', [])], [report('ag-1')]);
    await tick();
    project.applyStatusEvent([worktreeEntry('wt-1', ['ag-2'])], [report('ag-1')]);
    await tick();
    expect(project.agent('ag-2')).not.toBeNull();
  });

  it('brings a killed agent back when the daemon refuses the kill', async () => {
    const { project, owner: o } = setup((request) => {
      if (request.type === 'kill') return { type: 'error', code: 'NOPE', message: 'refused' };
      if (request.type === 'status') return status([report('ag-1')]);
      return { type: 'init_result', created: false };
    });
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    project.removeAndKillAgent('ag-1');
    expect(project.agent('ag-1')).toBeNull();
    await until(() => project.agent('ag-1') !== null);
    expect(o.errors).toContain('refused');
  });

  it('restarts the open sequence when the daemon is lost, clearing signals', async () => {
    const signal = {
      identity: {
        agent_id: 'ag-1',
        worktree_id: null,
        launch_id: 'l',
        generation: 1,
        native_session_id: 'n',
      },
    } as never;
    const { project, requests } = setup((request) =>
      request.type === 'status'
        ? status([report('ag-1', { signal })])
        : { type: 'init_result', created: false },
    );
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    expect(project.rootAgents[0]?.signal).not.toBeNull();
    let inits = requests.filter((r) => r.type === 'init').length;
    expect(inits).toBe(1);
    project.handleSubscriptionState({ kind: 'grid', projectRoot: root, state: 'daemon_lost' });
    project.handleSubscriptionState({ kind: 'status', projectRoot: root, state: 'subscribed' });
    expect(requests.filter((r) => r.type === 'init')).toHaveLength(1);
    project.handleSubscriptionState({ kind: 'status', projectRoot: root, state: 'daemon_lost' });
    expect(project.rootAgents[0]?.signal).toBeNull();
    await until(() => requests.filter((r) => r.type === 'init').length === 2);
    inits = requests.filter((r) => r.type === 'init').length;
    expect(inits).toBe(2);
  });

  it('refreshes on a manifest change', async () => {
    let agents = [report('ag-1')];
    const { project } = setup((request) =>
      request.type === 'status' ? status(agents) : { type: 'init_result', created: false },
    );
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    agents = [report('ag-1'), report('ag-2')];
    project.handleManifestChanged();
    await until(() => project.rootAgents.length === 2);
  });

  it('spawns into the selected worktree and selects the new agent', async () => {
    const {
      project,
      requests,
      owner: o,
    } = setup((request) =>
      request.type === 'spawn'
        ? { type: 'spawn_result', worktree_id: 'wt-1', agent_id: 'ag-new', status: 'running' }
        : request.type === 'create_worktree'
          ? { type: 'create_worktree_result', worktree_id: 'wt-9' }
          : { type: 'ok' },
    );
    project.createAgent({
      agent: 'claude',
      prompt: 'go',
      selection: { kind: 'worktree', id: 'wt-1' },
    });
    await until(() => o.pendingAgent !== null);
    expect(requests[0]).toEqual({
      type: 'spawn',
      project_root: root,
      prompt: 'go',
      agent: 'claude',
      name: null,
      root: false,
      worktree: 'wt-1',
      command: null,
    });
    expect(o.pendingAgent).toBe('ag-new');
    expect(o.active).toBe(root);
    project.createWorktree('feature');
    await until(() => o.pendingWorktree === 'wt-9');
  });

  it('routes palette results', async () => {
    const { project, requests } = setup(() => ({ type: 'ok' }));
    const swarms: string[] = [];
    project.handlePaletteResult(
      {
        kind: 'runSwarm',
        def: {
          name: 'full',
          worktree_count: 1,
          worktree_template: '',
          roster: [],
          include_terminal: false,
          scope: 'local',
        },
      },
      null,
      (r, name) => {
        swarms.push(`${r}:${name}`);
        return Promise.resolve();
      },
    );
    expect(swarms).toEqual([`${root}:full`]);
    project.handlePaletteResult({ kind: 'createWorktree', name: null }, null);
    project.handlePaletteResult({ kind: 'openFilePane', path: null }, null);
    await until(() => requests.length === 1);
    expect(requests[0]).toEqual({ type: 'create_worktree', project_root: root, name: null });
  });

  it('sends worktree removals through the watcher release', async () => {
    const { project, calls, requests } = setup(() => ({ type: 'ok' }));
    project.deleteWorktree('wt-1');
    project.killWorktreeAgents('wt-2');
    project.renameAgent('ag-1', 'new');
    project.killAllAgents();
    await until(() => requests.length === 4);
    expect(calls).toEqual(['send delete_worktree', 'send kill', 'send rename', 'send kill']);
    expect(requests[1]).toEqual({ type: 'kill', project_root: root, target: { worktree: 'wt-2' } });
  });

  it('reserves a tab, writes the layout, and binds the spawned agent to it', async () => {
    const { project, registry, layout, requests } = setup((request) => {
      if (request.type === 'spawn') {
        return { type: 'spawn_result', worktree_id: null, agent_id: 'ag-2', status: 'running' };
      }
      if (request.type === 'status') return status([report('ag-1')]);
      return { type: 'init_result', created: false };
    });
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    await layout.open(root);
    const workspace = registry.workspace('ws-ag-1');
    expect(workspace).not.toBeNull();
    const surfaceId = registry.newTab('ws-ag-1', workspace?.focusedLeafId ?? 0);
    expect(surfaceId).not.toBeNull();
    project.spawnAgentForSurface({
      agent: 'claude',
      prompt: '',
      workspaceId: 'ws-ag-1',
      surfaceId: surfaceId ?? 0,
    });
    await until(() => requests.some((r) => r.type === 'spawn'));
    const spawn = requests.find((r) => r.type === 'spawn');
    expect(spawn).toMatchObject({
      root: true,
      worktree: null,
      placement: { workspace: 'ws-ag-1', surface: surfaceId },
    });
    await until(() => registry.workspaceIdForAgent('ag-2') === 'ws-ag-1');
  });

  it('stops watching: unwatches the manifest and stops the subscriptions', async () => {
    const { project, calls } = setup((request) =>
      request.type === 'status' ? status() : { type: 'init_result', created: false },
    );
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    project.stopWatching();
    await until(() => calls.includes('unwatch 7'));
    expect(calls).toContain(`unsubscribe ${root}`);
  });
});

describe('ProjectState remote projects', () => {
  const machineId = '6f1c2b3a-1111-4222-8333-944455556666';

  it('restores the remote layout through the host, retrying a failed read', async () => {
    const registry = new WorkspaceRegistry({
      persistence: new MemoryWorkspacePersistence(),
      scheduler: new ManualScheduler(),
    });
    const projectId = `machine:${machineId}:/srv/app`;
    let reads = 0;
    const host: LayoutHostIO = {
      stat: () => Promise.resolve(null),
      read: () => {
        reads += 1;
        return reads === 1 ? Promise.reject(new Error('ssh dropped')) : Promise.resolve(null);
      },
      write: () => Promise.resolve(5),
    };
    const saver = RemoteLayoutSaver.forHost('/srv/app', host);
    const delays: number[] = [];
    let errorSeen: string | null = null;
    await ProjectState.restoreRemoteLayout({
      projectRoot: '/srv/app',
      projectId,
      host,
      registry,
      saver,
      isCancelled: () => false,
      sleep: (ms) => {
        delays.push(ms);
        errorSeen = registry.layoutErrorsByProject[projectId] ?? null;
        return Promise.resolve();
      },
    });
    expect(delays).toEqual([2000]);
    expect(errorSeen).toBe("Could not restore this project's layout: ssh dropped");
    expect(registry.layoutErrorsByProject[projectId]).toBeUndefined();
    expect(registry.workspaces(projectId)).toEqual([]);
  });

  it('follows a remote project through its own client and never inits it', async () => {
    const { client, requests } = daemon(() => status([report('ag-1')]));
    const project = new ProjectState({
      projectRoot: '/srv/app',
      machineId,
      client,
      registry: null,
      environment: environment().env,
    });
    expect(project.isRemote).toBe(true);
    expect(project.projectId).toBe(`machine:${machineId}:/srv/app`);
    project.startWatching();
    await until(() => project.hasLoadedWorktrees);
    expect(requests.map((r) => r.type).filter((type) => type !== 'channel_read')).toEqual([
      'status',
    ]);
    expect(project.remoteLayoutSaver).not.toBeNull();
  });
});
