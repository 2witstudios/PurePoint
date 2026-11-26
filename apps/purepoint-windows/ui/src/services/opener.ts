/**
 * Opens a URL outside the app, in the user's browser or mail client.
 *
 * Stub for now: `window.open` with no opener. A later leaf routes this through
 * the Tauri opener plugin so links never navigate the app's own webview.
 */
export function openExternal(url: string): void {
  window.open(url, '_blank', 'noopener,noreferrer');
}
