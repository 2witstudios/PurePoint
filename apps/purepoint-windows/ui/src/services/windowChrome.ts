// Front end of the window chrome (WIN-7.1): `src-tauri/src/chrome.rs` and the Tauri window API.
// The title bar reports where it laid itself out so the backend can answer Windows' hit tests
// (Snap Layouts over maximize, the caption drag area), and the backend reports the maximize
// button's hover and pressed states, which CSS cannot see because the backend owns that input.
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** The event `chrome.rs` emits with {@link CaptionButtonsState}. */
export const CAPTION_BUTTONS_EVENT = 'chrome-caption-buttons';

/** A rectangle in CSS px relative to the window's client area. */
export interface ChromeRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Where the title bar is: the strip, its maximize button, and what in it takes clicks. */
export interface ChromeLayout {
  caption: ChromeRect;
  maximize: ChromeRect;
  interactive: ChromeRect[];
}

export interface CaptionButtonsState {
  maximizeHover: boolean;
  maximizePressed: boolean;
}

export type WindowAction = 'minimize' | 'toggleMaximize' | 'close';

export function rectOf(element: Element): ChromeRect {
  const { x, y, width, height } = element.getBoundingClientRect();
  return { x, y, width, height };
}

/** Tells the backend where the title bar is (`chrome_set_layout`). */
export async function setChromeLayout(layout: ChromeLayout): Promise<void> {
  await invoke('chrome_set_layout', { layout });
}

/** Calls `handler` whenever the maximize button's hover or pressed state changes. */
export function onCaptionButtons(
  handler: (state: CaptionButtonsState) => void,
): Promise<UnlistenFn> {
  return listen<CaptionButtonsState>(CAPTION_BUTTONS_EVENT, (event) => {
    handler(event.payload);
  });
}

/** Minimize, maximize/restore or close the main window; failures are logged, not thrown. */
export function runWindowAction(action: WindowAction): void {
  const current = getCurrentWindow();
  current[action]().catch((error: unknown) => {
    console.error(`PurePoint: window ${action} failed`, error);
  });
}

export function isMaximized(): Promise<boolean> {
  return getCurrentWindow().isMaximized();
}

/** Calls `handler` after every resize, including maximize and restore. */
export function onWindowResized(handler: () => void): Promise<UnlistenFn> {
  return getCurrentWindow().onResized(() => {
    handler();
  });
}
