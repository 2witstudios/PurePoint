// Port of apps/purepoint-macos/purepoint-macos/State/SidebarVisibility.swift (WIN-6.5).
//
// Where the window's two sidebars keep their shown/hidden state, so each survives a relaunch. A
// sidebar the user has never toggled is shown.

import { create } from 'zustand';
import type { HotkeyAction } from '../model/keyBinding.ts';
import { browserStorage } from './storage.ts';

/** The machines and projects outline on the left. */
export const SIDEBAR_KEY = 'purepoint.sidebar.shown.v1';
/** The workspace's Changes/Files/Point Guard panel on the right. */
export const WORKSPACE_SIDEBAR_KEY = 'purepoint.workspaceSidebar.shown.v1';

export type SidebarVisibilityKey = typeof SIDEBAR_KEY | typeof WORKSPACE_SIDEBAR_KEY;

export function isSidebarShown(key: SidebarVisibilityKey, storage: Storage | null): boolean {
  try {
    const raw = storage?.getItem(key) ?? null;
    return raw === null ? true : raw !== 'false';
  } catch {
    return true;
  }
}

export function toggleSidebarKey(key: SidebarVisibilityKey, storage: Storage | null): void {
  try {
    storage?.setItem(key, String(!isSidebarShown(key, storage)));
  } catch {
    // Storage full or blocked: nothing to remember.
  }
}

/** The sidebar a hotkey action shows or hides, if it is one of the two toggles. */
export function keyToggledBy(action: HotkeyAction): SidebarVisibilityKey | null {
  switch (action) {
    case 'toggleSidebar':
      return SIDEBAR_KEY;
    case 'toggleWorkspaceSidebar':
      return WORKSPACE_SIDEBAR_KEY;
    default:
      return null;
  }
}

export interface SidebarVisibilityState {
  readonly sidebarShown: boolean;
  readonly workspaceSidebarShown: boolean;
  isShown(key: SidebarVisibilityKey): boolean;
  toggle(key: SidebarVisibilityKey): void;
  /**
   * Toggles the sidebar `action` names and says whether it named one. Called once per command
   * rather than from each window: two windows each flipping the shared setting would cancel out.
   */
  toggleFor(action: HotkeyAction): boolean;
}

/** A store backed by `storage` (localStorage by default; null keeps it in memory). */
export function createSidebarVisibilityStore(storage: Storage | null = browserStorage()) {
  // With no storage the state lives only in the store.
  const read = (key: SidebarVisibilityKey, fallback: boolean) =>
    storage === null ? fallback : isSidebarShown(key, storage);
  return create<SidebarVisibilityState>()((set, get) => ({
    sidebarShown: read(SIDEBAR_KEY, true),
    workspaceSidebarShown: read(WORKSPACE_SIDEBAR_KEY, true),

    isShown: (key) => (key === SIDEBAR_KEY ? get().sidebarShown : get().workspaceSidebarShown),

    toggle: (key) => {
      const shown = !get().isShown(key);
      try {
        storage?.setItem(key, String(shown));
      } catch {
        // Storage full or blocked: the toggle still applies for this session.
      }
      set(key === SIDEBAR_KEY ? { sidebarShown: shown } : { workspaceSidebarShown: shown });
    },

    toggleFor: (action) => {
      const key = keyToggledBy(action);
      if (key === null) return false;
      get().toggle(key);
      return true;
    },
  }));
}

export const useSidebarVisibilityStore = createSidebarVisibilityStore();
