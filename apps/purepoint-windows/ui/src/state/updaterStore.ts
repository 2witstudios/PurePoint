// The updater's state for the menu item, the update dialog and the About status (WIN-10.3): the
// `@Published` side of Services/CheckForUpdatesViewModel.swift. The backend owns the state; this
// mirrors its `updater-state` events and the replies of the commands it sends.
import { create } from 'zustand';
import {
  canCheckForUpdates,
  updaterCheck,
  updaterDownload,
  updaterInstall,
  type UpdaterState,
} from '../services/updater.ts';

export interface UpdaterStoreState {
  readonly status: UpdaterState;
  /** The update dialog (Sparkle's update window) is shown. */
  readonly dialogOpen: boolean;
  setStatus(status: UpdaterState): void;
  openDialog(): void;
  closeDialog(): void;
}

export function createUpdaterStore() {
  return create<UpdaterStoreState>()((set) => ({
    status: { state: 'idle' },
    dialogOpen: false,
    setStatus: (status) => {
      set({ status });
    },
    openDialog: () => {
      set({ dialogOpen: true });
    },
    closeDialog: () => {
      set({ dialogOpen: false });
    },
  }));
}

export type UpdaterStore = ReturnType<typeof createUpdaterStore>;

export const useUpdaterStore = createUpdaterStore();

/** The backend commands, injectable for tests. */
export interface UpdaterCommands {
  check(): Promise<UpdaterState>;
  download(): Promise<UpdaterState>;
  install(): Promise<UpdaterState>;
}

const backend: UpdaterCommands = {
  check: updaterCheck,
  download: updaterDownload,
  install: updaterInstall,
};

function errorText(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

function run(
  store: UpdaterStore,
  step: () => Promise<UpdaterState>,
  pending: UpdaterState | null,
): Promise<void> {
  if (pending !== null) store.getState().setStatus(pending);
  return step().then(
    (status) => {
      store.getState().setStatus(status);
    },
    (error: unknown) => {
      store.getState().setStatus({ state: 'error', message: errorText(error) });
    },
  );
}

/**
 * Check for Updates…: shows the dialog and checks, unless a check, download or install is
 * already running (then the dialog shows that). With the placeholder key the backend answers
 * `notConfigured` without fetching anything.
 */
export function checkForUpdates(
  store: UpdaterStore = useUpdaterStore,
  commands: UpdaterCommands = backend,
): Promise<void> {
  store.getState().openDialog();
  const status = store.getState().status;
  if (!canCheckForUpdates(status)) return Promise.resolve();
  return run(
    store,
    () => commands.check(),
    status.state === 'notConfigured' ? null : { state: 'checking' },
  );
}

/** Install Update: download and verify. */
export function downloadUpdate(
  store: UpdaterStore = useUpdaterStore,
  commands: UpdaterCommands = backend,
): Promise<void> {
  const status = store.getState().status;
  if (status.state !== 'available') return Promise.resolve();
  return run(store, () => commands.download(), {
    state: 'downloading',
    version: status.version,
    downloaded: 0,
    total: null,
  });
}

/** Install and Relaunch: the app quits once the installer starts. */
export function installUpdate(
  store: UpdaterStore = useUpdaterStore,
  commands: UpdaterCommands = backend,
): Promise<void> {
  const status = store.getState().status;
  if (status.state !== 'readyToRestart') return Promise.resolve();
  return run(store, () => commands.install(), { state: 'installing', version: status.version });
}

/**
 * A check nobody asked for (Sparkle's scheduled check): it shows the dialog only when an update
 * is available, and stays out of the way while the dialog is open, an update is under way or
 * already found, or updates are not configured.
 */
export function checkForUpdatesInBackground(
  store: UpdaterStore = useUpdaterStore,
  commands: UpdaterCommands = backend,
): Promise<void> {
  const { status, dialogOpen } = store.getState();
  const settled =
    status.state === 'available' ||
    status.state === 'readyToRestart' ||
    status.state === 'notConfigured';
  if (dialogOpen || settled || !canCheckForUpdates(status)) return Promise.resolve();
  return commands.check().then(
    (result) => {
      store.getState().setStatus(result);
      if (result.state === 'available') store.getState().openDialog();
    },
    () => {
      // A failed background check stays quiet; the next one, or Check for Updates…, retries.
    },
  );
}

/** First automatic check after launch, then one a day (Sparkle's default interval). */
export const AUTOMATIC_CHECK_DELAY_MS = 30_000;
export const AUTOMATIC_CHECK_INTERVAL_MS = 24 * 60 * 60 * 1000;

/** Check for updates shortly after launch and then daily; returns its stop. */
export function startAutomaticUpdateChecks(
  store: UpdaterStore = useUpdaterStore,
  commands: UpdaterCommands = backend,
): () => void {
  const check = () => {
    void checkForUpdatesInBackground(store, commands);
  };
  let interval: ReturnType<typeof setInterval> | null = null;
  const first = setTimeout(() => {
    check();
    interval = setInterval(check, AUTOMATIC_CHECK_INTERVAL_MS);
  }, AUTOMATIC_CHECK_DELAY_MS);
  return () => {
    clearTimeout(first);
    if (interval !== null) clearInterval(interval);
  };
}
