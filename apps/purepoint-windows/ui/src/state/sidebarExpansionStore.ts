// Port of apps/purepoint-macos/purepoint-macos/State/SidebarExpansionStore.swift (WIN-6.5).
//
// Which sidebar rows the user closed. The tree is rebuilt on every data refresh, so what is open
// cannot live in it — a rebuild would forget it and every row would snap back open. Keys are
// `SidebarNode.rowKey`, which is parent-scoped, so two rows sharing a bare id never stand in for
// each other. Only closed rows are kept: anything else is open, which is what a fresh launch
// shows, so this is the smaller set and it stays stable as rows come and go.

import { create } from 'zustand';
import { browserStorage } from './storage.ts';

/** localStorage key for the closed rows (a sorted JSON string array). */
export const SIDEBAR_EXPANSION_STORAGE_KEY = 'purepoint.sidebarExpansion.v1';

export interface SidebarExpansionState {
  /** Rows the user closed. */
  readonly closedRowKeys: ReadonlySet<string>;
  /** Records one row the user opened or closed. */
  setExpanded(rowKey: string, isExpanded: boolean): void;
  /**
   * Records the rows a tree currently shows closed, dropping keys that no longer exist so a
   * removed row cannot resurrect itself when it comes back.
   */
  record(closedRowKeys: ReadonlySet<string>): void;
}

export function loadClosedRowKeys(storage: Storage | null): Set<string> {
  try {
    const raw = storage?.getItem(SIDEBAR_EXPANSION_STORAGE_KEY) ?? null;
    if (raw === null) return new Set();
    const saved: unknown = JSON.parse(raw);
    if (!Array.isArray(saved)) return new Set();
    return new Set(saved.filter((key): key is string => typeof key === 'string'));
  } catch {
    return new Set();
  }
}

function sameKeys(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  return a.size === b.size && [...a].every((key) => b.has(key));
}

/** A store backed by `storage` (localStorage by default; null keeps it in memory). */
export function createSidebarExpansionStore(storage: Storage | null = browserStorage()) {
  return create<SidebarExpansionState>()((set, get) => ({
    closedRowKeys: loadClosedRowKeys(storage),

    setExpanded: (rowKey, isExpanded) => {
      const keys = new Set(get().closedRowKeys);
      if (isExpanded) keys.delete(rowKey);
      else keys.add(rowKey);
      get().record(keys);
    },

    record: (closedRowKeys) => {
      if (sameKeys(closedRowKeys, get().closedRowKeys)) return;
      const keys = new Set(closedRowKeys);
      set({ closedRowKeys: keys });
      try {
        storage?.setItem(SIDEBAR_EXPANSION_STORAGE_KEY, JSON.stringify([...keys].sort()));
      } catch {
        // Storage full or blocked: the rows stay as they are for this session.
      }
    },
  }));
}

export const useSidebarExpansionStore = createSidebarExpansionStore();
