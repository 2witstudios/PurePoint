// AgentSignalNotifications (WIN-7.7): the taskbar badge, toasts while the window is unfocused,
// the focus-taken route, acknowledged errors and the per-kind settings.
import { describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => Promise.resolve()) }));
vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({ onFocusChanged: () => Promise.resolve(() => undefined) }),
}));

const {
  AgentSignalNotifications,
  badgeUpdate,
  signalAlertKind,
  signalNotificationsEnabled,
  signalToastBody,
} = await import('./agentSignalNotifications');
const { makeSignalAlertAgent } = await import('../model/agentSignalPolicy');
type SignalAlertAgent = import('../model/agentSignalPolicy').SignalAlertAgent;

const tick = () =>
  new Promise<void>((resolve) => {
    setTimeout(resolve, 0);
  });

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    map,
    getItem: (key: string) => map.get(key) ?? null,
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

function setup(initial: Record<string, string> = {}) {
  const storage = memoryStorage(initial);
  const toasts: { title: string; body: string }[] = [];
  const badges: number[] = [];
  const host = {
    storage,
    postToast: (toast: { title: string; body: string }) => {
      toasts.push(toast);
      return Promise.resolve();
    },
    setBadge: (count: number) => {
      badges.push(count);
      return Promise.resolve();
    },
  };
  const notifications = new AgentSignalNotifications(host);
  const opened: SignalAlertAgent[] = [];
  notifications.open = (agent) => {
    opened.push(agent);
    return Promise.resolve(true);
  };
  return { notifications, storage, toasts, badges, opened };
}

const destination = (agentId: string, eventId: string) => ({
  projectId: 'C:\\repo',
  worktreeId: null,
  agentId,
  launchId: 'launch',
  generation: 1,
  nativeSessionId: 'native',
  eventId,
});

const alerting = (identity: string, label: string, eventId: string, needsAttention = true) =>
  makeSignalAlertAgent({
    identity,
    launch: 'launch',
    presentation: { label, eventId, needsAttention, detail: '' },
    destination: destination(identity, eventId),
  });

describe('AgentSignalNotifications', () => {
  it('shows the attention count on the taskbar, applied on change and cleared at zero', async () => {
    const { notifications, badges } = setup();
    notifications.update([alerting('a', 'Permission', 'e1'), alerting('b', 'Error', 'e2')], null);
    notifications.update([alerting('a', 'Permission', 'e1'), alerting('b', 'Error', 'e2')], null);
    notifications.update([], null);
    await tick();
    expect(badges).toEqual([2, 0]);
  });

  it('posts no toast while the window is focused, and does not post it later', () => {
    const { notifications, toasts } = setup();
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    notifications.windowFocusChanged(false);
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    expect(toasts).toHaveLength(0);
  });

  it('posts one content-free toast per new event while unfocused', () => {
    const { notifications, toasts, storage } = setup();
    notifications.windowFocusChanged(false);
    notifications.update([alerting('a', 'Question', 'e1')], null);
    notifications.update([alerting('a', 'Question', 'e1')], null);
    notifications.update([alerting('a', 'Question', 'e2')], null);
    expect(toasts).toEqual([
      { title: 'PurePoint', body: 'An agent needs attention.' },
      { title: 'PurePoint', body: 'An agent needs attention.' },
    ]);
    expect(JSON.parse(storage.map.get('PP_signalDeliveredEvents') ?? '[]')).toHaveLength(2);
  });

  it('does not repeat an event delivered before a restart', () => {
    const first = setup();
    first.notifications.windowFocusChanged(false);
    first.notifications.update([alerting('a', 'Permission', 'e1')], null);
    const second = setup(Object.fromEntries(first.storage.map));
    second.notifications.windowFocusChanged(false);
    second.notifications.update([alerting('a', 'Permission', 'e1')], null);
    expect(first.toasts).toHaveLength(1);
    expect(second.toasts).toHaveLength(0);
  });

  it('skips the agent on screen, one without a route, and finished turns by default', () => {
    const { notifications, toasts } = setup();
    notifications.windowFocusChanged(false);
    const unrouted = makeSignalAlertAgent({
      identity: 'c',
      launch: 'launch',
      presentation: { label: 'Permission', eventId: 'e3', needsAttention: true, detail: '' },
    });
    notifications.update(
      [alerting('a', 'Permission', 'e1'), alerting('b', 'Finished', 'e2', false), unrouted],
      'a',
    );
    expect(toasts).toHaveLength(0);
  });

  it('posts finished turns once they are turned on', () => {
    const { notifications, toasts } = setup({ PP_signal_completionNotifications: 'true' });
    notifications.windowFocusChanged(false);
    notifications.update([alerting('b', 'Finished', 'e2', false)], null);
    expect(toasts).toEqual([{ title: 'PurePoint', body: 'An agent finished its turn.' }]);
  });

  it('routes to the newest toast when the window regains focus', async () => {
    const { notifications, opened } = setup();
    notifications.windowFocusChanged(false);
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    notifications.update([alerting('a', 'Permission', 'e1'), alerting('b', 'Error', 'e2')], null);
    notifications.windowFocusChanged(true);
    await tick();
    expect(opened.map((agent) => agent.identity)).toEqual(['b']);
    notifications.windowFocusChanged(false);
    notifications.windowFocusChanged(true);
    await tick();
    expect(opened).toHaveLength(1);
  });

  it('does not route to a toast whose event is gone', async () => {
    const { notifications, opened } = setup();
    notifications.windowFocusChanged(false);
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    notifications.update([alerting('a', 'Working', 'unused', false)], null);
    notifications.windowFocusChanged(true);
    await tick();
    expect(opened).toHaveLength(0);
  });

  it('keeps a tap pending until its route opens', async () => {
    const { notifications, opened } = setup();
    let allow = false;
    notifications.open = (agent) => {
      opened.push(agent);
      return Promise.resolve(allow);
    };
    notifications.windowFocusChanged(false);
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    notifications.windowFocusChanged(true);
    await tick();
    allow = true;
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    await tick();
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    await tick();
    expect(opened).toHaveLength(2);
  });

  it('acknowledges an error seen on screen, dropping it from the badge for good', async () => {
    const { notifications, badges, storage } = setup();
    const error = alerting('a', 'Error', 'e1');
    notifications.update([error], null);
    notifications.update([error], 'a');
    notifications.update([error], null);
    await tick();
    expect(badges).toEqual([1, 0]);
    expect(JSON.parse(storage.map.get('PP_signalAcknowledgedErrors') ?? '[]')).toHaveLength(1);
  });

  it('retries the badge after a failure', async () => {
    const storage = memoryStorage();
    const calls: number[] = [];
    let fail = true;
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const notifications = new AgentSignalNotifications({
      storage,
      postToast: () => Promise.resolve(),
      setBadge: (count) => {
        calls.push(count);
        return fail ? Promise.reject(new Error('no taskbar')) : Promise.resolve();
      },
    });
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    await tick();
    fail = false;
    notifications.update([alerting('a', 'Permission', 'e1')], null);
    await tick();
    expect(calls).toEqual([1, 1]);
    errors.mockRestore();
  });
});

describe('signal notification settings and text', () => {
  it('names kinds as the Mac settings do', () => {
    expect(signalAlertKind('Finished')).toBe('completion');
    expect(signalAlertKind('Permission')).toBe('permission');
    expect(signalAlertKind('Error')).toBe('error');
  });

  it('defaults attention kinds on and finished turns off', () => {
    const storage = memoryStorage({ PP_signal_errorNotifications: 'false' });
    expect(signalNotificationsEnabled(storage, 'permission')).toBe(true);
    expect(signalNotificationsEnabled(storage, 'question')).toBe(true);
    expect(signalNotificationsEnabled(storage, 'error')).toBe(false);
    expect(signalNotificationsEnabled(storage, 'completion')).toBe(false);
    expect(signalNotificationsEnabled(null, 'permission')).toBe(true);
  });

  it('never puts request content in the toast', () => {
    expect(signalToastBody('Permission')).toBe('An agent needs attention.');
    expect(signalToastBody('Finished')).toBe('An agent finished its turn.');
  });

  it('applies the badge only when it changes, never below zero', () => {
    expect(badgeUpdate(3, null)).toBe(3);
    expect(badgeUpdate(3, 3)).toBeNull();
    expect(badgeUpdate(0, 3)).toBe(0);
    expect(badgeUpdate(0, null)).toBe(0);
    expect(badgeUpdate(-1, 0)).toBeNull();
  });
});
