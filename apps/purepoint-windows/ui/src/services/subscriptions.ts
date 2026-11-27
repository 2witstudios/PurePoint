// Front end of the daemon subscriptions (WIN-5.5): `src-tauri/src/subscriptions/`, ported
// from Services/{DaemonStatusSubscription,DaemonGridSubscription,DaemonProjectsSubscription}.swift.
// Each stream runs on its own daemon connection in the backend, which reconnects with the
// Swift backoff and raises one Tauri event per daemon message.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { GridCommand, Response } from '../protocol';

export const SUBSCRIPTION_EVENTS = {
  status: 'status-event',
  grid: 'grid-event',
  projects: 'projects-changed',
  state: 'subscription-state',
} as const;

type StatusEventResponse = Extract<Response, { type: 'status_event' }>;
type GridEventResponse = Extract<Response, { type: 'grid_event' }>;
type ProjectsChangedResponse = Extract<Response, { type: 'projects_changed' }>;

/**
 * Which daemon an event comes from: a machine's id for a remote project's streams (WIN-8.14),
 * absent for the local daemon's.
 */
interface EventSource {
  machineId?: string | null;
}

/** `status-event`: a project's full status, on subscribe and after every change. */
export type StatusEvent = EventSource & { projectRoot: string } & Pick<
    StatusEventResponse,
    'worktrees' | 'agents'
  >;

/** `grid-event`: a grid command for the app to apply to the project's pane tree. */
export type GridEvent = EventSource & { projectRoot: string } & Pick<GridEventResponse, 'command'>;

/** `projects-changed`: the local daemon's registered project roots, in registry order. */
export type ProjectsChanged = Pick<ProjectsChangedResponse, 'projects'>;

export type SubscriptionKind = 'status' | 'grid' | 'projects';

/**
 * `subscription-state`. `daemon_lost` (status only) and `failed` end the stream: after
 * `daemon_lost` the app ensures a daemon, inits the project and starts it again.
 */
export type SubscriptionStateEvent = EventSource & {
  kind: SubscriptionKind;
  /** Null for the projects stream. */
  projectRoot: string | null;
} & (
    | { state: 'subscribed' }
    | { state: 'retrying'; attempt: number; delayMs: number; message: string }
    | { state: 'daemon_lost' }
    | { state: 'failed'; message: string }
  );

/**
 * Starts (or restarts) a project's status and grid subscriptions: on its machine's daemon (over
 * the ssh relay) when `machineId` is given, else on the local daemon.
 */
export async function startProjectSubscriptions(
  projectRoot: string,
  machineId: string | null = null,
): Promise<void> {
  await invoke('subscriptions_start_project', { projectRoot, machineId });
}

/** Stops a project's subscriptions; resolves false if none were running. */
export function stopProjectSubscriptions(
  projectRoot: string,
  machineId: string | null = null,
): Promise<boolean> {
  return invoke<boolean>('subscriptions_stop_project', { projectRoot, machineId });
}

/** Follows the daemon's registered projects (starting the daemon first); false if already. */
export function startProjectsSubscription(): Promise<boolean> {
  return invoke<boolean>('subscriptions_start_projects');
}

/** Stops following the registered projects. */
export function stopProjectsSubscription(): Promise<boolean> {
  return invoke<boolean>('subscriptions_stop_projects');
}

/** Sends a grid command on the project's grid connection; false when it has none. */
export function sendGridCommand(
  projectRoot: string,
  command: GridCommand,
  machineId: string | null = null,
): Promise<boolean> {
  return invoke<boolean>('subscriptions_grid_command', { projectRoot, command, machineId });
}

export function onStatusEvent(handler: (event: StatusEvent) => void): Promise<UnlistenFn> {
  return listen<StatusEvent>(SUBSCRIPTION_EVENTS.status, (event) => {
    handler(event.payload);
  });
}

export function onGridEvent(handler: (event: GridEvent) => void): Promise<UnlistenFn> {
  return listen<GridEvent>(SUBSCRIPTION_EVENTS.grid, (event) => {
    handler(event.payload);
  });
}

export function onProjectsChanged(handler: (event: ProjectsChanged) => void): Promise<UnlistenFn> {
  return listen<ProjectsChanged>(SUBSCRIPTION_EVENTS.projects, (event) => {
    handler(event.payload);
  });
}

export function onSubscriptionState(
  handler: (event: SubscriptionStateEvent) => void,
): Promise<UnlistenFn> {
  return listen<SubscriptionStateEvent>(SUBSCRIPTION_EVENTS.state, (event) => {
    handler(event.payload);
  });
}
