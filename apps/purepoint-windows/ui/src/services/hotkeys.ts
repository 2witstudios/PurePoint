// Port of apps/purepoint-macos/purepoint-macos/Services/HotkeyMonitor.swift (WIN-7.6).
//
// The app's keyboard shortcuts: one window `keydown` listener that turns a chord into its
// `HotkeyAction` (matched on `KeyboardEvent.code`, REQ-WINAPP-067) and runs it.
//
// Differences from Swift:
// - The Mac splits its shortcuts between SwiftUI menu commands and this monitor
//   (`isMonitorHandled`); the Windows menus are drawn in the page, so this one handler runs every
//   action and the menu bar only shows the accelerators.
// - A focused terminal keeps every chord that is not an app binding, and always keeps plain
//   Ctrl+letter (Ctrl+C, Ctrl+R, ...), REQ-WINAPP-068: xterm.js lets app chords bubble up to
//   here (`scrollableTerminal.ts`), and this handler skips the reserved ones.
// - The listener is on the bubble phase and leaves a key a focused control already handled
//   (`defaultPrevented`), so an editor's own Alt+Arrow or a dialog's keys win; the Mac's local
//   monitor sees a key before any view.
// - WebView2's own browser keys (F5, Ctrl+R, Ctrl+P, Ctrl+F, F12, zoom, ...) are turned off in
//   the backend (`src-tauri/src/webview.rs`, REQ-WINAPP-065), so they never reach the browser.

import type { HotkeyAction, KeyChordEvent } from '../model/keyBinding.ts';

/** What the monitor reads from the key binding store. */
export interface HotkeyBindings {
  readonly recordingAction: HotkeyAction | null;
  actionFor(event: KeyChordEvent, focusInTerminal: boolean): HotkeyAction | null;
}

/** Whether a key went to a terminal (xterm.js's helper textarea sits inside `.xterm`). */
export function isTerminalTarget(target: EventTarget | null): boolean {
  return typeof Element !== 'undefined' && target instanceof Element
    ? target.closest('.xterm') !== null
    : false;
}

/**
 * The action a keydown triggers (Swift `handleKeyDown`), or null when the key is not the app's:
 * nothing while a shortcut is being recorded, nothing for a key a control already handled, and
 * no reserved terminal chord while a terminal has focus.
 */
export function hotkeyActionFor(
  event: KeyChordEvent & {
    readonly defaultPrevented: boolean;
    readonly target: EventTarget | null;
  },
  bindings: HotkeyBindings,
): HotkeyAction | null {
  if (event.defaultPrevented) return null;
  if (bindings.recordingAction !== null) return null;
  return bindings.actionFor(event, isTerminalTarget(event.target));
}

export interface HotkeyMonitorOptions {
  /** The current bindings (the key binding store's state). */
  readonly bindings: () => HotkeyBindings;
  /** Runs an action; ContentView's `runCommand`. */
  readonly run: (action: HotkeyAction) => void;
  readonly target?: Pick<Window, 'addEventListener' | 'removeEventListener'>;
}

/** Swift `HotkeyMonitor.start()`; the returned function is `stop()`. */
export function startHotkeyMonitor(options: HotkeyMonitorOptions): () => void {
  const target = options.target ?? window;
  const onKeyDown = (event: KeyboardEvent) => {
    const action = hotkeyActionFor(event, options.bindings());
    if (action === null) return;
    // Consumed, as the Mac's monitor returns nil: nothing else sees the chord.
    event.preventDefault();
    event.stopPropagation();
    options.run(action);
  };
  target.addEventListener('keydown', onKeyDown);
  return () => {
    target.removeEventListener('keydown', onKeyDown);
  };
}
