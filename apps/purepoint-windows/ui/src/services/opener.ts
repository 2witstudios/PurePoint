/**
 * Opens a URL outside the app, in the user's browser or mail client, through the Tauri opener
 * plugin (`plugin:opener|open_url`), so a link never navigates the app's own webview.
 */
import { openUrl } from '@tauri-apps/plugin-opener';

interface OpenerSpyGlobal {
  /**
   * Set by the end-to-end tests only: receives each URL instead of the system opener, so a test
   * run never opens a browser. (`__TAURI_INTERNALS__.invoke` is read-only, so it cannot be
   * intercepted there.)
   */
  __PU_OPENER_SPY__?: string[];
}

export function openExternal(url: string): void {
  const spy = (globalThis as OpenerSpyGlobal).__PU_OPENER_SPY__;
  if (Array.isArray(spy)) {
    spy.push(url);
    return;
  }
  openUrl(url).catch((error: unknown) => {
    console.error(`PurePoint: could not open ${url}`, error);
  });
}
