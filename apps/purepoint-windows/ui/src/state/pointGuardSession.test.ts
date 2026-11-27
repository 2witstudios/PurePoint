// The app's Point Guard session (WIN-8.13): the runtime's folder becomes `pointGuardRoot` only once
// the home folder is known, so Pi's default folder (home) is never opened as a project.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => new Promise(() => undefined)) }));
vi.mock('@tauri-apps/api/path', () => ({ homeDir: vi.fn(() => Promise.reject(new Error('no'))) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));

const { AppState } = await import('./appState.ts');
const { WorkspaceRegistry } = await import('./workspaceRegistry.ts');
const { ManualScheduler, MemoryWorkspacePersistence } =
  await import('./workspaceRegistry.fixtures.ts');
const { PointGuardServiceState } = await import('./pointGuardService.ts');
const {
  makePointGuardSession,
  startPointGuardSession,
  SHOW_CONVERSATIONS_KEY,
  HOME_RETRY_FIRST_MS,
  HOME_RETRY_MAX_MS,
  lookupPointGuardHome,
} = await import('./pointGuardSession.ts');
const { PiChatModel, memorySecretStore } = await import('./piChat.ts');
const { memoryRecoveryStore } = await import('../services/pointGuardChat.ts');
const { invoke } = await import('@tauri-apps/api/core');
const { homeDir } = await import('@tauri-apps/api/path');

function setup() {
  const registry = new WorkspaceRegistry({
    persistence: new MemoryWorkspacePersistence(),
    scheduler: new ManualScheduler(),
  });
  const started: string[] = [];
  const app = new AppState({
    registry,
    client: { send: () => Promise.resolve({ type: 'ok' }) },
    startProject: (project) => started.push(project.projectRoot),
    followsLocalDaemon: false,
    storage: null,
  });
  const service = new PointGuardServiceState({
    backend: {
      status: () => new Promise(() => undefined),
      start: () => Promise.resolve(),
      stop: () => Promise.resolve(),
      restart: () => Promise.resolve(),
      request: () => Promise.resolve(null),
      chatConnection: () => Promise.reject(new Error('unused')),
      validateFolder: (path) => Promise.resolve(path),
      onState: () => Promise.resolve(() => undefined),
    },
  });
  const chat = new PiChatModel({
    recovery: memoryRecoveryStore(),
    secrets: memorySecretStore(),
    preferences: null,
  });
  const storage = new Map<string, string>();
  const session = makePointGuardSession({
    chat,
    service,
    storage: { getItem: (k) => storage.get(k) ?? null, setItem: (k, v) => storage.set(k, v) },
  });
  return { app, registry, service, session, started, storage };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('startPointGuardSession', () => {
  it('waits for the home folder, then keeps home out of the projects', async () => {
    const { app, registry, service, session, started } = setup();
    let resolveHome: (home: string) => void = () => undefined;
    const home = new Promise<string | null>((resolve) => {
      resolveHome = resolve;
    });
    const stop = startPointGuardSession(app, session, () => home);
    service.store.setState({ activeCwd: 'C:\\Users\\me' });
    expect(app.pointGuardRoot).toBeNull();
    resolveHome('C:\\Users\\me');
    await tick();
    expect(app.pointGuardRoot).toBe('C:\\Users\\me');
    expect(app.isTooBroadProjectRoot('C:\\Users\\me')).toBe(true);
    expect(started).toEqual([]);
    expect(registry.activeWorkspaceId).toBeNull();
    stop();
  });

  it('follows a project folder into the Point Guard workspace', async () => {
    const { app, registry, service, session, started } = setup();
    const stop = startPointGuardSession(app, session, () => Promise.resolve('C:\\Users\\me'));
    await tick();
    service.store.setState({ activeCwd: 'C:\\Users\\me\\work' });
    expect(app.pointGuardRoot).toBe('C:\\Users\\me\\work');
    expect(started).toEqual(['C:\\Users\\me\\work']);
    expect(registry.activeWorkspaceId).toBe('ws-point-guard:C:\\Users\\me\\work');
    stop();
  });

  it('given an unknown home, should follow nothing until a retried lookup knows it', async () => {
    vi.useFakeTimers();
    try {
      const { app, registry, service, session, started } = setup();
      const answers: (string | null)[] = [null, null, 'C:\\Users\\me'];
      const home = vi.fn(() => {
        const answer = answers.shift() ?? null;
        return answer === null ? Promise.reject(new Error('unknown')) : Promise.resolve(answer);
      });
      const stop = startPointGuardSession(app, session, home);
      service.store.setState({ activeCwd: 'C:\\Users\\me' });
      await vi.advanceTimersByTimeAsync(0);
      // Fail closed: Pi's default folder (home) is not registered while home is unknown.
      expect(app.pointGuardRoot).toBeNull();
      expect(started).toEqual([]);
      expect(home).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(HOME_RETRY_FIRST_MS);
      expect(home).toHaveBeenCalledTimes(2);
      expect(app.pointGuardRoot).toBeNull();
      await vi.advanceTimersByTimeAsync(HOME_RETRY_FIRST_MS * 2);
      expect(home).toHaveBeenCalledTimes(3);
      expect(app.pointGuardRoot).toBe('C:\\Users\\me');
      expect(app.isTooBroadProjectRoot('C:\\Users\\me')).toBe(true);
      expect(started).toEqual([]);
      expect(registry.activeWorkspaceId).toBeNull();
      stop();
    } finally {
      vi.useRealTimers();
    }
  });

  it('given a stop while home is unknown, should stop retrying', async () => {
    vi.useFakeTimers();
    try {
      const { app, session } = setup();
      const home = vi.fn(() => Promise.resolve(null));
      const stop = startPointGuardSession(app, session, home);
      await vi.advanceTimersByTimeAsync(0);
      stop();
      await vi.advanceTimersByTimeAsync(HOME_RETRY_MAX_MS * 4);
      expect(home).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it('given pointguard_home fails, should fall back to the Tauri home folder, else null', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error('backend down'));
    vi.mocked(homeDir).mockResolvedValueOnce('C:\\Users\\me');
    await expect(lookupPointGuardHome()).resolves.toBe('C:\\Users\\me');
    vi.mocked(invoke).mockRejectedValueOnce(new Error('backend down'));
    await expect(lookupPointGuardHome()).resolves.toBeNull();
  });

  it('remembers the conversation rail', () => {
    const { session, storage } = setup();
    expect(session.showConversations.getState().shown).toBe(false);
    session.setShowConversations(true);
    expect(session.showConversations.getState().shown).toBe(true);
    expect(storage.get(SHOW_CONVERSATIONS_KEY)).toBe('true');
  });
});
