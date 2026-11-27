// App startup (WIN-7.1): what purepoint_macosApp.swift and ContentView start when the window
// opens — the theme following Windows, AppState's backend listeners (daemon state into the
// banner), the first daemon status, the projects subscription, the installer's state, and the
// updater's state (WIN-10.3), and the
// machines in `machines.json` (WIN-8.14), whose agents' terminals attach through their machine,
// and the Point Guard session (WIN-8.13), whose runtime folder becomes `pointGuardRoot`.
import { daemonStatus, type DaemonConnectionState } from '../../services/daemon.ts';
import {
  installerStatus,
  onInstallerState,
  type InstallerState,
} from '../../services/installer.ts';
import { localLayoutHostIO } from '../../services/fs.ts';
import { onUpdaterState, updaterStatus } from '../../services/updater.ts';
import { useUpdaterStore } from '../../state/updaterStore.ts';
import {
  AgentSignalNotifications,
  startAgentSignals,
} from '../../services/agentSignalNotifications.ts';
import { AppState } from '../../state/appState.ts';
import { startPointGuardSession } from '../../state/pointGuardSession.ts';
import { TauriMachineDirectory } from '../../state/machineState.ts';
import { configureTerminals } from '../terminal/terminals.ts';
import { CachedWorkspacePersistence } from '../../state/cachedWorkspacePersistence.ts';
import { WorkspaceRegistry } from '../../state/workspaceRegistry.ts';
import { startThemeSync } from '../../theme/themeStore.ts';
import { useInstallerBannerStore } from './ShellBanners.tsx';

export interface StartAppDeps {
  startThemeSync(): () => void;
  daemonStatus(): Promise<DaemonConnectionState>;
  installerStatus(): Promise<InstallerState>;
  onInstallerState(handler: (state: InstallerState) => void): Promise<() => void>;
  setInstallerState(state: InstallerState): void;
  /** Toasts and the taskbar badge from agent signals (WIN-7.7); returns its stop. */
  startAgentSignals?(app: AppState): () => void;
  /** Follows `updater-state` into the updater store (WIN-10.3); returns its stop. */
  startUpdater?(): () => void;
  /** The machines in `machines.json` and where remote agents' terminals attach (WIN-8.14). */
  loadMachines?(app: AppState): void;
  /** The Point Guard session: the service, the chat binding and the runtime folder (WIN-8.13). */
  startPointGuard?(app: AppState): () => void;
}

const defaultDeps: StartAppDeps = {
  startThemeSync: () => startThemeSync(),
  daemonStatus,
  installerStatus,
  onInstallerState,
  setInstallerState: (state) => {
    useInstallerBannerStore.getState().setInstaller(state);
  },
  startAgentSignals,
  startUpdater,
  loadMachines: (app) => {
    configureTerminals({
      machineIdFor: (agentId) => app.projectStateForAgent(agentId)?.machineId ?? undefined,
    });
    app.loadMachines().catch(logged('loading machines'));
  },
  startPointGuard: (app) => startPointGuardSession(app),
};

/** The updater store follows the backend: its events, then its current state. */
export function startUpdater(store = useUpdaterStore): () => void {
  let stopped = false;
  let unlisten: (() => void) | null = null;
  onUpdaterState((state) => {
    store.getState().setStatus(state);
  }).then((stop) => {
    if (stopped) stop();
    else unlisten = stop;
  }, logged('listening for the updater'));
  updaterStatus().then((state) => {
    if (!stopped) store.getState().setStatus(state);
  }, logged('reading the updater status'));
  return () => {
    stopped = true;
    unlisten?.();
  };
}

function logged(what: string) {
  return (error: unknown) => {
    console.error(`PurePoint: ${what} failed`, error);
  };
}

/**
 * The window's AppState with its workspace registry (purepoint_macosApp.swift's `.onAppear`:
 * `appState.registry = registry` and `registry.onCloseAgent`). The registry gives the sidebar its
 * workspace rows; it persists each local project's layout through the cached `.pu/workspaces.json`
 * mirror.
 */
export function createAppState(): AppState {
  let registry: WorkspaceRegistry | null = null;
  const persistence = new CachedWorkspacePersistence({
    host: localLayoutHostIO,
    onConflict: (projectRoot) => {
      registry?.adoptLocalLayoutIfChanged(projectRoot);
    },
    onWriteError: (projectRoot, message) => {
      console.error(`PurePoint: saving the layout of ${projectRoot} failed: ${message}`);
    },
  });
  registry = new WorkspaceRegistry({ persistence });
  const app = new AppState({
    registry,
    persistence,
    signalNotifications: new AgentSignalNotifications(),
    machines: new TauriMachineDirectory(),
  });
  registry.onCloseAgent = (projectId, agentId) => {
    app.projectState(projectId)?.removeAndKillAgent(agentId);
  };
  return app;
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
  // listeners is lost. The projects subscription starts only once the listeners are in: its
  // first `projects-changed` (the daemon's registered projects) is sent once, on subscribing,
  // and an event emitted before the webview listens is dropped, leaving the sidebar empty.
  const listening = app.connect().then(keep, logged('listening for backend events'));
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
  void listening.then(() => {
    if (!stopped) app.restoreProjects();
  });
  // Machines: connect the registered ones and follow the registry (WIN-8.14).
  if (deps.loadMachines !== undefined) deps.loadMachines(app);
  if (deps.startAgentSignals !== undefined) keep(deps.startAgentSignals(app));
  if (deps.startUpdater !== undefined) keep(deps.startUpdater());
  if (deps.startPointGuard !== undefined) keep(deps.startPointGuard(app));

  return () => {
    stopped = true;
    for (const stop of stops.splice(0)) stop();
    app.stopFollowingLocalProjects();
  };
}
