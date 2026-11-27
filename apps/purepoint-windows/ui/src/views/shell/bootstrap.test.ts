import { describe, expect, it, vi } from 'vitest';
import type { DaemonConnectionState } from '../../services/daemon.ts';
import type { InstallerState } from '../../services/installer.ts';
import type { AppState } from '../../state/appState.ts';
import { startApp, type StartAppDeps } from './bootstrap.ts';

function fakeApp() {
  const stopListening = vi.fn();
  const app = {
    connect: vi.fn(() => Promise.resolve(stopListening)),
    handleDaemonState: vi.fn(),
    restoreProjects: vi.fn(),
    stopFollowingLocalProjects: vi.fn(),
  };
  return { app, stopListening };
}

function fakeDeps() {
  let installerHandler: ((state: InstallerState) => void) | null = null;
  const stopTheme = vi.fn();
  const stopInstaller = vi.fn();
  const deps = {
    startThemeSync: vi.fn(() => stopTheme),
    daemonStatus: vi.fn(() =>
      Promise.resolve<DaemonConnectionState>({ state: 'failed', kind: 'timeout', message: 'slow' }),
    ),
    installerStatus: vi.fn(() => Promise.resolve<InstallerState>({ state: 'running' })),
    onInstallerState: vi.fn((handler: (state: InstallerState) => void) => {
      installerHandler = handler;
      return Promise.resolve(stopInstaller);
    }),
    setInstallerState: vi.fn(),
  } satisfies StartAppDeps;
  return {
    deps,
    stopTheme,
    stopInstaller,
    emitInstaller: (state: InstallerState) => installerHandler?.(state),
  };
}

describe('startApp', () => {
  it('syncs the theme, listens to the backend, reads the first states and follows projects', async () => {
    const { app, stopListening } = fakeApp();
    const { deps, stopTheme, stopInstaller, emitInstaller } = fakeDeps();
    const stop = startApp(app as unknown as AppState, deps);
    await vi.waitFor(() => {
      expect(app.handleDaemonState).toHaveBeenCalledWith({
        state: 'failed',
        kind: 'timeout',
        message: 'slow',
      });
    });
    expect(deps.startThemeSync).toHaveBeenCalledOnce();
    expect(app.connect).toHaveBeenCalledOnce();
    await vi.waitFor(() => {
      expect(app.restoreProjects).toHaveBeenCalledOnce();
    });
    await vi.waitFor(() => {
      expect(deps.setInstallerState).toHaveBeenCalledWith({ state: 'running' });
    });
    emitInstaller({ state: 'failed', message: 'denied', report: null });
    expect(deps.setInstallerState).toHaveBeenLastCalledWith({
      state: 'failed',
      message: 'denied',
      report: null,
    });

    stop();
    expect(stopTheme).toHaveBeenCalledOnce();
    expect(stopListening).toHaveBeenCalledOnce();
    expect(stopInstaller).toHaveBeenCalledOnce();
    expect(app.stopFollowingLocalProjects).toHaveBeenCalledOnce();
  });

  it('follows the projects only once the backend listeners are in', async () => {
    const { app, stopListening } = fakeApp();
    let listen: (stop: typeof stopListening) => void = () => {};
    app.connect.mockReturnValueOnce(
      new Promise<typeof stopListening>((resolve) => {
        listen = resolve;
      }),
    );
    startApp(app as unknown as AppState, fakeDeps().deps);
    await Promise.resolve();
    expect(app.restoreProjects).not.toHaveBeenCalled();
    listen(stopListening);
    await vi.waitFor(() => {
      expect(app.restoreProjects).toHaveBeenCalledOnce();
    });
  });

  it('drops listeners that arrive after it stopped', async () => {
    const { app, stopListening } = fakeApp();
    const { deps } = fakeDeps();
    const stop = startApp(app as unknown as AppState, deps);
    stop();
    await vi.waitFor(() => {
      expect(stopListening).toHaveBeenCalledOnce();
    });
    await Promise.resolve();
    expect(app.handleDaemonState).not.toHaveBeenCalled();
    expect(app.restoreProjects).not.toHaveBeenCalled();
  });
});
