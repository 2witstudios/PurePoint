// Command and event wiring of the subscription wrappers; the streams are tested in Rust.
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { GridCommand } from '../protocol';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

type Listener = (event: { payload: unknown }) => void;
const listeners = new Map<string, Listener>();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, handler: Listener) => {
    listeners.set(name, handler);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

import type { GridEvent, StatusEvent, SubscriptionStateEvent } from './subscriptions';

const subscriptions = await import('./subscriptions');

describe('subscription service', () => {
  beforeEach(() => {
    invoke.mockReset();
    listeners.clear();
  });

  it('starts and stops subscriptions with camelCase arguments', async () => {
    invoke.mockResolvedValue(true);
    await subscriptions.startProjectSubscriptions('C:\\p');
    expect(invoke).toHaveBeenLastCalledWith('subscriptions_start_project', {
      projectRoot: 'C:\\p',
    });
    await subscriptions.stopProjectSubscriptions('C:\\p');
    expect(invoke).toHaveBeenLastCalledWith('subscriptions_stop_project', { projectRoot: 'C:\\p' });
    await subscriptions.startProjectsSubscription();
    expect(invoke).toHaveBeenLastCalledWith('subscriptions_start_projects', undefined);
    await subscriptions.stopProjectsSubscription();
    expect(invoke).toHaveBeenLastCalledWith('subscriptions_stop_projects', undefined);
    const command: GridCommand = { action: 'split', leaf_id: 1, axis: 'h' };
    await expect(subscriptions.sendGridCommand('C:\\p', command)).resolves.toBe(true);
    expect(invoke).toHaveBeenLastCalledWith('subscriptions_grid_command', {
      projectRoot: 'C:\\p',
      command,
    });
  });

  it('delivers typed event payloads to listeners', async () => {
    const status = vi.fn();
    const grid = vi.fn();
    const projects = vi.fn();
    const state = vi.fn();
    const unlisten = await subscriptions.onStatusEvent(status);
    await subscriptions.onGridEvent(grid);
    await subscriptions.onProjectsChanged(projects);
    await subscriptions.onSubscriptionState(state);

    const statusEvent: StatusEvent = {
      projectRoot: 'C:\\p',
      worktrees: [],
      agents: [],
    };
    const gridEvent: GridEvent = {
      projectRoot: 'C:\\p',
      command: { action: 'get_layout' },
    };
    const lost: SubscriptionStateEvent = {
      kind: 'status',
      projectRoot: 'C:\\p',
      state: 'daemon_lost',
    };
    listeners.get('status-event')?.({ payload: statusEvent });
    listeners.get('grid-event')?.({ payload: gridEvent });
    listeners.get('projects-changed')?.({ payload: { projects: ['C:\\a'] } });
    listeners.get('subscription-state')?.({ payload: lost });
    expect(status).toHaveBeenCalledWith(statusEvent);
    expect(grid).toHaveBeenCalledWith(gridEvent);
    expect(projects).toHaveBeenCalledWith({ projects: ['C:\\a'] });
    expect(state).toHaveBeenCalledWith(lost);
    unlisten();
    expect(listeners.has('status-event')).toBe(false);
  });
});
