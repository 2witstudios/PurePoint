// The left sidebar's width (WIN-7.1). The Mac's sidebar column is fixed at the ideal width; on
// Windows it is resizable between the theme's min and max widths (PurePointTheme.sidebarMinWidth,
// sidebarMaxWidth) and the width survives a relaunch.
import { create } from 'zustand';
import { browserStorage } from '../../state/storage.ts';

export const SIDEBAR_WIDTH_KEY = 'purepoint.sidebar.width.v1';
/** PurePointTheme.sidebarMinWidth / sidebarIdealWidth / sidebarMaxWidth (tokens.css). */
export const SIDEBAR_MIN_WIDTH = 200;
export const SIDEBAR_IDEAL_WIDTH = 240;
export const SIDEBAR_MAX_WIDTH = 320;

export function clampSidebarWidth(width: number): number {
  if (!Number.isFinite(width)) return SIDEBAR_IDEAL_WIDTH;
  return Math.round(Math.min(SIDEBAR_MAX_WIDTH, Math.max(SIDEBAR_MIN_WIDTH, width)));
}

export function loadSidebarWidth(storage: Storage | null): number {
  try {
    const raw = storage?.getItem(SIDEBAR_WIDTH_KEY) ?? null;
    return raw === null ? SIDEBAR_IDEAL_WIDTH : clampSidebarWidth(Number(raw));
  } catch {
    return SIDEBAR_IDEAL_WIDTH;
  }
}

export interface SidebarWidthState {
  readonly width: number;
  /** Live width while dragging; not saved. */
  preview(width: number): void;
  /** Sets and saves the width. */
  commit(width: number): void;
}

/** A store backed by `storage` (localStorage by default; null keeps it in memory). */
export function createSidebarWidthStore(storage: Storage | null = browserStorage()) {
  return create<SidebarWidthState>()((set) => ({
    width: loadSidebarWidth(storage),
    preview: (width) => {
      set({ width: clampSidebarWidth(width) });
    },
    commit: (width) => {
      const clamped = clampSidebarWidth(width);
      try {
        storage?.setItem(SIDEBAR_WIDTH_KEY, String(clamped));
      } catch {
        // Storage full or blocked: the width still applies for this session.
      }
      set({ width: clamped });
    },
  }));
}

export const useSidebarWidthStore = createSidebarWidthStore();
