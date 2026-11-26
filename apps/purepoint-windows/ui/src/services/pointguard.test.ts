// Command wiring of the Point Guard wrappers; the runtime itself is tested in Rust.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
const listen =
  vi.fn<(event: string, handler: (event: { payload: unknown }) => void) => Promise<() => void>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));
vi.mock('@tauri-apps/api/event', () => ({
  listen: (event: string, handler: (event: { payload: unknown }) => void) => listen(event, handler),
}));

const { pointGuardBackend, POINTGUARD_STATE_EVENT } = await import('./pointguard');

describe('point guard service', () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
    invoke.mockResolvedValue(null);
  });

  it('maps every call to its command', async () => {
    await pointGuardBackend.status();
    expect(invoke).toHaveBeenLastCalledWith('pointguard_status', undefined);
    await pointGuardBackend.start();
    expect(invoke).toHaveBeenLastCalledWith('pointguard_start', { cwd: null });
    await pointGuardBackend.start('C:\\work');
    expect(invoke).toHaveBeenLastCalledWith('pointguard_start', { cwd: 'C:\\work' });
    await pointGuardBackend.stop();
    expect(invoke).toHaveBeenLastCalledWith('pointguard_stop', undefined);
    await pointGuardBackend.restart();
    expect(invoke).toHaveBeenLastCalledWith('pointguard_restart', { cwd: null });
    await pointGuardBackend.request('status');
    expect(invoke).toHaveBeenLastCalledWith('pointguard_request', {
      operation: 'status',
      fields: {},
    });
    await pointGuardBackend.request('auth.start', { provider: 'p' });
    expect(invoke).toHaveBeenLastCalledWith('pointguard_request', {
      operation: 'auth.start',
      fields: { provider: 'p' },
    });
    await pointGuardBackend.chatConnection();
    expect(invoke).toHaveBeenLastCalledWith('pointguard_chat_connection', undefined);
    await pointGuardBackend.validateFolder('C:\\x');
    expect(invoke).toHaveBeenLastCalledWith('pointguard_validate_folder', { path: 'C:\\x' });
  });

  it('forwards pointguard-state payloads', async () => {
    const unlisten = () => {};
    listen.mockResolvedValue(unlisten);
    const seen: unknown[] = [];
    await expect(pointGuardBackend.onState((s) => seen.push(s))).resolves.toBe(unlisten);
    expect(listen.mock.calls[0]?.[0]).toBe(POINTGUARD_STATE_EVENT);
    listen.mock.calls[0]?.[1]({ payload: { phase: 'Point Guard ready' } });
    expect(seen).toEqual([{ phase: 'Point Guard ready' }]);
  });
});
