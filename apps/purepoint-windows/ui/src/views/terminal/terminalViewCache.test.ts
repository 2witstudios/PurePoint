// TerminalViewCache semantics (WIN-7.3): hidden terminals stay alive and are evicted 30 s after
// hiding; showing one again cancels the eviction; a gone agent goes as soon as it is hidden.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { EVICTION_DELAY_MS, TerminalViewCache, type CachedTerminal } from './terminalViewCache.ts';

class FakeTerminal implements CachedTerminal {
  isAgentGone = false;
  disposed = 0;
  reconnects = 0;
  readonly agentId: string;
  constructor(agentId: string) {
    this.agentId = agentId;
  }
  reconnectIfNeeded(): void {
    this.reconnects++;
  }
  dispose(): void {
    this.disposed++;
  }
}

function makeCache() {
  const created: FakeTerminal[] = [];
  const cache = new TerminalViewCache((agentId) => {
    const view = new FakeTerminal(agentId);
    created.push(view);
    return view;
  });
  return { cache, created };
}

describe('TerminalViewCache', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('creates a terminal once and returns the same one while cached', () => {
    const { cache, created } = makeCache();
    const first = cache.acquire('ag-1');
    cache.release('ag-1');
    const again = cache.acquire('ag-1');
    expect(again).toBe(first);
    expect(created).toHaveLength(1);
    expect(again.reconnects).toBe(1);
  });

  it('keeps a hidden terminal alive until 30 s after it was hidden', () => {
    const { cache } = makeCache();
    const view = cache.acquire('ag-1');
    vi.advanceTimersByTime(10 * EVICTION_DELAY_MS);
    expect(cache.has('ag-1')).toBe(true); // shown: never evicted
    cache.release('ag-1');
    vi.advanceTimersByTime(EVICTION_DELAY_MS - 1);
    expect(cache.has('ag-1')).toBe(true);
    expect(view.disposed).toBe(0);
    vi.advanceTimersByTime(1);
    expect(cache.has('ag-1')).toBe(false);
    expect(view.disposed).toBe(1);
  });

  it('cancels the eviction when the terminal is shown again', () => {
    const { cache, created } = makeCache();
    cache.acquire('ag-1');
    cache.release('ag-1');
    vi.advanceTimersByTime(EVICTION_DELAY_MS - 1000);
    cache.acquire('ag-1');
    vi.advanceTimersByTime(5 * EVICTION_DELAY_MS);
    expect(cache.has('ag-1')).toBe(true);
    // Hidden again: the 30 s restart from now.
    cache.release('ag-1');
    vi.advanceTimersByTime(EVICTION_DELAY_MS - 1);
    expect(cache.has('ag-1')).toBe(true);
    vi.advanceTimersByTime(1);
    expect(cache.has('ag-1')).toBe(false);
    expect(created[0]?.disposed).toBe(1);
  });

  it('stays while any pane still shows it', () => {
    const { cache } = makeCache();
    cache.acquire('ag-1');
    cache.acquire('ag-1');
    cache.release('ag-1');
    vi.advanceTimersByTime(2 * EVICTION_DELAY_MS);
    expect(cache.isShown('ag-1')).toBe(true);
    expect(cache.has('ag-1')).toBe(true);
  });

  it('evicts each hidden terminal on its own clock', () => {
    const { cache } = makeCache();
    cache.acquire('ag-1');
    cache.acquire('ag-2');
    cache.release('ag-1');
    vi.advanceTimersByTime(20_000);
    cache.release('ag-2');
    vi.advanceTimersByTime(10_000);
    expect(cache.agentIds).toEqual(['ag-2']);
    vi.advanceTimersByTime(20_000);
    expect(cache.size).toBe(0);
  });

  it('evicts a gone agent as soon as it is hidden', () => {
    const { cache, created } = makeCache();
    cache.acquire('ag-1');
    const view = created[0];
    if (view === undefined) throw new Error('not created');
    view.isAgentGone = true;
    cache.agentGone('ag-1');
    expect(cache.has('ag-1')).toBe(true); // on screen
    cache.release('ag-1');
    expect(cache.has('ag-1')).toBe(false);
    expect(view.disposed).toBe(1);
  });

  it('evicts a hidden terminal immediately when its agent is reported gone', () => {
    const { cache, created } = makeCache();
    cache.acquire('ag-1');
    cache.release('ag-1');
    cache.agentGone('ag-1');
    expect(cache.has('ag-1')).toBe(false);
    expect(created[0]?.disposed).toBe(1);
    vi.advanceTimersByTime(EVICTION_DELAY_MS);
    expect(created[0]?.disposed).toBe(1);
  });

  it('tracks unseen output until the terminal is shown, notifying once', () => {
    const { cache } = makeCache();
    const listener = vi.fn();
    cache.subscribe(listener);
    cache.acquire('ag-1');
    cache.release('ag-1');
    cache.markUnseenOutput('ag-1');
    cache.markUnseenOutput('ag-1');
    expect(cache.hasUnseenOutput('ag-1')).toBe(true);
    expect(listener).toHaveBeenCalledTimes(1);
    cache.acquire('ag-1');
    expect(cache.hasUnseenOutput('ag-1')).toBe(false);
    expect(listener).toHaveBeenCalledTimes(2);
  });

  it('remove and clear tear terminals down now', () => {
    const { cache, created } = makeCache();
    cache.acquire('ag-1');
    cache.acquire('ag-2');
    cache.remove('ag-1');
    expect(created[0]?.disposed).toBe(1);
    cache.clear();
    expect(created[1]?.disposed).toBe(1);
    expect(cache.size).toBe(0);
  });
});
