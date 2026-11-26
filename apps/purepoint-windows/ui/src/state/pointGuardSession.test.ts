// The app's Point Guard session (WIN-8.13): the runtime's folder becomes `pointGuardRoot` only once
// the home folder is known, so Pi's default folder (home) is never opened as a project.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => new Promise(() => undefined)) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));

const { AppState } = await import('./appState.ts');
const { WorkspaceRegistry } = await import('./workspaceRegistry.ts');
const { ManualScheduler, MemoryWorkspacePersistence } =
  await import('./workspaceRegistry.fixtures.ts');
const { PointGuardServiceState } = await import('./pointGuardService.ts');
const { makePointGuardSession, startPointGuardSession, SHOW_CONVERSATIONS_KEY } =
  await import('./pointGuardSession.ts');
const { PiChatModel, memorySecretStore } = await import('./piChat.ts');
const { memoryRecoveryStore } = await import('../services/pointGuardChat.ts');

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
    const stop = startPointGuardSession(app, session, home);
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
    const stop = startPointGuardSession(app, session, Promise.resolve('C:\\Users\\me'));
    await tick();
    service.store.setState({ activeCwd: 'C:\\Users\\me\\work' });
    expect(app.pointGuardRoot).toBe('C:\\Users\\me\\work');
    expect(started).toEqual(['C:\\Users\\me\\work']);
    expect(registry.activeWorkspaceId).toBe('ws-point-guard:C:\\Users\\me\\work');
    stop();
  });

  it('remembers the conversation rail', () => {
    const { session, storage } = setup();
    expect(session.showConversations.getState().shown).toBe(false);
    session.setShowConversations(true);
    expect(session.showConversations.getState().shown).toBe(true);
    expect(storage.get(SHOW_CONVERSATIONS_KEY)).toBe('true');
  });
});
