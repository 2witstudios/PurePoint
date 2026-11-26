// Front end of the app-managed Point Guard runtime (WIN-9.3): `src-tauri/src/pointguard/`,
// ported from State/PointGuardServiceModel.swift (process side), Services/BundleSeal.swift,
// Services/PointGuardTailnet.swift and Services/PointGuardApplicationDelegate.swift.
//
// The backend owns the bridge process (start, readiness, stop, crash restart, the Quit
// wait) and relays owner admin requests over the bridge's owner pipe; it reports its state
// as `pointguard-state`.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

/** The event the backend emits with the new {@link PointGuardSnapshot} on every change. */
export const POINTGUARD_STATE_EVENT = 'pointguard-state';

export type PointGuardLayout = 'bundled' | 'development';

/** The backend's view of the owned runtime. */
export interface PointGuardSnapshot {
  phase: string;
  error: string | null;
  ready: boolean;
  /** An owned child exists (starting, ready or stopping). */
  running: boolean;
  instanceId: string | null;
  /** The phone endpoint (`chatURL`), when the bridge listens on the tailnet. */
  remoteEndpoint: string | null;
  layout: PointGuardLayout | null;
  /** Automatic restarts since the last stable run. */
  restarts: number;
}

/**
 * The local Point Guard chat (WIN-9.4 binds the chat to it). Its secret stays in the backend,
 * saved in Credential Manager under `endpoint`.
 */
export interface PointGuardChatConnection {
  endpoint: string;
  clientId: string;
}

export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };

/** The backend calls the state uses; tests pass a fake. */
export interface PointGuardBackend {
  status(): Promise<PointGuardSnapshot>;
  start(cwd?: string): Promise<void>;
  stop(): Promise<void>;
  restart(cwd?: string): Promise<void>;
  request(operation: string, fields?: JsonObject): Promise<JsonValue>;
  chatConnection(): Promise<PointGuardChatConnection>;
  validateFolder(path: string): Promise<string>;
  onState(handler: (snapshot: PointGuardSnapshot) => void): Promise<UnlistenFn>;
}

export const pointGuardBackend: PointGuardBackend = {
  status: () => invoke<PointGuardSnapshot>('pointguard_status'),
  start: async (cwd) => {
    await invoke('pointguard_start', { cwd: cwd ?? null });
  },
  stop: async () => {
    await invoke('pointguard_stop');
  },
  restart: async (cwd) => {
    await invoke('pointguard_restart', { cwd: cwd ?? null });
  },
  request: (operation, fields = {}) =>
    invoke<JsonValue>('pointguard_request', { operation, fields }),
  chatConnection: () => invoke<PointGuardChatConnection>('pointguard_chat_connection'),
  validateFolder: (path) => invoke<string>('pointguard_validate_folder', { path }),
  onState: (handler) =>
    listen<PointGuardSnapshot>(POINTGUARD_STATE_EVENT, (event) => {
      handler(event.payload);
    }),
};
