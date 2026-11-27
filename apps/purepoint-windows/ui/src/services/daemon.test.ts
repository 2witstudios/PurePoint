// Command wiring of the daemon wrappers; the client and lifecycle are tested in Rust.
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

const daemon = await import('./daemon');

describe('daemon service', () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it('sends requests as json', async () => {
    invoke.mockResolvedValue({ type: 'ok' });
    await expect(daemon.daemonRequest({ type: 'health' })).resolves.toEqual({ type: 'ok' });
    expect(invoke).toHaveBeenCalledWith('daemon_request', { json: { type: 'health' } });
  });

  it('reads health and status', async () => {
    invoke.mockResolvedValueOnce({ type: 'health_report', pid: 1 });
    await daemon.daemonHealth();
    expect(invoke).toHaveBeenLastCalledWith('daemon_health', undefined);
    invoke.mockResolvedValueOnce({ state: 'idle' });
    await expect(daemon.daemonStatus()).resolves.toEqual({ state: 'idle' });
    expect(invoke).toHaveBeenLastCalledWith('daemon_status', undefined);
  });

  it('forwards daemon-state payloads', async () => {
    const unlisten = () => {};
    listen.mockResolvedValue(unlisten);
    const seen: unknown[] = [];
    await expect(daemon.onDaemonState((state) => seen.push(state))).resolves.toBe(unlisten);
    expect(listen.mock.calls[0]?.[0]).toBe('daemon-state');
    listen.mock.calls[0]?.[1]({ payload: { state: 'starting' } });
    expect(seen).toEqual([{ state: 'starting' }]);
  });

  it('shows the banner only for failures', () => {
    expect(daemon.daemonBannerMessage({ state: 'connected', pid: 1, launched: true })).toBeNull();
    expect(
      daemon.daemonBannerMessage({
        state: 'incompatible',
        daemonVersion: 3,
        appVersion: 14,
        message: 'Incompatible daemon (protocol 3)',
      }),
    ).toBe('Incompatible daemon (protocol 3)');
    expect(daemon.daemonBannerMessage({ state: 'failed', kind: 'eof', message: 'closed' })).toBe(
      'closed',
    );
  });

  it('recognizes command errors', () => {
    expect(daemon.isDaemonError({ kind: 'busy', message: 'full' })).toBe(true);
    expect(daemon.isDaemonError(new Error('x'))).toBe(false);
    expect(daemon.isDaemonError(null)).toBe(false);
  });
});
