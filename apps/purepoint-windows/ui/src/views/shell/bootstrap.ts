// App startup (WIN-7.1): what purepoint_macosApp.swift and ContentView start when the window
// opens — the theme following Windows, AppState's backend listeners (daemon state into the
// banner), the first daemon status, the projects subscription, and the installer's state.
import { daemonStatus, type DaemonConnectionState } from '../../services/daemon.ts';
import {
  installerStatus,
  onInstallerState,
  type InstallerState,
} from '../../services/installer.ts';
import type { AppState } from '../../state/appState.ts';
import { startThemeSync } from '../../theme/themeStore.ts';
import { useInstallerBannerStore } from './ShellBanners.tsx';

export interface StartAppDeps {
  startThemeSync(): () => void;
  daemonStatus(): Promise<DaemonConnectionState>;
  installerStatus(): Promise<InstallerState>;
  onInstallerState(handler: (state: InstallerState) => void): Promise<() => void>;
  setInstallerState(state: InstallerState): void;
}

const defaultDeps: StartAppDeps = {
  startThemeSync: () => startThemeSync(),
  daemonStatus,
  installerStatus,
  onInstallerState,
  setInstallerState: (state) => {
    useInstallerBannerStore.getState().setInstaller(state);
  },
};

function logged(what: string) {
  return (error: unknown) => {
    console.error(`PurePoint: ${what} failed`, error);
  };
}

/** Starts everything; returns the function that stops the listeners. */
export function startApp(app: AppState, deps: StartAppDeps = defaultDeps): () => void {
  let stopped = false;
  const stops: (() => void)[] = [deps.startThemeSync()];
  const keep = (stop: () => void) => {
    if (stopped) stop();
    else stops.push(stop);
  };

  // Listen first, so no daemon or installer change between the first reads and the
  // listeners is lost.
  app.connect().then(keep, logged('listening for backend events'));
  deps
    .onInstallerState((state) => {
      deps.setInstallerState(state);
    })
    .then(keep, logged('listening for the installer'));

  deps.daemonStatus().then((state) => {
    if (!stopped) app.handleDaemonState(state);
  }, logged('reading the daemon status'));
  deps.installerStatus().then((state) => {
    if (!stopped) deps.setInstallerState(state);
  }, logged('reading the installer status'));

  // The local daemon's projects, followed from here on (the projects subscription).
  app.restoreProjects();

  return () => {
    stopped = true;
    for (const stop of stops.splice(0)) stop();
    app.stopFollowingLocalProjects();
  };
}
