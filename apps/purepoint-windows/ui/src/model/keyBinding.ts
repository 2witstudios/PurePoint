// Port of apps/purepoint-macos/purepoint-macos/Models/KeyBinding.swift (WIN-6.7).
//
// Chords match on `KeyboardEvent.code` (the physical key), so keyboard layouts and AltGr never
// change which chord a key press is. Defaults follow the DEC-8 Mac-to-Windows table: no default
// uses the Win key, and none uses Ctrl+Alt together (that is AltGr on Windows).

// MARK: - HotkeyCategory

export const HOTKEY_CATEGORIES = ['application', 'navigation', 'panes', 'tabs', 'chat'] as const;
export type HotkeyCategory = (typeof HOTKEY_CATEGORIES)[number];

export const CATEGORY_DISPLAY_NAMES: Record<HotkeyCategory, string> = {
  application: 'Application',
  navigation: 'Navigation',
  panes: 'Panes',
  tabs: 'Tabs',
  chat: 'Chat',
};

// MARK: - HotkeyAction

/** Every action, in the Swift `HotkeyAction.allCases` order (resolution depends on it). */
export const HOTKEY_ACTIONS = [
  // Application
  'newAgent',
  'openProject',
  'settings',
  'closeAgent',
  // Navigation
  'focusSidebar',
  'focusContent',
  'toggleSidebar',
  'toggleWorkspaceSidebar',
  'navDashboard',
  'navAgents',
  'navSchedule',
  // Panes
  'splitBelow',
  'splitRight',
  'closePane',
  'focusUp',
  'focusDown',
  'focusLeft',
  'focusRight',
  // Tabs
  'newTab',
  'nextTab',
  'previousTab',
  'selectTab1',
  'selectTab2',
  'selectTab3',
  'selectTab4',
  'selectTab5',
  'selectTab6',
  'selectTab7',
  'selectTab8',
  'selectLastTab',
  'breakTab',
  // Chat
  'toggleChatSidebar',
] as const;
export type HotkeyAction = (typeof HOTKEY_ACTIONS)[number];

export function isHotkeyAction(value: string): value is HotkeyAction {
  return (HOTKEY_ACTIONS as readonly string[]).includes(value);
}

export const SELECT_TAB_ACTIONS: readonly HotkeyAction[] = [
  'selectTab1',
  'selectTab2',
  'selectTab3',
  'selectTab4',
  'selectTab5',
  'selectTab6',
  'selectTab7',
  'selectTab8',
];

/** The 0-based tab position a "Go to Tab N" action selects. */
export function tabIndex(action: HotkeyAction): number | null {
  const index = SELECT_TAB_ACTIONS.indexOf(action);
  return index === -1 ? null : index;
}

const DISPLAY_NAMES: Record<HotkeyAction, string> = {
  newAgent: 'New Agent',
  openProject: 'Open Project',
  settings: 'Settings',
  closeAgent: 'Close Tab',
  focusSidebar: 'Focus Sidebar',
  focusContent: 'Focus Content',
  toggleSidebar: 'Toggle Sidebar',
  toggleWorkspaceSidebar: 'Toggle Workspace Sidebar',
  navDashboard: 'Dashboard',
  navAgents: 'Agents',
  navSchedule: 'Schedule',
  splitBelow: 'Split Below',
  splitRight: 'Split Right',
  closePane: 'Close Pane',
  focusUp: 'Focus Up',
  focusDown: 'Focus Down',
  focusLeft: 'Focus Left',
  focusRight: 'Focus Right',
  newTab: 'New Tab',
  nextTab: 'Next Tab',
  previousTab: 'Previous Tab',
  selectTab1: 'Go to Tab 1',
  selectTab2: 'Go to Tab 2',
  selectTab3: 'Go to Tab 3',
  selectTab4: 'Go to Tab 4',
  selectTab5: 'Go to Tab 5',
  selectTab6: 'Go to Tab 6',
  selectTab7: 'Go to Tab 7',
  selectTab8: 'Go to Tab 8',
  selectLastTab: 'Go to Last Tab',
  breakTab: 'Move Tab to New Pane',
  toggleChatSidebar: 'Toggle Chat Sidebar',
};

export function actionDisplayName(action: HotkeyAction): string {
  return DISPLAY_NAMES[action];
}

const CATEGORIES: Record<HotkeyAction, HotkeyCategory> = {
  newAgent: 'application',
  openProject: 'application',
  settings: 'application',
  closeAgent: 'tabs',
  focusSidebar: 'navigation',
  focusContent: 'navigation',
  toggleSidebar: 'navigation',
  toggleWorkspaceSidebar: 'navigation',
  navDashboard: 'navigation',
  navAgents: 'navigation',
  navSchedule: 'navigation',
  splitBelow: 'panes',
  splitRight: 'panes',
  closePane: 'panes',
  focusUp: 'panes',
  focusDown: 'panes',
  focusLeft: 'panes',
  focusRight: 'panes',
  newTab: 'tabs',
  nextTab: 'tabs',
  previousTab: 'tabs',
  selectTab1: 'tabs',
  selectTab2: 'tabs',
  selectTab3: 'tabs',
  selectTab4: 'tabs',
  selectTab5: 'tabs',
  selectTab6: 'tabs',
  selectTab7: 'tabs',
  selectTab8: 'tabs',
  selectLastTab: 'tabs',
  breakTab: 'tabs',
  toggleChatSidebar: 'chat',
};

export function actionCategory(action: HotkeyAction): HotkeyCategory {
  return CATEGORIES[action];
}

export function actionsFor(category: HotkeyCategory): HotkeyAction[] {
  return HOTKEY_ACTIONS.filter((action) => CATEGORIES[action] === category);
}

/**
 * Actions the Mac app handles in its NSEvent monitor rather than in SwiftUI menu commands. On
 * Windows every chord goes through one keydown handler; this is kept so the menu (WIN-7.1) can
 * list the same set of actions the Mac menus do.
 */
export function isMonitorHandled(action: HotkeyAction): boolean {
  switch (action) {
    case 'focusSidebar':
    case 'focusContent':
    case 'toggleSidebar':
    case 'toggleWorkspaceSidebar':
    case 'navDashboard':
    case 'navAgents':
    case 'navSchedule':
    case 'toggleChatSidebar':
      return true;
    default:
      return false;
  }
}

// MARK: - KeyModifier

/** Windows modifiers. `meta` is the Win key; no default uses it. */
export const KEY_MODIFIERS = ['ctrl', 'alt', 'shift', 'meta'] as const;
export type KeyModifier = (typeof KEY_MODIFIERS)[number];

export const MODIFIER_LABELS: Record<KeyModifier, string> = {
  ctrl: 'Ctrl',
  alt: 'Alt',
  shift: 'Shift',
  meta: 'Win',
};

function isKeyModifier(value: unknown): value is KeyModifier {
  return typeof value === 'string' && (KEY_MODIFIERS as readonly string[]).includes(value);
}

/** Unique modifiers in display order: Ctrl, Alt, Shift, Win. */
export function sortModifiers(modifiers: Iterable<KeyModifier>): KeyModifier[] {
  const set = new Set(modifiers);
  return KEY_MODIFIERS.filter((m) => set.has(m));
}

// MARK: - SpecialKey

/** The Swift `SpecialKey` cases and the `KeyboardEvent.code` each one is on Windows. */
export const SPECIAL_KEY_CODES = {
  upArrow: 'ArrowUp',
  downArrow: 'ArrowDown',
  leftArrow: 'ArrowLeft',
  rightArrow: 'ArrowRight',
  escape: 'Escape',
  return: 'Enter',
  tab: 'Tab',
  space: 'Space',
  delete: 'Backspace',
} as const;
export type SpecialKey = keyof typeof SPECIAL_KEY_CODES;

// MARK: - KeyBinding

export interface KeyBinding {
  /** `KeyboardEvent.code`, e.g. `KeyN`, `Digit1`, `BracketLeft`, `ArrowUp`. */
  readonly code: string;
  /** Unique, in display order (see `sortModifiers`). */
  readonly modifiers: readonly KeyModifier[];
}

export function keyBinding(code: string, ...modifiers: KeyModifier[]): KeyBinding {
  return { code, modifiers: sortModifiers(modifiers) };
}

/** A stable identity for a chord, e.g. `ctrl+shift+KeyN`; equal chords give equal ids. */
export function chordId(binding: KeyBinding): string {
  return [...sortModifiers(binding.modifiers), binding.code].join('+');
}

export function bindingsEqual(a: KeyBinding, b: KeyBinding): boolean {
  return chordId(a) === chordId(b);
}

const CODE_LABELS: Record<string, string> = {
  Minus: '-',
  Equal: '=',
  BracketLeft: '[',
  BracketRight: ']',
  Backslash: '\\',
  IntlBackslash: '\\',
  Semicolon: ';',
  Quote: "'",
  Comma: ',',
  Period: '.',
  Slash: '/',
  Backquote: '`',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  Escape: 'Esc',
  Enter: 'Enter',
  Tab: 'Tab',
  Space: 'Space',
  Backspace: 'Backspace',
  Delete: 'Del',
  Insert: 'Ins',
  Home: 'Home',
  End: 'End',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
};

/** The key's label as Windows shows it in menus: `N`, `1`, `[`, `Up`, `Tab`, `F5`. */
export function keyLabel(code: string): string {
  const fixed = CODE_LABELS[code];
  if (fixed !== undefined) return fixed;
  const letter = /^Key([A-Z])$/.exec(code);
  if (letter?.[1] !== undefined) return letter[1];
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit?.[1] !== undefined) return digit[1];
  const numpad = /^Numpad(.+)$/.exec(code);
  if (numpad?.[1] !== undefined) return `Num ${numpad[1]}`;
  return code;
}

/** Ordered display keys for UI rendering, e.g. `["Ctrl", "Shift", "N"]`. */
export function displayKeys(binding: KeyBinding): string[] {
  return [
    ...sortModifiers(binding.modifiers).map((m) => MODIFIER_LABELS[m]),
    keyLabel(binding.code),
  ];
}

/** Windows-style display string, e.g. `Ctrl+Shift+N`. */
export function displayString(binding: KeyBinding): string {
  return displayKeys(binding).join('+');
}

// MARK: - Defaults (DEC-8)

const DEFAULT_BINDINGS: Record<HotkeyAction, KeyBinding> = {
  newAgent: keyBinding('KeyN', 'ctrl', 'shift'),
  openProject: keyBinding('KeyO', 'ctrl', 'shift'),
  settings: keyBinding('Comma', 'ctrl'),
  closeAgent: keyBinding('KeyW', 'ctrl', 'shift'),
  focusSidebar: keyBinding('BracketLeft', 'ctrl', 'shift'),
  focusContent: keyBinding('BracketRight', 'ctrl', 'shift'),
  toggleSidebar: keyBinding('Backslash', 'ctrl', 'shift'),
  toggleWorkspaceSidebar: keyBinding('Backslash', 'alt', 'shift'),
  navDashboard: keyBinding('Digit1', 'alt', 'shift'),
  navAgents: keyBinding('Digit2', 'alt', 'shift'),
  navSchedule: keyBinding('Digit3', 'alt', 'shift'),
  splitBelow: keyBinding('Minus', 'alt', 'shift'),
  splitRight: keyBinding('Equal', 'alt', 'shift'),
  closePane: keyBinding('KeyW', 'alt', 'shift'),
  focusUp: keyBinding('ArrowUp', 'alt'),
  focusDown: keyBinding('ArrowDown', 'alt'),
  focusLeft: keyBinding('ArrowLeft', 'alt'),
  focusRight: keyBinding('ArrowRight', 'alt'),
  newTab: keyBinding('KeyT', 'ctrl', 'shift'),
  nextTab: keyBinding('Tab', 'ctrl'),
  previousTab: keyBinding('Tab', 'ctrl', 'shift'),
  selectTab1: keyBinding('Digit1', 'ctrl', 'shift'),
  selectTab2: keyBinding('Digit2', 'ctrl', 'shift'),
  selectTab3: keyBinding('Digit3', 'ctrl', 'shift'),
  selectTab4: keyBinding('Digit4', 'ctrl', 'shift'),
  selectTab5: keyBinding('Digit5', 'ctrl', 'shift'),
  selectTab6: keyBinding('Digit6', 'ctrl', 'shift'),
  selectTab7: keyBinding('Digit7', 'ctrl', 'shift'),
  selectTab8: keyBinding('Digit8', 'ctrl', 'shift'),
  selectLastTab: keyBinding('Digit9', 'ctrl', 'shift'),
  breakTab: keyBinding('KeyD', 'alt', 'shift'),
  toggleChatSidebar: keyBinding('KeyS', 'ctrl', 'shift'),
};

export function defaultBinding(action: HotkeyAction): KeyBinding {
  return DEFAULT_BINDINGS[action];
}

// MARK: - Events

/** The subset of `KeyboardEvent` chord matching reads, so tests can pass plain objects. */
export interface KeyChordEvent {
  readonly code: string;
  readonly ctrlKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
  readonly metaKey: boolean;
  getModifierState?(key: string): boolean;
}

function eventModifiers(event: KeyChordEvent): KeyModifier[] {
  const mods: KeyModifier[] = [];
  if (event.ctrlKey) mods.push('ctrl');
  if (event.altKey) mods.push('alt');
  if (event.shiftKey) mods.push('shift');
  if (event.metaKey) mods.push('meta');
  return mods;
}

/** AltGr reports Ctrl+Alt on Windows; such presses type characters and are never chords. */
export function isAltGraph(event: KeyChordEvent): boolean {
  return event.getModifierState?.('AltGraph') === true;
}

/** Whether this binding matches a keydown: same physical key, exactly the same modifiers. */
export function matches(binding: KeyBinding, event: KeyChordEvent): boolean {
  if (isAltGraph(event)) return false;
  return event.code === binding.code && chordId(binding) === chordId(eventChord(event));
}

function eventChord(event: KeyChordEvent): KeyBinding {
  return { code: event.code, modifiers: sortModifiers(eventModifiers(event)) };
}

const MODIFIER_CODES = new Set([
  'ControlLeft',
  'ControlRight',
  'AltLeft',
  'AltRight',
  'ShiftLeft',
  'ShiftRight',
  'MetaLeft',
  'MetaRight',
  'OSLeft',
  'OSRight',
  'CapsLock',
  'NumLock',
  'ScrollLock',
  'Fn',
  'FnLock',
]);

/**
 * Create a binding from a keydown (for recording). Like the Swift version, a chord must include
 * Ctrl, Alt or the Win key; Shift alone is not enough. Modifier-only and AltGr presses give null.
 */
export function bindingFromEvent(event: KeyChordEvent): KeyBinding | null {
  if (event.code === '' || MODIFIER_CODES.has(event.code) || isAltGraph(event)) return null;
  const chord = eventChord(event);
  if (!chord.modifiers.some((m) => m === 'ctrl' || m === 'alt' || m === 'meta')) return null;
  return chord;
}

// MARK: - Terminal pass-through

const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ'.split('');
const CONTROL_PUNCTUATION = ['BracketLeft', 'BracketRight', 'Backslash', 'Minus', 'Space', 'Slash'];

/**
 * Chords that always belong to a focused terminal: plain Ctrl+letter (Ctrl+C, Ctrl+D, Ctrl+R,
 * Ctrl+L, Ctrl+Z, ...) and the plain Ctrl+punctuation keys that send control characters
 * (Ctrl+[ is Esc, Ctrl+\ is SIGQUIT, Ctrl+Space is NUL, ...).
 */
export const reservedTerminalChords: ReadonlySet<string> = new Set([
  ...LETTERS.map((l) => chordId(keyBinding(`Key${l}`, 'ctrl'))),
  ...CONTROL_PUNCTUATION.map((code) => chordId(keyBinding(code, 'ctrl'))),
]);

/** Whether a chord is reserved for the terminal when one has focus. */
export function isReservedTerminalChord(binding: KeyBinding): boolean {
  return reservedTerminalChords.has(chordId(binding));
}

/**
 * Whether the app should take this keydown: it matches one of `bindings` (the defaults unless the
 * caller passes the current ones) and, when a terminal has focus, is not a reserved terminal
 * chord. A custom binding on Ctrl+C still reaches the terminal while it is focused.
 */
export function isAppChord(
  event: KeyChordEvent,
  focusInTerminal: boolean,
  bindings: Readonly<Record<HotkeyAction, KeyBinding>> = DEFAULT_BINDINGS,
): boolean {
  return actionForEvent(event, focusInTerminal, bindings) !== null;
}

/** The action this keydown triggers, or null (see `isAppChord`). */
export function actionForEvent(
  event: KeyChordEvent,
  focusInTerminal: boolean,
  bindings: Readonly<Record<HotkeyAction, KeyBinding>> = DEFAULT_BINDINGS,
): HotkeyAction | null {
  if (isAltGraph(event)) return null;
  const chord = eventChord(event);
  if (focusInTerminal && isReservedTerminalChord(chord)) return null;
  for (const action of HOTKEY_ACTIONS) {
    if (matches(bindings[action], event)) return action;
  }
  return null;
}

// MARK: - Persistence encoding

/** JSON shape of one saved binding: `{ "code": "KeyN", "modifiers": ["ctrl", "shift"] }`. */
export function encodeBinding(binding: KeyBinding): { code: string; modifiers: KeyModifier[] } {
  return { code: binding.code, modifiers: sortModifiers(binding.modifiers) };
}

/** Decode one saved binding, or null when it is malformed (the Swift decoder throws). */
export function decodeBinding(value: unknown): KeyBinding | null {
  if (typeof value !== 'object' || value === null) return null;
  const { code, modifiers } = value as { code?: unknown; modifiers?: unknown };
  if (typeof code !== 'string' || code === '') return null;
  if (!Array.isArray(modifiers) || !modifiers.every(isKeyModifier)) return null;
  return { code, modifiers: sortModifiers(modifiers) };
}
