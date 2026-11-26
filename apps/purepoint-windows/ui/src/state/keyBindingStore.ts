// Port of apps/purepoint-macos/purepoint-macos/State/KeyBindingState.swift (WIN-6.7).
import { create } from 'zustand';
import {
  HOTKEY_ACTIONS,
  type HotkeyAction,
  type KeyBinding,
  type KeyChordEvent,
  actionForEvent,
  bindingsEqual,
  chordId,
  decodeBinding,
  defaultBinding,
  encodeBinding,
  isHotkeyAction,
} from '../model/keyBinding.ts';

/** localStorage key for the user's overrides (per Windows user, via the WebView2 profile). */
export const KEY_BINDINGS_STORAGE_KEY = 'purepoint.keybindings.v1';

export type KeyBindingMap = Record<HotkeyAction, KeyBinding>;
export type KeyBindingOverrides = Partial<Record<HotkeyAction, KeyBinding>>;

export function defaultBindings(): KeyBindingMap {
  return Object.fromEntries(
    HOTKEY_ACTIONS.map((action) => [action, defaultBinding(action)]),
  ) as KeyBindingMap;
}

/**
 * The bindings in effect: every saved override wins, and an action still on a default that an
 * override has claimed (e.g. a newly added action whose default the user had already given to
 * another action) takes a default some override gave up instead — the same swap the Settings
 * "Override" button makes. Defaults are unique, so there is always a freed default for each
 * displaced action and no two actions share a binding.
 */
export function resolve(
  defaults: Partial<Record<HotkeyAction, KeyBinding>>,
  overrides: KeyBindingOverrides,
): Partial<Record<HotkeyAction, KeyBinding>> {
  const claimed = new Set(Object.values(overrides).map(chordId));
  const freed: KeyBinding[] = [];
  for (const action of HOTKEY_ACTIONS) {
    const binding = defaults[action];
    if (
      overrides[action] !== undefined &&
      binding !== undefined &&
      !claimed.has(chordId(binding))
    ) {
      freed.push(binding);
    }
  }

  const result: Partial<Record<HotkeyAction, KeyBinding>> = { ...overrides };
  for (const action of HOTKEY_ACTIONS) {
    if (overrides[action] !== undefined) continue;
    const binding = defaults[action];
    if (binding === undefined) continue;
    if (!claimed.has(chordId(binding))) {
      result[action] = binding;
    } else {
      const next = freed.shift();
      if (next !== undefined) result[action] = next;
    }
  }
  return result;
}

function resolveAll(overrides: KeyBindingOverrides): KeyBindingMap {
  const resolved = resolve(defaultBindings(), overrides);
  // `resolve` covers every action when the defaults do; fall back to the default as Swift does.
  return Object.fromEntries(
    HOTKEY_ACTIONS.map((action) => [action, resolved[action] ?? defaultBinding(action)]),
  ) as KeyBindingMap;
}

/** Saved overrides; unknown actions and malformed bindings are skipped, as in Swift. */
export function loadOverrides(storage: Storage | null): KeyBindingOverrides {
  const overrides: KeyBindingOverrides = {};
  let raw: string | null;
  try {
    raw = storage?.getItem(KEY_BINDINGS_STORAGE_KEY) ?? null;
  } catch {
    return overrides;
  }
  if (raw === null) return overrides;
  let saved: unknown;
  try {
    saved = JSON.parse(raw);
  } catch {
    return overrides;
  }
  if (typeof saved !== 'object' || saved === null || Array.isArray(saved)) return overrides;
  for (const [rawAction, value] of Object.entries(saved as Record<string, unknown>)) {
    const binding = decodeBinding(value);
    if (isHotkeyAction(rawAction) && binding !== null) overrides[rawAction] = binding;
  }
  return overrides;
}

function persist(storage: Storage | null, overrides: KeyBindingOverrides): void {
  const dict: Record<string, unknown> = {};
  for (const action of HOTKEY_ACTIONS) {
    const binding = overrides[action];
    if (binding !== undefined) dict[action] = encodeBinding(binding);
  }
  try {
    storage?.setItem(KEY_BINDINGS_STORAGE_KEY, JSON.stringify(dict));
  } catch {
    // Storage full or blocked: the bindings still apply for this session.
  }
}

export interface KeyBindingState {
  /** Current bindings (defaults merged with user overrides). */
  readonly bindings: KeyBindingMap;
  /** User overrides only (persisted). */
  readonly overrides: KeyBindingOverrides;
  /** Which action is currently being recorded (null = not recording). */
  recordingAction: HotkeyAction | null;

  binding(action: HotkeyAction): KeyBinding;
  isCustomized(action: HotkeyAction): boolean;
  /** Find an existing action that uses the same binding, excluding a given action. */
  detectConflict(binding: KeyBinding, excluding: HotkeyAction): HotkeyAction | null;
  /** Reverse lookup: the action a keydown triggers (see `isAppChord`). */
  actionFor(event: KeyChordEvent, focusInTerminal: boolean): HotkeyAction | null;

  setRecordingAction(action: HotkeyAction | null): void;
  setBinding(binding: KeyBinding, action: HotkeyAction): void;
  resetBinding(action: HotkeyAction): void;
  resetAllBindings(): void;
}

function browserStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/** A store backed by `storage` (localStorage by default; null keeps it in memory). */
export function createKeyBindingStore(storage: Storage | null = browserStorage()) {
  const initialOverrides = loadOverrides(storage);
  return create<KeyBindingState>()((set, get) => ({
    bindings: resolveAll(initialOverrides),
    overrides: initialOverrides,
    recordingAction: null,

    binding: (action) => get().bindings[action],
    isCustomized: (action) => get().overrides[action] !== undefined,
    detectConflict: (binding, excluding) => {
      const { bindings } = get();
      for (const action of HOTKEY_ACTIONS) {
        if (action !== excluding && bindingsEqual(bindings[action], binding)) return action;
      }
      return null;
    },
    actionFor: (event, focusInTerminal) => actionForEvent(event, focusInTerminal, get().bindings),

    setRecordingAction: (action) => {
      set({ recordingAction: action });
    },
    setBinding: (binding, action) => {
      const overrides = { ...get().overrides, [action]: binding };
      set({ overrides, bindings: resolveAll(overrides) });
      persist(storage, overrides);
    },
    resetBinding: (action) => {
      const overrides: KeyBindingOverrides = Object.fromEntries(
        Object.entries(get().overrides).filter(([key]) => key !== action),
      );
      set({ overrides, bindings: resolveAll(overrides) });
      persist(storage, overrides);
    },
    resetAllBindings: () => {
      set({ overrides: {}, bindings: defaultBindings() });
      persist(storage, {});
    },
  }));
}

export const useKeyBindingStore = createKeyBindingStore();
