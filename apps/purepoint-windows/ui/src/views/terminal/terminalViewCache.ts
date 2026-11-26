// Port of apps/purepoint-macos/purepoint-macos/Views/Terminal/TerminalViewCache.swift (WIN-7.3).
//
// Terminals by agent ID, shown and hidden rather than created and destroyed when panes switch.
// A hidden terminal keeps its xterm.js instance and its attach session, parked in a hidden
// element, and is evicted 30 s after it was hidden (REQ-WINAPP-061); showing it again before
// then cancels the eviction. An agent the daemon reports gone is evicted as soon as it is hidden.
//
// Differences from Swift:
// - Eviction is a timer per hidden terminal, not a 20 s sweep, and applies to live agents too:
//   REQ-WINAPP-061 bounds hidden xterm.js instances (each holds a renderer and 10k lines of
//   scrollback). The agent keeps running; showing it again reattaches and the daemon replays.
// - Visibility is counted by `acquire`/`release` (React mounts), not read from the view tree.

/** What the cache keeps (TerminalPaneController; fakes in tests). */
export interface CachedTerminal {
  readonly isAgentGone: boolean;
  reconnectIfNeeded?(): void;
  dispose(): void;
}

/** `evictionDelay`. */
export const EVICTION_DELAY_MS = 30_000;

interface Entry<T> {
  readonly view: T;
  shown: number;
  evictTimer: ReturnType<typeof setTimeout> | null;
}

export interface TerminalViewCacheOptions {
  evictionDelayMs?: number;
}

export class TerminalViewCache<T extends CachedTerminal> {
  private readonly views = new Map<string, Entry<T>>();
  private readonly create: (agentId: string) => T;
  private readonly evictionDelayMs: number;
  /** Agents whose terminal printed output while hidden, since last shown (the tab dot). */
  private readonly unseen = new Set<string>();
  private readonly listeners = new Set<() => void>();

  constructor(create: (agentId: string) => T, options: TerminalViewCacheOptions = {}) {
    this.create = create;
    this.evictionDelayMs = options.evictionDelayMs ?? EVICTION_DELAY_MS;
  }

  /** The agent's terminal, created on first use, now counted as on screen (`terminalView(for:)` + `show`). */
  acquire(agentId: string): T {
    let entry = this.views.get(agentId);
    if (entry === undefined) {
      entry = { view: this.create(agentId), shown: 0, evictTimer: null };
      this.views.set(agentId, entry);
    } else {
      entry.view.reconnectIfNeeded?.();
    }
    entry.shown++;
    this.cancelEviction(entry);
    if (this.unseen.delete(agentId)) this.notify();
    return entry.view;
  }

  /** A pane stopped showing the agent's terminal; evict it once hidden for the delay. */
  release(agentId: string): void {
    const entry = this.views.get(agentId);
    if (entry === undefined) return;
    entry.shown = Math.max(0, entry.shown - 1);
    if (entry.shown > 0) return;
    if (entry.view.isAgentGone) {
      this.remove(agentId);
      return;
    }
    this.cancelEviction(entry);
    entry.evictTimer = setTimeout(() => {
      entry.evictTimer = null;
      if (this.views.get(agentId) === entry && entry.shown === 0) this.remove(agentId);
    }, this.evictionDelayMs);
  }

  /** The daemon reported the agent gone: evict now unless it is on screen (then on release). */
  agentGone(agentId: string): void {
    const entry = this.views.get(agentId);
    if (entry !== undefined && entry.shown === 0) this.remove(agentId);
  }

  get(agentId: string): T | null {
    return this.views.get(agentId)?.view ?? null;
  }

  /** `hasView(for:)`. */
  has(agentId: string): boolean {
    return this.views.has(agentId);
  }

  isShown(agentId: string): boolean {
    return (this.views.get(agentId)?.shown ?? 0) > 0;
  }

  get agentIds(): string[] {
    return [...this.views.keys()];
  }

  get size(): number {
    return this.views.size;
  }

  /** Writes only on the first chunk since last shown, so busy agents do not churn observers. */
  markUnseenOutput(agentId: string): void {
    if (this.unseen.has(agentId)) return;
    this.unseen.add(agentId);
    this.notify();
  }

  hasUnseenOutput(agentId: string): boolean {
    return this.unseen.has(agentId);
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  }

  /** Remove an agent's terminal now (`remove(agentId:)`). */
  remove(agentId: string): void {
    const entry = this.views.get(agentId);
    if (entry === undefined) return;
    this.cancelEviction(entry);
    this.views.delete(agentId);
    entry.view.dispose();
    if (this.unseen.delete(agentId)) this.notify();
  }

  /** Tear every terminal down (`deinit`). */
  clear(): void {
    for (const agentId of [...this.views.keys()]) this.remove(agentId);
  }

  forEach(visit: (view: T, agentId: string) => void): void {
    for (const [agentId, entry] of this.views) visit(entry.view, agentId);
  }

  private cancelEviction(entry: Entry<T>): void {
    if (entry.evictTimer !== null) clearTimeout(entry.evictTimer);
    entry.evictTimer = null;
  }

  private notify(): void {
    for (const listener of this.listeners) listener();
  }
}
