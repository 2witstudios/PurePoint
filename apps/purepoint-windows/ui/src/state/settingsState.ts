// Port of apps/purepoint-macos/purepoint-macos/State/SettingsState.swift (WIN-8.11), plus the
// `@AppStorage` agent notification toggles of Views/Settings/SettingsGeneralView.swift.
//
// One place for every preference Settings edits. The Mac keeps them all on `SettingsState`; here
// the ones other stores already own stay there and this state reads and writes through them, so
// what applies them keeps working unchanged:
// - appearance → `useThemeStore` (theme/themeStore.ts, applied live to <html data-theme>);
// - terminal font size → `terminalFontStore` (views/terminal/terminalFontStore.ts, applied live
//   to every open terminal);
// - hotkeys → `useKeyBindingStore` (state/keyBindingStore.ts), edited by the Hotkeys pane;
// - agent notifications → the `PP_signal_<kind>Notifications` keys that
//   services/agentSignalNotifications.ts reads on every alert, so a change applies to the next one.
// The rest (restore projects, launch at login, Point Guard names, palette order, grid gap) live in
// this store under the Mac's `PP_` keys. The command palette reads `usePointGuardNames` and
// `commandPaletteOrder` from here (`PP_usePointGuardNames`, `PP_commandPaletteOrder`, the keys
// state/commandPalettePreferences.ts uses too).
//
// Differences from Swift:
// - The Mac stores `restoreProjectsOnLaunch`, `launchAtLogin` and `gridGap` without anything
//   reading them; they are kept and persisted the same way here.
// - Notification defaults are Windows' (attention kinds on, finished turns off; WIN-7.7), and
//   the per-kind sound toggles are not ported: the toast plays Windows' default sound.
import { useStore } from 'zustand';
import { createStore } from 'zustand/vanilla';
import { type CommandPaletteItem, moveCommandPaletteItem } from '../model/commandPalette.ts';
import { signalNotificationsEnabled } from '../services/agentSignalNotifications.ts';
import { type ThemePreference, useThemeStore } from '../theme/themeStore.ts';
import { terminalFontStore } from '../views/terminal/terminalFontStore.ts';
import { browserStorage } from './storage.ts';
import {
  COMMAND_PALETTE_ORDER_KEY,
  USE_POINT_GUARD_NAMES_KEY,
} from './commandPalettePreferences.ts';

/** Swift `AppAppearance`, in its `allCases` order. */
export const APP_APPEARANCES: readonly { value: ThemePreference; label: string }[] = [
  { value: 'system', label: 'System' },
  { value: 'dark', label: 'Dark' },
  { value: 'light', label: 'Light' },
];

/** The agent signal kinds with a notification toggle, in the Mac's order. */
export const SIGNAL_KINDS = ['permission', 'question', 'error', 'completion'] as const;
export type SignalKind = (typeof SIGNAL_KINDS)[number];

export const SIGNAL_KIND_LABELS: Record<SignalKind, string> = {
  permission: 'Permission',
  question: 'Question',
  error: 'Error',
  completion: 'Completion',
};

export const signalNotificationsKey = (kind: SignalKind): string =>
  `PP_signal_${kind}Notifications`;

export const RESTORE_PROJECTS_KEY = 'PP_restoreProjectsOnLaunch';
export const LAUNCH_AT_LOGIN_KEY = 'PP_launchAtLogin';
export const GRID_GAP_KEY = 'PP_gridGap';

/** Swift's Display slider ranges. */
export const TERMINAL_FONT_SLIDER = { min: 10, max: 24 } as const;
export const GRID_GAP_SLIDER = { min: 0, max: 8 } as const;

export interface SettingsFields {
  readonly restoreProjectsOnLaunch: boolean;
  readonly launchAtLogin: boolean;
  readonly usePointGuardNames: boolean;
  readonly commandPaletteOrder: readonly string[];
  readonly signalNotifications: Readonly<Record<SignalKind, boolean>>;
  readonly gridGap: number;
}

export interface SettingsActions {
  setRestoreProjectsOnLaunch: (value: boolean) => void;
  setLaunchAtLogin: (value: boolean) => void;
  setUsePointGuardNames: (value: boolean) => void;
  setCommandPaletteOrder: (order: readonly string[]) => void;
  /** Swap visible neighbours while retaining preferences for entries unavailable here. */
  moveCommandPaletteItem: (
    id: string,
    offset: number,
    items: readonly CommandPaletteItem[],
  ) => void;
  setSignalNotifications: (kind: SignalKind, enabled: boolean) => void;
  setGridGap: (value: number) => void;
  setAppearance: (value: ThemePreference) => void;
  setTerminalFontSize: (value: number) => void;
}

export type SettingsStoreState = SettingsFields & SettingsActions;

type PreferenceStorage = Pick<Storage, 'getItem' | 'setItem'>;

function read(storage: PreferenceStorage | null, key: string): string | null {
  try {
    return storage?.getItem(key) ?? null;
  } catch {
    return null;
  }
}

function write(storage: PreferenceStorage | null, key: string, value: string): void {
  try {
    storage?.setItem(key, value);
  } catch {
    // Storage full or blocked: the value still applies for this session.
  }
}

function readBool(storage: PreferenceStorage | null, key: string, fallback: boolean): boolean {
  const raw = read(storage, key);
  if (raw === 'true') return true;
  if (raw === 'false') return false;
  return fallback;
}

function readOrder(raw: string | null): string[] {
  if (raw === null) return [];
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : [];
  } catch {
    return [];
  }
}

/** Swift: a negative grid gap resets to 1. */
export function validGridGap(value: number): number {
  if (!Number.isFinite(value) || value < 0) return 1;
  return Math.min(GRID_GAP_SLIDER.max, Math.round(value));
}

/** The saved values, with the Mac's defaults and validation. */
export function loadSettings(storage: PreferenceStorage | null): SettingsFields {
  const gap = read(storage, GRID_GAP_KEY);
  return {
    restoreProjectsOnLaunch: readBool(storage, RESTORE_PROJECTS_KEY, true),
    launchAtLogin: readBool(storage, LAUNCH_AT_LOGIN_KEY, false),
    usePointGuardNames: readBool(storage, USE_POINT_GUARD_NAMES_KEY, true),
    commandPaletteOrder: readOrder(read(storage, COMMAND_PALETTE_ORDER_KEY)),
    signalNotifications: Object.fromEntries(
      SIGNAL_KINDS.map((kind) => [kind, signalNotificationsEnabled(storage, kind)]),
    ) as Record<SignalKind, boolean>,
    gridGap: gap === null ? 1 : validGridGap(Number(gap)),
  };
}

export interface SettingsStoreOptions {
  storage?: PreferenceStorage | null;
  setAppearance?: (value: ThemePreference) => void;
  setTerminalFontSize?: (value: number) => void;
}

/** A settings store backed by `storage` (localStorage by default; null keeps it in memory). */
export function createSettingsStore(options: SettingsStoreOptions = {}) {
  const storage = options.storage === undefined ? browserStorage() : options.storage;
  const setAppearance =
    options.setAppearance ??
    ((value: ThemePreference) => {
      useThemeStore.getState().setPreference(value);
    });
  const setTerminalFontSize =
    options.setTerminalFontSize ??
    ((value: number) => {
      terminalFontStore.getState().setFontSize(value);
    });
  return createStore<SettingsStoreState>()((set, get) => ({
    ...loadSettings(storage),

    setRestoreProjectsOnLaunch(value) {
      write(storage, RESTORE_PROJECTS_KEY, String(value));
      set({ restoreProjectsOnLaunch: value });
    },
    setLaunchAtLogin(value) {
      write(storage, LAUNCH_AT_LOGIN_KEY, String(value));
      set({ launchAtLogin: value });
    },
    setUsePointGuardNames(value) {
      write(storage, USE_POINT_GUARD_NAMES_KEY, String(value));
      set({ usePointGuardNames: value });
    },
    setCommandPaletteOrder(order) {
      const next = [...order];
      write(storage, COMMAND_PALETTE_ORDER_KEY, JSON.stringify(next));
      set({ commandPaletteOrder: next });
    },
    moveCommandPaletteItem(id, offset, items) {
      const current = get().commandPaletteOrder;
      const next = moveCommandPaletteItem(current, id, offset, items);
      if (next !== current) get().setCommandPaletteOrder(next);
    },
    setSignalNotifications(kind, enabled) {
      write(storage, signalNotificationsKey(kind), String(enabled));
      set({ signalNotifications: { ...get().signalNotifications, [kind]: enabled } });
    },
    setGridGap(value) {
      const gridGap = validGridGap(value);
      write(storage, GRID_GAP_KEY, String(gridGap));
      set({ gridGap });
    },
    setAppearance,
    setTerminalFontSize,
  }));
}

export type SettingsStore = ReturnType<typeof createSettingsStore>;

/** The app's settings (Swift `@Environment(SettingsState.self)`). */
export const settingsStore = createSettingsStore();

export function useSettings<T>(selector: (state: SettingsStoreState) => T): T {
  return useStore(settingsStore, selector);
}
