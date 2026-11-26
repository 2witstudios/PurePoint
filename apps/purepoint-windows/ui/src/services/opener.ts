/**
 * Hands URLs and local files to Windows (WIN-7.3, WIN-8.15; REQ-WINAPP-074), through the Tauri
 * opener plugin, so nothing ever navigates the app's own webview:
 *
 * - `openExternal`: a URL in the user's browser or mail client (`plugin:opener|open_url`),
 *   where the Mac calls `NSWorkspace.open(url)`.
 * - `openInDefaultApp`: a project file in its default app (`ShellExecute`), the Mac's
 *   `NSWorkspace.open(fileURL)`, through the app's own `open_in_default_app` command
 *   (`src-tauri/src/files/open.rs`): Windows runs scripts, shortcuts and installers as their
 *   "default app", so those are refused here and in the backend, which also requires the file to
 *   be inside an open project. The caller offers Reveal in Explorer instead.
 * - `revealInExplorer`: an Explorer window with the item selected (`explorer.exe /select,`,
 *   `reveal_item_in_dir`), the Mac's Reveal in Finder (`NSWorkspace.selectFile`).
 */
import { invoke } from '@tauri-apps/api/core';
import { testHooksEnabled } from './testHooks';
import { openUrl, revealItemInDir } from '@tauri-apps/plugin-opener';

interface OpenerSpyGlobal {
  /**
   * Set by the end-to-end tests only (read in debug builds only, `__PU_TEST_HOOKS__`): receives each URL instead of the system opener, so a test
   * run never opens a browser, an app or Explorer. Files arrive as `open-path:<path>` and
   * `reveal:<path>`. (`__TAURI_INTERNALS__.invoke` is read-only, so it cannot be intercepted
   * there.)
   */
  __PU_OPENER_SPY__?: string[];
}

function spy(): string[] | null {
  if (!testHooksEnabled()) return null;
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

/**
 * Types Windows runs (or hands to an interpreter or installer) instead of opening as documents.
 * Kept in step with `BLOCKED_EXTENSIONS` in `src-tauri/src/files/open.rs` (a Rust test checks);
 * the backend also blocks everything in `PATHEXT`.
 */
export const DEFAULT_APP_BLOCKED_EXTENSIONS: ReadonlySet<string> = new Set([
  'appref-ms',
  'application',
  'appx',
  'appxbundle',
  'bat',
  'chm',
  'cmd',
  'com',
  'cpl',
  'diagcab',
  'exe',
  'gadget',
  'hta',
  'inf',
  'ins',
  'isp',
  'jar',
  'js',
  'jse',
  'library-ms',
  'lnk',
  'msc',
  'msi',
  'msix',
  'msixbundle',
  'msp',
  'mst',
  'pif',
  'ps1',
  'ps1xml',
  'ps2',
  'psc1',
  'psd1',
  'psm1',
  'reg',
  'scf',
  'scr',
  'search-ms',
  'searchconnector-ms',
  'settingcontent-ms',
  'shb',
  'shs',
  'sys',
  'url',
  'vb',
  'vbe',
  'vbs',
  'website',
  'ws',
  'wsc',
  'wsf',
  'wsh',
  'xbap',
]);

/**
 * Whether Open in Default App would run `path` rather than open it. Windows drops trailing dots
 * and spaces from names (`a.bat.` is `a.bat`); a stream name (`a.txt:x.exe`) or a shell
 * namespace extension (`x.{CLSID}`) is refused too.
 */
export function isBlockedFromDefaultApp(path: string): boolean {
  const name = (path.split(/[\\/]/).pop() ?? '').replace(/[. ]+$/, '');
  if (name.includes(':')) return true;
  const dot = name.lastIndexOf('.');
  if (dot < 0) return false;
  const extension = name.slice(dot + 1).toLowerCase();
  return extension.startsWith('{') || DEFAULT_APP_BLOCKED_EXTENSIONS.has(extension);
}

/** What Open in Default App did: `blocked` means the caller should offer Reveal instead. */
export type DefaultAppOutcome = 'opened' | 'blocked';

/** Open a project file or folder in the app Windows associates with it, unless it would run. */
export function openInDefaultApp(path: string): DefaultAppOutcome {
  if (isBlockedFromDefaultApp(path)) return 'blocked';
  const recorded = spy();
  if (recorded !== null) {
    recorded.push(`open-path:${path}`);
    return 'opened';
  }
  invoke('open_in_default_app', { path }).catch(logFailure(path));
  return 'opened';
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
