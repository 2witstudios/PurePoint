// Front end of the daemon backend (WIN-5.3): `src-tauri/src/daemon/`, ported from
// Services/DaemonClient.swift and Services/DaemonLifecycle.swift. Every request travels
// through the backend, which owns the connection to `pu-engine` and starts it if needed.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { Request, Response } from '../protocol';

/** The event the backend emits with the new {@link DaemonConnectionState} on every change. */
export const DAEMON_STATE_EVENT = 'daemon-state';

/** The local daemon connection, as `daemon_status` and the `daemon-state` event report it. */
export type DaemonConnectionState =
  | { state: 'idle' }
  | { state: 'starting' }
  /** `pid` is null when the daemon answered BUSY; `launched` when this app started it. */
  | { state: 'connected'; pid: number | null; launched: boolean }
  | { state: 'incompatible'; daemonVersion: number; appVersion: number; message: string }
  | { state: 'failed'; kind: DaemonErrorKind; message: string };

export type DaemonErrorKind =
  | 'not_running'
  | 'busy'
  | 'timeout'
  | 'eof'
  | 'malformed'
  | 'incompatible'
  | 'binary_not_found'
  | 'startup_timeout'
  | 'invalid_request'
  | 'io';

/** How a daemon command rejects. `message` is the text the error banner shows. */
export interface DaemonCommandError {
  kind: DaemonErrorKind;
  message: string;
}

export type HealthReport = Extract<Response, { type: 'health_report' }>;

/** True for a {@link DaemonCommandError} rejection. */
export function isDaemonError(error: unknown): error is DaemonCommandError {
  return (
    typeof error === 'object' &&
    error !== null &&
    typeof (error as { kind?: unknown }).kind === 'string' &&
    typeof (error as { message?: unknown }).message === 'string'
  );
}

/** One request to the local daemon. Error replies other than BUSY resolve as `error` responses. */
export function daemonRequest(request: Request): Promise<Response> {
  return invoke<Response>('daemon_request', { json: request });
}

/** The daemon's health report, starting the daemon if none is running. */
export function daemonHealth(): Promise<HealthReport> {
  return invoke<HealthReport>('daemon_health');
}

/** The current connection state. */
export function daemonStatus(): Promise<DaemonConnectionState> {
  return invoke<DaemonConnectionState>('daemon_status');
}

/** Calls `handler` with every connection state change. */
export function onDaemonState(
  handler: (state: DaemonConnectionState) => void,
): Promise<UnlistenFn> {
  return listen<DaemonConnectionState>(DAEMON_STATE_EVENT, (event) => {
    handler(event.payload);
  });
}

/** The banner text for a state, or null when there is nothing to report (`DaemonErrorBanner`). */
export function daemonBannerMessage(state: DaemonConnectionState): string | null {
  return state.state === 'incompatible' || state.state === 'failed' ? state.message : null;
}
