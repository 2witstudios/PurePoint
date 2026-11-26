// Command wiring of the installer wrappers; the install itself is tested in Rust.
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

const installer = await import('./installer');

describe('installer service', () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it('reads status and runs the installer', async () => {
    invoke.mockResolvedValueOnce({ state: 'idle' });
    await expect(installer.installerStatus()).resolves.toEqual({ state: 'idle' });
    expect(invoke).toHaveBeenLastCalledWith('installer_status', undefined);
    invoke.mockResolvedValueOnce({ state: 'running' });
    await installer.installerRun();
    expect(invoke).toHaveBeenLastCalledWith('installer_run', undefined);
  });

  it('forwards installer-state payloads', async () => {
    const unlisten = () => {};
    listen.mockResolvedValue(unlisten);
    const seen: unknown[] = [];
    await expect(installer.onInstallerState((state) => seen.push(state))).resolves.toBe(unlisten);
    expect(listen.mock.calls[0]?.[0]).toBe('installer-state');
    listen.mock.calls[0]?.[1]({ payload: { state: 'running' } });
    expect(seen).toEqual([{ state: 'running' }]);
  });

  it('shows the banner only for failures', () => {
    expect(installer.installerBannerMessage({ state: 'running' })).toBeNull();
    expect(
      installer.installerBannerMessage({
        state: 'failed',
        message: 'Could not install pu.exe',
        report: null,
      }),
    ).toBe('Could not install pu.exe');
  });
});
