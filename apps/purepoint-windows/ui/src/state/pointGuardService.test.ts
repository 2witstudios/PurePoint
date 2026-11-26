// PointGuardServiceState (WIN-9.3): the setup side of PointGuardServiceModel.swift against a
// fake backend. The process side (launch, readiness, stop, Quit) is tested in Rust.
import { describe, expect, it, vi } from 'vitest';
import type {
  JsonObject,
  JsonValue,
  PointGuardBackend,
  PointGuardSnapshot,
} from '../services/pointguard';
import {
  phoneAccessLevel,
  phoneAccessSteps,
  phoneAccessSummary,
  PointGuardServiceState,
} from './pointGuardService';

const STOPPED: PointGuardSnapshot = {
  phase: 'Stopped',
  error: null,
  ready: false,
  running: false,
  instanceId: null,
  remoteEndpoint: null,
  layout: null,
  restarts: 0,
};

function ready(instanceId = 'run-1', remoteEndpoint: string | null = null): PointGuardSnapshot {
  return {
    ...STOPPED,
    phase: 'Point Guard ready',
    ready: true,
    running: true,
    instanceId,
    remoteEndpoint,
    layout: 'bundled',
  };
}

type Handler = (operation: string, fields: JsonObject) => JsonValue | Promise<JsonValue>;

function fakeBackend(handler: Handler) {
  const sent: { operation: string; fields: JsonObject }[] = [];
  let emit: (snapshot: PointGuardSnapshot) => void = () => {};
  const mocks = {
    start: vi.fn((cwd?: string) => Promise.resolve(cwd).then(() => undefined)),
    restart: vi.fn((cwd?: string) => Promise.resolve(cwd).then(() => undefined)),
    request: vi.fn(async (operation: string, fields: JsonObject = {}) => {
      sent.push({ operation, fields });
      return handler(operation, fields);
    }),
  };
  const backend: PointGuardBackend = {
    status: () => Promise.resolve(STOPPED),
    start: mocks.start,
    stop: () => Promise.resolve(),
    restart: mocks.restart,
    request: mocks.request,
    chatConnection: vi.fn(() =>
      Promise.resolve({ endpoint: 'ws://127.0.0.1:1/v1', clientId: 'c' }),
    ),
    validateFolder: vi.fn((path: string) =>
      path.startsWith('C:\\')
        ? Promise.resolve(path)
        : Promise.reject(
            new Error('Choose an existing working folder before starting Point Guard.'),
          ),
    ),
    onState: vi.fn((handler: (snapshot: PointGuardSnapshot) => void) => {
      emit = handler;
      return Promise.resolve(() => {});
    }),
  };
  return {
    backend,
    mocks,
    sent,
    emit: (snapshot: PointGuardSnapshot) => {
      emit(snapshot);
    },
  };
}

const STATUS = {
  cwd: 'C:\\work',
  provider: 'anthropic',
  model: 'm1',
  remoteRecovery: { message: 'Connect Tailscale.', recovery: 'Then restart.' },
};

function standard(extra: Handler = () => ({})): Handler {
  return (operation, fields) => {
    switch (operation) {
      case 'status':
        return STATUS;
      case 'providers':
        return { providers: [{ id: 'anthropic', name: 'Anthropic' }] };
      case 'devices.list':
        return { devices: [{ deviceId: 'd1', terminals: 'view', workspaceManagement: true }] };
      default:
        return extra(operation, fields);
    }
  };
}

const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

async function readyState(handler: Handler, options = {}) {
  const fake = fakeBackend(handler);
  const connections: unknown[] = [];
  let disconnects = 0;
  const state = new PointGuardServiceState({
    backend: fake.backend,
    sleep: () => Promise.resolve(),
    onConnection: (c) => connections.push(c),
    onDisconnect: () => disconnects++,
    ...options,
  });
  await state.attach();
  fake.emit(ready('run-1', 'wss://100.100.1.2:8787/v1'));
  await flush();
  await flush();
  return { state, fake, connections, disconnects: () => disconnects };
}

describe('PointGuardPhoneAccess', () => {
  it('maps stored grants to one level', () => {
    expect(phoneAccessLevel('control', true)).toBe('full');
    expect(phoneAccessLevel('control', false)).toBe('terminals');
    expect(phoneAccessLevel('view', true)).toBe('view');
    expect(phoneAccessLevel('nope', false)).toBe('off');
  });

  it('orders writes so workspace management never outlives Control', () => {
    expect(phoneAccessSteps('control', true, 'view')).toEqual([
      { kind: 'workspaceManagement', allowed: false },
      { kind: 'terminals', terminals: 'view' },
    ]);
    expect(phoneAccessSteps('off', false, 'full')).toEqual([
      { kind: 'terminals', terminals: 'control' },
      { kind: 'workspaceManagement', allowed: true },
    ]);
    // A legacy management grant hidden under View is still cleared.
    expect(phoneAccessSteps('view', true, 'view')).toEqual([
      { kind: 'workspaceManagement', allowed: false },
    ]);
    expect(phoneAccessSteps('control', true, 'full')).toEqual([]);
  });

  it('describes grants for this PC', () => {
    expect(phoneAccessSummary('control', true)).toContain('on this PC');
    expect(phoneAccessSummary('off', false)).toBe(
      'No access to agents, terminals or Point Guard chat.',
    );
  });
});

describe('PointGuardServiceState', () => {
  it('mirrors backend snapshots and refreshes once ready', async () => {
    const { state, fake, connections } = await readyState(standard());
    const s = state.store.getState();
    expect(s.phase).toBe('Point Guard ready');
    expect(s.ready).toBe(true);
    expect(s.layout).toBe('bundled');
    expect(s.activeCwd).toBe('C:\\work');
    expect(s.cwd).toBe('C:\\work');
    expect(s.selectedProvider).toBe('anthropic');
    expect(s.selectedModel).toBe('m1');
    expect(s.remoteRecovery).toBe('Connect Tailscale. Then restart.');
    expect(s.remoteEndpoint).toBe('wss://100.100.1.2:8787/v1');
    expect(s.providers).toHaveLength(1);
    expect(s.devices).toHaveLength(1);
    expect(connections).toEqual([{ endpoint: 'ws://127.0.0.1:1/v1', clientId: 'c' }]);
    expect(fake.sent.map((r) => r.operation)).toEqual(['status', 'providers', 'devices.list']);
  });

  it('shows backend errors and refuses requests while stopped', async () => {
    const fake = fakeBackend(standard());
    const state = new PointGuardServiceState({ backend: fake.backend });
    await state.attach();
    fake.emit({ ...STOPPED, phase: 'Setup needs attention', error: 'Reinstall PurePoint.' });
    expect(state.error).toBe('Reinstall PurePoint.');
    expect(state.phase).toBe('Setup needs attention');
    await expect(state.request('status')).rejects.toThrow(
      'Start Point Guard before changing setup.',
    );
    expect(fake.mocks.request).not.toHaveBeenCalled();
  });

  it('signs in and polls until complete, then requires a restart', async () => {
    let polls = 0;
    const { state, fake } = await readyState(
      standard((operation) => {
        if (operation === 'auth.start') return { attemptId: 'a1' };
        if (operation === 'auth.status')
          return ++polls < 2
            ? { status: 'pending', attemptId: 'a1', prompt: { id: 'p1', message: 'Paste' } }
            : { status: 'complete', attemptId: 'a1' };
        return {};
      }),
    );
    await state.login('anthropic', 'oauth');
    expect(state.authMessage).toMatch(/^Signing in to Anthropic/);
    await vi.waitFor(() => {
      expect(state.store.getState().restartRequired).toBe(true);
    });
    expect(state.authMessage).toBe('Signed in to Anthropic.');
    expect(fake.sent.filter((r) => r.operation === 'auth.status')).toHaveLength(2);
  });

  it('keeps cancellations owned until the server confirms them', async () => {
    let failCancel = true;
    const { state, fake } = await readyState(
      standard((operation) => {
        if (operation === 'auth.start') return { attemptId: 'a1' };
        if (operation === 'auth.status') return new Promise<JsonValue>(() => {});
        if (operation === 'auth.cancel' && failCancel) throw new Error('unreachable');
        return {};
      }),
    );
    await state.login('anthropic', 'api');
    await state.cancelLogin();
    expect(state.store.getState().hasPendingAuthCleanup).toBe(true);
    expect(state.error).toMatch(/^Login cancellation could not be confirmed/);
    failCancel = false;
    await state.cancelLogin();
    expect(state.store.getState().hasPendingAuthCleanup).toBe(false);
    expect(state.authMessage).toBe('Sign-in canceled. Try again when you’re ready.');
    expect(fake.sent.filter((r) => r.operation === 'auth.cancel')).toHaveLength(2);
  });

  it('pairs a phone only on the actual tailnet endpoint and revokes a replaced code', async () => {
    let n = 0;
    const { state, fake } = await readyState(
      standard((operation) => {
        if (operation === 'pairing.create') return { enrollmentId: `e${String(++n)}`, code: 'x' };
        if (operation === 'pairing.status') return { status: 'consumed' };
        return {};
      }),
    );
    await state.connectPhone();
    expect(state.store.getState().enrollmentStatus).toBe('pending');
    expect(fake.sent.at(-1)).toEqual({
      operation: 'pairing.create',
      fields: { endpoint: 'wss://100.100.1.2:8787/v1' },
    });
    await state.connectPhone();
    expect(fake.sent.map((r) => r.operation)).toContain('pairing.revoke');
    await state.refreshEnrollment();
    expect(state.store.getState().enrollmentStatus).toBe('consumed');
    await state.closeEnrollment();
    expect(state.store.getState().enrollment).toBeNull();
  });

  it('asks for Tailscale when there is no phone endpoint', async () => {
    const fake = fakeBackend(standard());
    const state = new PointGuardServiceState({ backend: fake.backend });
    await state.attach();
    fake.emit(ready('run-1', null));
    await state.connectPhone();
    expect(state.error).toBe(
      'Connect Tailscale on this PC, then restart Point Guard to enable phone pairing.',
    );
  });

  it('changes device access in order and holds the row while it runs', async () => {
    const { state, fake } = await readyState(standard());
    const pending = state.setAccess('d1', 'full');
    expect(state.store.getState().deviceUpdates.has('d1')).toBe(true);
    await state.setAccess('d1', 'off'); // ignored while d1 is updating
    await pending;
    expect(state.store.getState().deviceUpdates.has('d1')).toBe(false);
    const writes = fake.sent.filter(
      (r) => r.operation.startsWith('devices.') && r.operation !== 'devices.list',
    );
    expect(writes).toEqual([
      { operation: 'devices.terminals', fields: { deviceId: 'd1', terminals: 'control' } },
    ]);
    await state.revokeDevice('d1');
    expect(fake.sent.some((r) => r.operation === 'devices.revoke')).toBe(true);
  });

  it('applies a folder now when ready and to the next launch when stopped', async () => {
    const { state, fake } = await readyState(standard());
    await state.configure('C:\\other');
    expect(fake.sent.at(-1)).toEqual({
      operation: 'runtime.configure',
      fields: { cwd: 'C:\\other' },
    });
    expect(state.store.getState().restartRequired).toBe(true);

    const stopped = fakeBackend(standard());
    const later = new PointGuardServiceState({ backend: stopped.backend });
    await later.attach();
    await later.configure('relative');
    expect(later.error).toBe('Choose an existing working folder before starting Point Guard.');
    await later.configure('C:\\recovery');
    expect(later.phase).toBe('Folder selected · Start / Retry');
    await later.start();
    expect(stopped.mocks.start).toHaveBeenCalledWith('C:\\recovery');
  });

  it('resets the run state when the owned runtime exits or is replaced', async () => {
    const { state, fake, disconnects } = await readyState(
      standard((operation) => (operation === 'pairing.create' ? { enrollmentId: 'e1' } : {})),
    );
    await state.connectPhone();
    expect(state.store.getState().enrollment).not.toBeNull();
    fake.emit({ ...STOPPED, phase: 'Point Guard stopped', error: 'Point Guard exited (7).' });
    const s = state.store.getState();
    expect(s.enrollment).toBeNull();
    expect(s.activeCwd).toBe('');
    expect(s.ready).toBe(false);
    expect(s.error).toBe('Point Guard exited (7).');
    expect(disconnects()).toBe(1);
    fake.emit(ready('run-2'));
    await flush();
    expect(state.store.getState().ready).toBe(true);
  });

  it('refuses to restart while the chat is busy', async () => {
    const { state, fake } = await readyState(standard());
    await state.restart(true);
    expect(state.error).toBe('Wait for Point Guard to finish or Stop this run before restarting.');
    expect(fake.mocks.restart).not.toHaveBeenCalled();
    await state.restart(false);
    expect(fake.mocks.restart).toHaveBeenCalledWith(undefined);
  });

  it('uses the request override without a runtime', async () => {
    const state = new PointGuardServiceState({
      backend: fakeBackend(standard()).backend,
      requestOverride: (operation) => Promise.resolve({ operation }),
    });
    await expect(state.request('status')).resolves.toEqual({ operation: 'status' });
  });
});
