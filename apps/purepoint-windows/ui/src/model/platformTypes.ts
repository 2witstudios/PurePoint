// Port of apps/purepoint-macos/purepoint-macos/Platform/PlatformTypes.swift (WIN-6.0).
//
// Swift aliases SwiftTerm's `TerminalView` so shared code names one type on macOS and iOS, and
// keeps two platform calls there. The Windows terminal (xterm.js, WIN-7.x) has one platform; this
// is the same seam as an interface: what shared code asks of the terminal view.

export interface PlatformTerminalView {
  /** Whether the terminal currently receives keyboard input. */
  readonly hasKeyboardFocus: boolean;
  /** Hand the daemon's reported PTY size to the scrollable terminal hosting this view. */
  applyReportedPtySize(cols: number, rows: number): void;
}

/** `hasKeyboardFocus` for a DOM-hosted terminal: focus is inside its element. */
export function elementHasKeyboardFocus(
  element: Element,
  document: Pick<Document, 'activeElement'> = globalThis.document,
): boolean {
  const active = document.activeElement;
  return active !== null && element.contains(active);
}
