// The Hotkeys pane's recorder (WIN-8.11): what one keydown does while a row records. The Mac's
// `NSEvent` monitor in Views/Settings/SettingsHotkeysView.swift, on `KeyboardEvent.code`.
//
// Differences from Swift: chords a focused terminal always keeps (plain Ctrl+letter and the Ctrl
// punctuation keys, `reservedTerminalChords`) are refused, since the app could only use them
// outside a terminal; the Mac has no such chords (Cmd never reaches the shell).
import {
  type HotkeyAction,
  type KeyBinding,
  type KeyChordEvent,
  bindingFromEvent,
  displayString,
  isReservedTerminalChord,
} from '../../model/keyBinding.ts';

export type RecorderResult =
  /** Plain Esc: stop recording, nothing changes. */
  | { readonly kind: 'cancel' }
  /** A modifier on its own, a key without Ctrl/Alt/Win, or AltGr: keep listening. */
  | { readonly kind: 'ignored' }
  /** A chord the terminal keeps: say so and keep listening. */
  | { readonly kind: 'reserved'; readonly binding: KeyBinding; readonly message: string }
  /** Another action has the chord: offer Override (swap) or Cancel. */
  | { readonly kind: 'conflict'; readonly binding: KeyBinding; readonly action: HotkeyAction }
  /** Free: save it. */
  | { readonly kind: 'set'; readonly binding: KeyBinding };

export interface RecorderBindings {
  detectConflict(binding: KeyBinding, excluding: HotkeyAction): HotkeyAction | null;
}

export function reservedMessage(binding: KeyBinding): string {
  return `${displayString(binding)} is kept for the terminal`;
}

/** What a keydown while recording `action` does. */
export function recordKey(
  event: KeyChordEvent,
  action: HotkeyAction,
  bindings: RecorderBindings,
): RecorderResult {
  if (
    event.code === 'Escape' &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.shiftKey &&
    !event.metaKey
  ) {
    return { kind: 'cancel' };
  }
  const binding = bindingFromEvent(event);
  if (binding === null) return { kind: 'ignored' };
  if (isReservedTerminalChord(binding)) {
    return { kind: 'reserved', binding, message: reservedMessage(binding) };
  }
  const conflict = bindings.detectConflict(binding, action);
  if (conflict !== null) return { kind: 'conflict', binding, action: conflict };
  return { kind: 'set', binding };
}

/** The Override button: `action` takes `binding`, and `conflict` takes `action`'s old chord. */
export function overrideBindings(
  store: {
    binding(action: HotkeyAction): KeyBinding;
    setBinding(binding: KeyBinding, action: HotkeyAction): void;
  },
  action: HotkeyAction,
  conflict: HotkeyAction,
  binding: KeyBinding,
): void {
  const old = store.binding(action);
  store.setBinding(old, conflict);
  store.setBinding(binding, action);
}
