// Configurable terminal font size (REQ-WINAPP-070): 13 px by default like `TerminalTheme.font`,
// persisted per user, applied live to every open terminal. Settings (WIN-8.11) edits it.
import { createStore } from 'zustand/vanilla';
import { browserStorage } from '../../state/storage.ts';
import { TERMINAL_DEFAULT_FONT_SIZE } from './terminalTheme.ts';

export const TERMINAL_FONT_SIZE_KEY = 'purepoint.terminalFontSize';
export const TERMINAL_FONT_SIZE_MIN = 8;
export const TERMINAL_FONT_SIZE_MAX = 32;

export interface TerminalFontState {
  readonly fontSize: number;
  setFontSize(size: number): void;
}

export function clampFontSize(size: number): number {
  if (!Number.isFinite(size)) return TERMINAL_DEFAULT_FONT_SIZE;
  return Math.min(TERMINAL_FONT_SIZE_MAX, Math.max(TERMINAL_FONT_SIZE_MIN, Math.round(size)));
}

export function createTerminalFontStore(storage: Storage | null = browserStorage()) {
  let initial = TERMINAL_DEFAULT_FONT_SIZE;
  try {
    const saved = storage?.getItem(TERMINAL_FONT_SIZE_KEY);
    if (saved != null) initial = clampFontSize(Number(saved));
  } catch {
    // Unreadable storage: the default.
  }
  return createStore<TerminalFontState>()((set) => ({
    fontSize: initial,
    setFontSize(size) {
      const fontSize = clampFontSize(size);
      try {
        storage?.setItem(TERMINAL_FONT_SIZE_KEY, String(fontSize));
      } catch {
        // Kept in memory only.
      }
      set({ fontSize });
    },
  }));
}

export const terminalFontStore = createTerminalFontStore();
