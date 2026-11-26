/**
 * Hands URLs and local files to Windows (WIN-7.3, WIN-8.15; REQ-WINAPP-074), through the Tauri
 * opener plugin, so nothing ever navigates the app's own webview:
 *
 * - `openExternal`: a URL in the user's browser or mail client (`plugin:opener|open_url`),
 *   where the Mac calls `NSWorkspace.open(url)`.
 * - `openInDefaultApp`: a local file in its default app (`ShellExecute`, `open_path`), the Mac's
 *   `NSWorkspace.open(fileURL)`.
 * - `revealInExplorer`: an Explorer window with the item selected (`explorer.exe /select,`,
 *   `reveal_item_in_dir`), the Mac's Reveal in Finder (`NSWorkspace.selectFile`).
 */
import { openPath, openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';

interface OpenerSpyGlobal {
  /**
   * Set by the end-to-end tests only: receives each URL instead of the system opener, so a test
   * run never opens a browser, an app or Explorer. Files arrive as `open-path:<path>` and
   * `reveal:<path>`. (`__TAURI_INTERNALS__.invoke` is read-only, so it cannot be intercepted
   * there.)
   */
  __PU_OPENER_SPY__?: string[];
}

function spy(): string[] | null {
  const recorded = (globalThis as OpenerSpyGlobal).__PU_OPENER_SPY__;
  return Array.isArray(recorded) ? recorded : null;
}

function logFailure(what: string) {
  return (error: unknown) => {
    console.error(`PurePoint: could not open ${what}`, error);
  };
}

export function openExternal(url: string): void {
  const recorded = spy();
  if (recorded !== null) {
    recorded.push(url);
    return;
  }
  openUrl(url).catch(logFailure(url));
}

/** Open a local file or folder in the app Windows associates with it. */
export function openInDefaultApp(path: string): void {
  const recorded = spy();
  if (recorded !== null) {
    recorded.push(`open-path:${path}`);
    return;
  }
  openPath(path).catch(logFailure(path));
}

/** Show a local file or folder selected in an Explorer window. */
export function revealInExplorer(path: string): void {
  const recorded = spy();
  if (recorded !== null) {
    recorded.push(`reveal:${path}`);
    return;
  }
  revealItemInDir(path).catch(logFailure(path));
}
