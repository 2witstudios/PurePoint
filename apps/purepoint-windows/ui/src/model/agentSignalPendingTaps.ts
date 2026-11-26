// Port of apps/purepoint-macos/purepoint-macos/Models/AgentSignalPendingTaps.swift (WIN-7.7).
//
// A notification tap may arrive before the projects and their status are restored. Keep only a
// bounded, short-lived intent: at most 16 keys, each for two minutes, dropped when the clock
// runs backwards.

const MAX_PENDING = 16;
const MAX_KEY_BYTES = 8192;
const LIFETIME_MS = 120_000;

const utf8 = new TextEncoder();

export class AgentSignalPendingTaps {
  private pending: { key: string; receivedMs: number }[] = [];

  enqueue(key: string, nowMs: number = Date.now()): void {
    if (key === '' || utf8.encode(key).length > MAX_KEY_BYTES) return;
    this.prune(nowMs);
    if (this.pending.some((entry) => entry.key === key)) return;
    this.pending.push({ key, receivedMs: nowMs });
    if (this.pending.length > MAX_PENDING) {
      this.pending.splice(0, this.pending.length - MAX_PENDING);
    }
  }

  /** The oldest pending key whose route is available; it stays pending until `complete`. */
  next(availableKeys: ReadonlySet<string>, nowMs: number = Date.now()): string | null {
    this.prune(nowMs);
    return this.pending.find((entry) => availableKeys.has(entry.key))?.key ?? null;
  }

  complete(key: string): void {
    this.pending = this.pending.filter((entry) => entry.key !== key);
  }

  private prune(nowMs: number): void {
    this.pending = this.pending.filter(
      (entry) => nowMs >= entry.receivedMs && nowMs - entry.receivedMs < LIFETIME_MS,
    );
  }
}
