// MachineState and MachineConnection (WIN-8.14): what MachinesTests.swift checks of a machine's
// live state, plus the Windows relay's probe verdicts.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { Request, Response } from '../protocol';
import type { HealthProbe, Machine } from '../model/machine';
import type { MachineConnectionState } from '../model/sidebarItem';
import { MachineConnection, type MachineBackend } from '../services/machines';
import { MemoryStorage } from '../model/sidebarFixtures';
import {
  LiveMachineState,
  type MachineConnectionLike,
  type MachineStateOwner,
  isUserDisconnected,
} from './machineState';
import type { ProjectState } from './projectState';

const MACHINE: Machine = { id: 'm1', name: 'studio', sshHost: 'studio-mac' };
const RUNNING: MachineConnectionState = { kind: 'connected', daemon: { kind: 'running' } };
const STOPPED: MachineConnectionState = { kind: 'connected', daemon: { kind: 'stopped' } };

function inventory(roots: string[]): Response {
  return {
    type: 'inventory',
    observed_at: '',
    project_root: null,
    complete: true,
    summary: { projects: roots.length, agents: 0, worktrees: 0 } as never,
    projects: roots.map((root) => ({
      project_root: root,
      name: root,
      available: true,
      error: null,
      counts: {} as never,
    })),
    agents: [],
    worktrees: [],
  };
}

class FakeDaemon {
  roots: string[] = [];
  requests: Request[] = [];
  initReply: Response = { type: 'init_result', created: true };

  send = (request: Request): Promise<Response> => {
    this.requests.push(request);
    switch (request.type) {
      case 'inventory':
        return Promise.resolve(inventory([...this.roots]));
      case 'init':
        if (this.initReply.type === 'init_result') this.roots.push(request.project_root);
        return Promise.resolve(this.initReply);
      case 'remove_project':
        this.roots = this.roots.filter((root) => root !== request.project_root);
        return Promise.resolve({
          type: 'project_removed',
          project_root: request.project_root,
        } as Response);
      default:
        return Promise.resolve({ type: 'error', code: 'UNSUPPORTED', message: request.type });
    }
  };
}

class FakeConnection implements MachineConnectionLike {
  connects = 0;
  disconnects = 0;
  startError: string | null = null;
  onState: (state: MachineConnectionState) => void = () => {};
  connect() {
    this.connects += 1;
  }
  disconnect() {
    this.disconnects += 1;
  }
  refreshNow() {}
  startDaemon() {
    return Promise.resolve(this.startError);
  }
}

function owner(): MachineStateOwner & { followed: Set<ProjectState>; active: string | null } {
  const followed = new Set<ProjectState>();
  const self = {
    registry: null,
    followed,
    active: null as string | null,
    setDaemonError: () => {},
    projectState: () => null,
    setActiveProjectRoot: () => {},
    setPendingSelectAgentId: () => {},
    setPendingSelectWorktreeId: () => {},
    machineChanged: () => {},
    followProject: (project: ProjectState) => followed.add(project),
    unfollowProject: (project: ProjectState) => followed.delete(project),
    initializeActiveProjectIfNeeded: (root: string) => {
      self.active ??= root;
    },
  };
  return self;
}

function setup() {
  const daemon = new FakeDaemon();
  const connection = new FakeConnection();
  const storage = new MemoryStorage();
  const appState = owner();
  const subscriptions: string[] = [];
  const machine = new LiveMachineState({
    machine: MACHINE,
    owner: appState,
    storage,
    makeClient: () => ({ send: daemon.send }),
    makeConnection: (_machine, onState) => {
      connection.onState = onState;
      return connection;
    },
    projectEnvironment: {
      startSubscriptions: (root, machineId) => {
        subscriptions.push(`${String(machineId)}:${root}`);
        return Promise.resolve();
      },
      stopSubscriptions: () => Promise.resolve(true),
      sleep: () => new Promise(() => {}),
      log: () => {},
    },
  });
  return { machine, daemon, connection, storage, appState, subscriptions };
}

const live: LiveMachineState[] = [];
afterEach(() => {
  for (const machine of live.splice(0)) machine.disconnect();
});

async function settle() {
  for (let i = 0; i < 5; i++) await Promise.resolve();
  await new Promise((resolve) => setTimeout(resolve, 0));
}

describe('LiveMachineState', () => {
  it('given a running daemon, should list its projects and follow them on the machine', async () => {
    const t = setup();
    live.push(t.machine);
    t.daemon.roots = ['/home/me/app', '/home/me/site'];
    t.machine.connect();
    await settle();
    expect(t.connection.connects).toBe(1);
    expect(t.machine.snapshot().statusText).toBe('Disconnected');

    t.connection.onState({ kind: 'connecting' });
    expect(t.machine.statusText).toBe('Connecting…');
    t.connection.onState(RUNNING);
    await settle();
    expect(t.machine.projects.map((p) => p.projectRoot)).toEqual(['/home/me/app', '/home/me/site']);
    expect(t.machine.projects.every((p) => p.machineId === 'm1')).toBe(true);
    expect(t.machine.projects[0]?.projectId).toBe('machine:m1:/home/me/app');
    expect(t.machine.hasLoadedProjects).toBe(true);
    expect(t.machine.statusText).toBe('2 projects');
    expect(t.subscriptions).toEqual(['m1:/home/me/app', 'm1:/home/me/site']);
    expect(t.appState.followed.size).toBe(2);
    const snapshot = t.machine.snapshot();
    expect(snapshot.projects.map((p) => p.projectId)).toEqual([
      'machine:m1:/home/me/app',
      'machine:m1:/home/me/site',
    ]);
  });

  it('given a stopped daemon, should drop its projects; offline keeps them', async () => {
    const t = setup();
    live.push(t.machine);
    t.daemon.roots = ['/srv/app'];
    t.machine.connect();
    t.connection.onState(RUNNING);
    await settle();
    t.connection.onState({ kind: 'offline', reason: 'ssh: timeout' });
    expect(t.machine.projects).toHaveLength(1);
    expect(t.machine.snapshot().statusText).toBe('Offline');
    t.connection.onState(STOPPED);
    expect(t.machine.projects).toHaveLength(0);
    expect(t.appState.followed.size).toBe(0);
    expect(t.machine.statusText).toBe('Daemon stopped');
  });

  it('given open project, should register the folder and list it under the machine', async () => {
    const t = setup();
    live.push(t.machine);
    t.machine.connect();
    t.connection.onState(RUNNING);
    await settle();
    await t.machine.openProject('/home/me/new');
    expect(t.daemon.requests).toContainEqual({ type: 'init', project_root: '/home/me/new' });
    expect(t.machine.projects.map((p) => p.projectRoot)).toEqual(['/home/me/new']);
    expect(t.appState.active).toBe('machine:m1:/home/me/new');

    t.daemon.initReply = { type: 'error', code: 'project_root_not_allowed', message: 'too broad' };
    await expect(t.machine.openProject('/home/me')).rejects.toThrow('too broad');
  });

  it('given hide and remove, should hide on this PC or remove from the machine', async () => {
    const t = setup();
    live.push(t.machine);
    t.daemon.roots = ['/a', '/b'];
    t.machine.connect();
    t.connection.onState(RUNNING);
    await settle();
    t.machine.hideProject('/a');
    expect(t.machine.projects.map((p) => p.projectRoot)).toEqual(['/b']);
    expect(t.machine.hiddenProjectCount).toBe(1);
    await t.machine.refreshProjects();
    expect(t.machine.projects.map((p) => p.projectRoot)).toEqual(['/b']);
    t.machine.showHiddenProjects();
    await settle();
    expect(t.machine.projects.map((p) => p.projectRoot)).toEqual(['/a', '/b']);

    await t.machine.removeProject('/b', true);
    expect(t.daemon.requests).toContainEqual({
      type: 'remove_project',
      project_root: '/b',
      suspend_agents: true,
    });
    expect(t.machine.projects.map((p) => p.projectRoot)).toEqual(['/a']);
  });

  it('given start daemon failing, should report it as the action error', async () => {
    const t = setup();
    live.push(t.machine);
    t.connection.startError = 'ssh exited with status 1';
    t.machine.startDaemon();
    expect(t.machine.isStartingDaemon).toBe(true);
    await settle();
    expect(t.machine.isStartingDaemon).toBe(false);
    expect(t.machine.actionError).toBe(
      'Could not start the daemon on studio: ssh exited with status 1',
    );
    t.machine.dismissError();
    expect(t.machine.actionError).toBeNull();
  });

  it('given an explicit disconnect, should stay disconnected at the next launch', async () => {
    const t = setup();
    live.push(t.machine);
    t.machine.connect();
    await settle();
    t.machine.disconnect({ remember: true });
    expect(t.connection.disconnects).toBe(1);
    expect(isUserDisconnected('m1', t.storage)).toBe(true);
    // Reports in flight from the closed connection do not reopen it.
    t.connection.onState(RUNNING);
    expect(t.machine.state).toEqual({ kind: 'disconnected' });
    t.machine.connect();
    expect(isUserDisconnected('m1', t.storage)).toBe(false);
  });
});

describe('MachineConnection', () => {
  function connection(probes: HealthProbe[]) {
    const states: MachineConnectionState[] = [];
    const delays: number[] = [];
    let wake: (() => void) | null = null;
    const backend: MachineBackend = {
      probe: () => Promise.resolve(probes.shift() ?? { kind: 'running' }),
      startDaemon: () => Promise.resolve(null),
    };
    const subject = new MachineConnection(
      MACHINE,
      (state) => {
        states.push(state);
      },
      {
        backend,
        sleep: (ms) => {
          delays.push(ms);
          return new Promise((resolve) => {
            wake = resolve;
          });
        },
      },
    );
    const step = async () => {
      await settle();
      const next = wake;
      wake = null;
      next?.();
      await settle();
    };
    return { subject, states, delays, step };
  }

  it('given relay verdicts, should report connecting, then each state, and back off offline', async () => {
    const c = connection([
      { kind: 'offline', reason: 'ssh: Could not resolve hostname' },
      { kind: 'offline', reason: 'ssh: Could not resolve hostname' },
      { kind: 'stopped' },
      { kind: 'running' },
    ]);
    c.subject.connect();
    await c.step();
    await c.step();
    await c.step();
    await settle();
    expect(c.states).toEqual([
      { kind: 'connecting' },
      { kind: 'offline', reason: 'ssh: Could not resolve hostname' },
      STOPPED,
      RUNNING,
    ]);
    expect(c.delays.slice(0, 4)).toEqual([2_000, 4_000, 10_000, 10_000]);
    c.subject.disconnect();
    expect(c.states.at(-1)).toEqual({ kind: 'disconnected' });
  });

  it('given failed probes, should keep the last state until three in a row', async () => {
    const c = connection([
      { kind: 'running' },
      { kind: 'failed', reason: 'x' },
      { kind: 'busy' },
      { kind: 'failed', reason: 'x' },
      { kind: 'failed', reason: 'x' },
    ]);
    c.subject.connect();
    for (let i = 0; i < 5; i++) await c.step();
    expect(c.states.slice(0, 3)).toEqual([{ kind: 'connecting' }, RUNNING, STOPPED]);
    expect(c.delays.slice(0, 5)).toEqual([10_000, 2_000, 2_000, 2_000, 10_000]);
    c.subject.disconnect();
  });

  it('given a probe that throws, should count it as a failure', async () => {
    const backend: MachineBackend = {
      probe: vi.fn(() => Promise.reject(new Error('invoke failed'))),
      startDaemon: () => Promise.resolve('no ssh'),
    };
    const states: MachineConnectionState[] = [];
    const subject = new MachineConnection(MACHINE, (s) => states.push(s), {
      backend,
      sleep: () => new Promise(() => {}),
    });
    subject.connect();
    await settle();
    expect(states).toEqual([{ kind: 'connecting' }]);
    await expect(subject.startDaemon()).resolves.toBe('no ssh');
    subject.disconnect();
  });
});
