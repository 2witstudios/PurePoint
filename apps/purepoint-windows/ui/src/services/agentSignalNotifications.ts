// Port of apps/purepoint-macos/purepoint-macos/Services/AgentSignalNotifications.swift (WIN-7.7,
// REQ-WINAPP-078).
//
// Local delivery only: it never sends input, resolves requests or owns agent sessions. Each update
// projects the agents through `AgentSignalAlertPolicy`, shows the badge count as the taskbar
// overlay (`taskbar_set_badge`, in place of the dock tile) and posts a toast through
// `tauri-plugin-notification` for each new alert while the window is unfocused.
//
// Differences from Swift:
// - Toast activation. The notification plugin delivers no click callback on Windows desktop (its
//   `onAction` is mobile-only, and an unpackaged build's toasts carry PowerShell's AppUserModelID,
//   so a click cannot reach this process). A toast's route is therefore taken when the window next
//   gains focus: the newest toast posted while the window was unfocused, if its agent still has
//   the same event, becomes the pending tap — so clicking the toast and then the taskbar button,
//   or switching back with Alt+Tab, lands on the agent's pane.
// - The release qualification gate (`PP_signalQualifiedMacAdapters`, empty until an owner
//   qualifies a tool/adapter pair) is not ported: a Windows toast only opens the pane and never
//   answers the request, which is what that gate protects. Attention alerts (permission,
//   question, error) are on by default, finished turns off, per kind under
//   `PP_signal_<kind>Notifications` until Settings (WIN-8.11) exposes them. The Mac's per-kind
//   sound setting is not ported: the plugin plays Windows' default toast sound.
// - Posted toasts cannot be withdrawn on Windows desktop (the plugin's removal API is mobile-only);
//   an obsolete toast's route is dropped instead, so focusing the window no longer follows it.
// - The iOS-only app badge permission rules (`AgentSignalBadge`) have no Windows counterpart: the
//   overlay needs no permission. `badgeUpdate` holds the rules Windows keeps.

import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import {
  AgentSignalAlertPolicy,
  type SignalAlertAgent,
  signalEventKey,
} from '../model/agentSignalPolicy.ts';
import { AgentSignalPendingTaps } from '../model/agentSignalPendingTaps.ts';

const DELIVERED_KEY = 'PP_signalDeliveredEvents';
const ACKNOWLEDGED_KEY = 'PP_signalAcknowledgedErrors';
const MAX_ACKNOWLEDGED = 4096;

/** Key/value storage (Swift `UserDefaults`). */
export interface SignalStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** What the notifications post through; tests pass fakes. */
export interface SignalNotificationHost {
  readonly storage: SignalStorage | null;
  postToast(toast: { readonly title: string; readonly body: string }): Promise<void>;
  setBadge(count: number): Promise<void>;
}

function defaultStorage(): SignalStorage | null {
  try {
    return globalThis.localStorage;
  } catch {
    return null;
  }
}

export const tauriSignalHost: SignalNotificationHost = {
  storage: defaultStorage(),
  // The plugin's command, invoked directly: its guest `sendNotification` goes through
  // `window.Notification` and swallows failures.
  postToast: (toast) =>
    invoke('plugin:notification|notify', { options: { title: toast.title, body: toast.body } }),
  setBadge: (count) => invoke('taskbar_set_badge', { count }),
};

function readList(storage: SignalStorage | null, key: string): string[] {
  try {
    const raw = storage?.getItem(key) ?? null;
    if (raw === null) return [];
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed)
      ? parsed.filter((item): item is string => typeof item === 'string')
      : [];
  } catch {
    return [];
  }
}

function writeList(storage: SignalStorage | null, key: string, list: readonly string[]): void {
  try {
    storage?.setItem(key, JSON.stringify(list));
  } catch {
    // Not worth failing over.
  }
}

function listsEqual(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((item, index) => item === b[index]);
}

/** The settings key part for an alert's kind (Swift: `Finished` is `completion`). */
export function signalAlertKind(label: string): string {
  return label === 'Finished' ? 'completion' : label.toLowerCase();
}

/** Whether toasts are on for a kind: attention kinds default on, finished turns off. */
export function signalNotificationsEnabled(storage: SignalStorage | null, kind: string): boolean {
  let raw: string | null;
  try {
    raw = storage?.getItem(`PP_signal_${kind}Notifications`) ?? null;
  } catch {
    raw = null;
  }
  if (raw === 'true') return true;
  if (raw === 'false') return false;
  return kind !== 'completion';
}

/** The toast's text: no prompt or request content, as the Mac's. */
export function signalToastBody(label: string): string {
  return label === 'Finished' ? 'An agent finished its turn.' : 'An agent needs attention.';
}

/**
 * The badge to apply: null when the overlay already shows `count` (Windows' badge rules, which
 * replace the iOS-only `AgentSignalBadge`: no permission, applied on change, 0 clears).
 */
export function badgeUpdate(count: number, applied: number | null): number | null {
  const next = Math.max(0, Math.floor(count));
  return next === applied ? null : next;
}

export class AgentSignalNotifications {
  /** Opens the agent's pane; resolves whether it did. Set by AppState. */
  open: ((agent: SignalAlertAgent) => Promise<boolean>) | null = null;

  private readonly host: SignalNotificationHost;
  private readonly pendingTaps = new AgentSignalPendingTaps();
  private draining = false;
  private acknowledgedErrors: string[];
  private readonly policy: AgentSignalAlertPolicy;
  private routes = new Map<string, SignalAlertAgent>();
  /** Keys toasted while the window was unfocused, oldest first; taken on the next focus. */
  private toastedSinceBlur: string[] = [];
  private windowFocused = true;
  private appliedBadge: number | null = null;
  private badgeTask: Promise<void> = Promise.resolve();

  constructor(host: SignalNotificationHost = tauriSignalHost) {
    this.host = host;
    this.policy = new AgentSignalAlertPolicy(readList(host.storage, DELIVERED_KEY));
    this.acknowledgedErrors = readList(host.storage, ACKNOWLEDGED_KEY).slice(-MAX_ACKNOWLEDGED);
  }

  get badge(): number | null {
    return this.appliedBadge;
  }

  /**
   * Project the agents: badge, acknowledged errors, routes and toasts. `foregroundIdentity` is
   * the agent visible in the focused window's focused pane, if any.
   */
  update(agents: readonly SignalAlertAgent[], foregroundIdentity: string | null): void {
    for (const agent of agents) {
      if (agent.identity !== foregroundIdentity || agent.presentation.label !== 'Error') continue;
      const key = signalEventKey(agent);
      if (key !== null && !this.acknowledgedErrors.includes(key)) {
        this.acknowledgedErrors = [...this.acknowledgedErrors, key].slice(-MAX_ACKNOWLEDGED);
        writeList(this.host.storage, ACKNOWLEDGED_KEY, this.acknowledgedErrors);
      }
    }
    const storedDelivered = readList(this.host.storage, DELIVERED_KEY);
    const result = this.policy.project(
      agents,
      (agent) => agent.presentation.needsAttention,
      new Set(this.acknowledgedErrors),
    );
    if (!listsEqual(storedDelivered, this.policy.deliveredKeys)) {
      writeList(this.host.storage, DELIVERED_KEY, this.policy.deliveredKeys);
    }
    this.applyBadge(result.badge);

    const routes = new Map<string, SignalAlertAgent>();
    for (const agent of agents) {
      const key = signalEventKey(agent);
      if (key !== null) routes.set(key, agent);
    }
    this.routes = routes;
    // An obsolete toast no longer leads anywhere.
    this.toastedSinceBlur = this.toastedSinceBlur.filter((key) => routes.has(key));
    this.drainPendingTaps();

    for (const agent of result.alerts) {
      const key = signalEventKey(agent);
      if (key === null) continue;
      const label = agent.presentation.label;
      if (
        agent.destination === null ||
        !signalNotificationsEnabled(this.host.storage, signalAlertKind(label)) ||
        agent.identity === foregroundIdentity ||
        // REQ-WINAPP-078: toasts only while the window is unfocused; the sidebar shows the rest.
        this.windowFocused
      ) {
        continue;
      }
      this.toastedSinceBlur.push(key);
      this.host
        .postToast({ title: 'PurePoint', body: signalToastBody(label) })
        .catch((error: unknown) => {
          console.error('PurePoint: could not post a notification', error);
        });
    }
  }

  /**
   * The window's focus changed. Gaining it takes the newest toast posted while it was away as a
   * tap (see the header: Windows delivers no toast click to the app).
   */
  windowFocusChanged(focused: boolean): void {
    if (focused === this.windowFocused) return;
    this.windowFocused = focused;
    if (!focused) {
      this.toastedSinceBlur = [];
      return;
    }
    const key = this.toastedSinceBlur.at(-1);
    this.toastedSinceBlur = [];
    if (key !== undefined) this.tap(key);
  }

  /** A toast for `key` was activated (Swift `userNotificationCenter(_:didReceive:)`). */
  tap(key: string): void {
    this.pendingTaps.enqueue(key);
    this.drainPendingTaps();
  }

  private applyBadge(count: number): void {
    const next = badgeUpdate(count, this.appliedBadge);
    if (next === null) return;
    this.appliedBadge = next;
    this.badgeTask = this.badgeTask.then(() =>
      this.host.setBadge(next).catch((error: unknown) => {
        // Try again on the next update.
        if (this.appliedBadge === next) this.appliedBadge = null;
        console.error('PurePoint: could not set the taskbar badge', error);
      }),
    );
  }

  private drainPendingTaps(): void {
    const open = this.open;
    if (this.draining || open === null) return;
    this.draining = true;
    void (async () => {
      try {
        for (;;) {
          const key = this.pendingTaps.next(new Set(this.routes.keys()));
          const route = key === null ? undefined : this.routes.get(key);
          if (key === null || route === undefined) return;
          if (!(await open(route))) return;
          this.pendingTaps.complete(key);
        }
      } finally {
        this.draining = false;
      }
    })();
  }
}

/** What `startAgentSignals` drives: AppState's signal entry points. */
export interface AgentSignalsTarget {
  updateAgentSignals(): void;
  setWindowFocused(focused: boolean): void;
}

/** How often signals are projected: freshness advances even when no status arrives. */
export const SIGNAL_TICK_MS = 1000;

/**
 * Follow the main window's focus and project the signals every second (Swift ContentView's
 * `updateAgentSignals` loop). Returns the function that stops both.
 */
export function startAgentSignals(app: AgentSignalsTarget): () => void {
  let stopped = false;
  let unlisten: (() => void) | null = null;
  app.setWindowFocused(typeof document === 'undefined' || document.hasFocus());
  getCurrentWindow()
    .onFocusChanged(({ payload }) => {
      app.setWindowFocused(payload);
    })
    .then(
      (stop) => {
        if (stopped) stop();
        else unlisten = stop;
      },
      (error: unknown) => {
        console.error('PurePoint: listening for window focus failed', error);
      },
    );
  app.updateAgentSignals();
  const timer = setInterval(() => {
    app.updateAgentSignals();
  }, SIGNAL_TICK_MS);
  return () => {
    stopped = true;
    clearInterval(timer);
    unlisten?.();
  };
}
