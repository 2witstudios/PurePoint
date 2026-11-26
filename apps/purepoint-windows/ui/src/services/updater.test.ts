// Command wiring and texts of the updater front end; the check, signature and install are
// tested in Rust against a local feed (src-tauri/src/updater.rs).
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

const updater = await import('./updater');

describe('updater service', () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
  });

  it('sends the updater commands', async () => {
    invoke.mockResolvedValue({ state: 'idle' });
    await updater.updaterStatus();
    expect(invoke).toHaveBeenLastCalledWith('updater_status', undefined);
    await updater.updaterCheck();
    expect(invoke).toHaveBeenLastCalledWith('updater_check', undefined);
    await updater.updaterDownload();
    expect(invoke).toHaveBeenLastCalledWith('updater_download', undefined);
    await updater.updaterInstall();
    expect(invoke).toHaveBeenLastCalledWith('updater_install', undefined);
  });

  it('forwards updater-state payloads', async () => {
    const unlisten = () => {};
    listen.mockResolvedValue(unlisten);
    const seen: unknown[] = [];
    await expect(updater.onUpdaterState((state) => seen.push(state))).resolves.toBe(unlisten);
    expect(listen.mock.calls[0]?.[0]).toBe('updater-state');
    listen.mock.calls[0]?.[1]({ payload: { state: 'checking' } });
    expect(seen).toEqual([{ state: 'checking' }]);
  });

  it('allows a check unless one, a download or an install runs', () => {
    expect(updater.canCheckForUpdates({ state: 'idle' })).toBe(true);
    expect(updater.canCheckForUpdates({ state: 'notConfigured' })).toBe(true);
    expect(updater.canCheckForUpdates({ state: 'error', message: 'x' })).toBe(true);
    expect(updater.canCheckForUpdates({ state: 'checking' })).toBe(false);
    expect(
      updater.canCheckForUpdates({
        state: 'downloading',
        version: '0.2.0',
        downloaded: 1,
        total: null,
      }),
    ).toBe(false);
    expect(updater.canCheckForUpdates({ state: 'installing', version: '0.2.0' })).toBe(false);
  });

  it('uses the Sparkle texts', () => {
    expect(updater.updaterText({ state: 'checking' }).title).toBe('Checking for updates…');
    expect(updater.updaterText({ state: 'upToDate', currentVersion: '0.1.0' })).toEqual({
      title: 'You’re up to date!',
      detail: 'PurePoint 0.1.0 is currently the newest version available.',
      status: 'You’re up to date!',
    });
    const available = updater.updaterText({
      state: 'available',
      version: '0.2.0',
      currentVersion: '0.1.0',
      notes: null,
      date: null,
    });
    expect(available.title).toBe('A new version of PurePoint is available!');
    expect(available.detail).toBe(
      'PurePoint 0.2.0 is now available—you have 0.1.0. Would you like to download it now?',
    );
    expect(
      updater.updaterText({ state: 'downloading', version: '0.2.0', downloaded: 2048, total: 4096 })
        .detail,
    ).toBe('2 KB of 4 KB');
    expect(updater.updaterText({ state: 'readyToRestart', version: '0.2.0' }).title).toBe(
      'Ready to Install',
    );
    expect(updater.updaterText({ state: 'error', message: 'bad signature' })).toEqual({
      title: 'Update Error!',
      detail: 'bad signature',
      status: 'Update Error!',
    });
    expect(updater.updaterText({ state: 'notConfigured' }).status).toBe('Updates not configured');
  });

  it('reports download progress only when the size is known', () => {
    expect(
      updater.downloadFraction({ state: 'downloading', version: '1', downloaded: 5, total: 10 }),
    ).toBe(0.5);
    expect(
      updater.downloadFraction({ state: 'downloading', version: '1', downloaded: 5, total: null }),
    ).toBeNull();
    expect(updater.downloadFraction({ state: 'checking' })).toBeNull();
  });
});
