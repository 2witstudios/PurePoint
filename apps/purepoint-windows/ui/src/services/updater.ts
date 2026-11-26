// Front end of the in-place updater (WIN-10.3, REQ-WINAPP-076): `src-tauri/src/updater.rs`, the
// port of Services/CheckForUpdatesViewModel.swift (Sparkle → tauri-plugin-updater). The backend
// checks the signed `latest-windows.json` feed, downloads and verifies the update, and installs
// it after stopping Point Guard and the daemon; it reports every change as `updater-state`.
// The texts are Sparkle's standard update driver's, which the Mac shows.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** The event the backend emits with the new {@link UpdaterState} on every change. */
export const UPDATER_STATE_EVENT = 'updater-state';

const APP_NAME = 'PurePoint';

export type UpdaterState =
  | { state: 'idle' }
  /** The build carries the placeholder signing key: updates are off. */
  | { state: 'notConfigured' }
  | { state: 'checking' }
  | { state: 'upToDate'; currentVersion: string }
  | {
      state: 'available';
      version: string;
      currentVersion: string;
      notes: string | null;
      date: string | null;
    }
  | { state: 'downloading'; version: string; downloaded: number; total: number | null }
  | { state: 'readyToRestart'; version: string }
  | { state: 'installing'; version: string }
  | { state: 'error'; message: string };

export function updaterStatus(): Promise<UpdaterState> {
  return invoke<UpdaterState>('updater_status');
}

/** Check the feed (Check for Updates…). */
export function updaterCheck(): Promise<UpdaterState> {
  return invoke<UpdaterState>('updater_check');
}

/** Download and verify the available update (Install Update). */
export function updaterDownload(): Promise<UpdaterState> {
  return invoke<UpdaterState>('updater_download');
}

/** Stop Point Guard and the daemon, run the installer and quit (Install and Relaunch). */
export function updaterInstall(): Promise<UpdaterState> {
  return invoke<UpdaterState>('updater_install');
}

export function onUpdaterState(handler: (state: UpdaterState) => void): Promise<UnlistenFn> {
  return listen<UpdaterState>(UPDATER_STATE_EVENT, (event) => {
    handler(event.payload);
  });
}

/** Whether a new check may start (Sparkle's `canCheckForUpdates`, which the menu item follows). */
export function canCheckForUpdates(state: UpdaterState): boolean {
  return (
    state.state !== 'checking' && state.state !== 'downloading' && state.state !== 'installing'
  );
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${String(bytes)} bytes`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export interface UpdaterText {
  /** The dialog's headline. */
  readonly title: string;
  /** The line under it, or null. */
  readonly detail: string | null;
  /** The About pane's one-line status, or null when there is nothing to show. */
  readonly status: string | null;
}

/** Sparkle's texts for a state. */
export function updaterText(state: UpdaterState): UpdaterText {
  switch (state.state) {
    case 'idle':
      return { title: 'Check for Updates', detail: null, status: null };
    case 'notConfigured':
      return {
        title: 'Updates not configured',
        detail: `This build of ${APP_NAME} has no update signing key, so it can't verify or install updates.`,
        status: 'Updates not configured',
      };
    case 'checking':
      return { title: 'Checking for updates…', detail: null, status: 'Checking for updates…' };
    case 'upToDate':
      return {
        title: 'You’re up to date!',
        detail: `${APP_NAME} ${state.currentVersion} is currently the newest version available.`,
        status: 'You’re up to date!',
      };
    case 'available':
      return {
        title: `A new version of ${APP_NAME} is available!`,
        detail: `${APP_NAME} ${state.version} is now available—you have ${state.currentVersion}. Would you like to download it now?`,
        status: `${APP_NAME} ${state.version} is available`,
      };
    case 'downloading': {
      const progress =
        state.total === null
          ? formatBytes(state.downloaded)
          : `${formatBytes(state.downloaded)} of ${formatBytes(state.total)}`;
      return { title: 'Downloading update…', detail: progress, status: 'Downloading update…' };
    }
    case 'readyToRestart':
      return {
        title: 'Ready to Install',
        detail: `${APP_NAME} ${state.version} has been downloaded and is ready to use! Would you like to install it and relaunch ${APP_NAME} now?`,
        status: `${APP_NAME} ${state.version} is ready to install`,
      };
    case 'installing':
      return { title: 'Installing update…', detail: null, status: 'Installing update…' };
    case 'error':
      return { title: 'Update Error!', detail: state.message, status: 'Update Error!' };
  }
}

/** Download progress in 0…1, or null when the size is unknown or nothing downloads. */
export function downloadFraction(state: UpdaterState): number | null {
  if (state.state !== 'downloading' || state.total === null || state.total <= 0) return null;
  return Math.min(1, state.downloaded / state.total);
}
