// Front end of the first-launch installer (WIN-10.1): `src-tauri/src/installer/`, ported from
// Services/CLIInstaller.swift. The backend installs `pu.exe`, `pu-engine.exe` and the Claude
// Code plugin in the background at startup and reports progress as `installer-state`.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** The event the backend emits with the new {@link InstallerState} on every change. */
export const INSTALLER_STATE_EVENT = 'installer-state';

export type BinaryAction =
  | 'notBundled'
  | 'upToDate'
  | 'installed'
  | 'updated'
  /** The old binary was running, so it was renamed aside to `<name>.old-<n>`. */
  | 'replacedInUse'
  | 'failed';

export type PluginAction = 'notBundled' | 'upToDate' | 'installed' | 'failed';

export type ClaudeAction = 'notNeeded' | 'notFound' | 'registered' | 'failed';

/** What one install run did. */
export interface InstallReport {
  binDir: string;
  binaries: { name: string; action: BinaryAction }[];
  staleRemoved: number;
  staleKept: number;
  pathAdded: boolean;
  plugin: PluginAction;
  claude: ClaudeAction;
  restartDaemon: boolean;
  daemonRestarted: boolean;
  errors: string[];
}

export type InstallerState =
  | { state: 'idle' }
  | { state: 'running' }
  | { state: 'installed'; report: InstallReport }
  /** `message` is the banner text; `report` says what did succeed. */
  | { state: 'failed'; message: string; report: InstallReport | null };

/** The installer's last state. */
export function installerStatus(): Promise<InstallerState> {
  return invoke<InstallerState>('installer_status');
}

/** Run the installer again (the banner's Retry) and resolve to its outcome. */
export function installerRun(): Promise<InstallerState> {
  return invoke<InstallerState>('installer_run');
}

/** Calls `handler` with every installer state change. */
export function onInstallerState(handler: (state: InstallerState) => void): Promise<UnlistenFn> {
  return listen<InstallerState>(INSTALLER_STATE_EVENT, (event) => {
    handler(event.payload);
  });
}

/** The banner text for a state, or null when there is nothing to report. */
export function installerBannerMessage(state: InstallerState): string | null {
  return state.state === 'failed' ? state.message : null;
}
