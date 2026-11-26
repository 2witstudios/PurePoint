// Port of apps/purepoint-macos/purepoint-macos/Views/Terminal/ScrollableTerminal.swift (WIN-7.3).
//
// One xterm.js terminal: renderer, addons, theme, keyboard and clipboard rules, links, and the
// split between the pane's own (local) grid size and the size the daemon reports.
//
// Differences from Swift, because xterm.js already does what SwiftTerm needed help with:
// - Wheel events on the alternate screen: xterm.js reports them to mouse-mode TUIs itself (the
//   Mac's scroll monitor and its 16 ms tick accumulator are not needed).
// - Selection ownership: a mouse-mode TUI gets clicks, and Shift forces local selection, the
//   Windows convention (the Mac uses Option). xterm.js sends correct SGR hover reports, so the
//   Mac's hover filter is not needed; drag autoscroll is built in.
// - Links: Ctrl+click opens web URLs and OSC 8 hyperlinks (Cmd-click on the Mac) through the
//   opener service. The link-under-pointer hit test is xterm.js's link providers.
// - Remote hosts (`usesRemoteHost`, uploads of pasted images and dropped files) arrive with
//   remote machines (WIN-8.14); local file drops are the pane grid's (WIN-7.4).
// - Output filters (TerminalOutputFilterTests): none needed, see outputFilter.test.ts.
import { FitAddon } from '@xterm/addon-fit';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { WebglAddon } from '@xterm/addon-webgl';
import { Terminal } from '@xterm/xterm';
import type { KeyChordEvent } from '../../model/keyBinding.ts';
import { openExternal } from '../../services/opener.ts';
import { useKeyBindingStore } from '../../state/keyBindingStore.ts';
import { terminalFontFamily, terminalTheme } from './terminalTheme.ts';

export interface GridSize {
  readonly cols: number;
  readonly rows: number;
}

/** Swift `localGridSize` default, used until the pane has been measured. */
export const DEFAULT_GRID: GridSize = { cols: 120, rows: 40 };
/** `changeHistorySize(10_000)`. */
export const SCROLLBACK_LINES = 10_000;

function sameSize(a: GridSize | null, b: GridSize | null): boolean {
  return a !== null && b !== null && a.cols === b.cols && a.rows === b.rows;
}

/**
 * The pane's two sizes (`localGridSize` and `reportedGridSize`). The local size is what fits the
 * pane and is what the pane asks the daemon for; the reported size is what the daemon applied
 * (it differs while another client owns the PTY size). The terminal renders the reported size
 * once there is one, so its grid always matches the program's.
 */
export class TerminalGeometry {
  private localSize: GridSize = DEFAULT_GRID;
  private reportedSize: GridSize | null = null;

  get local(): GridSize {
    return this.localSize;
  }

  get reported(): GridSize | null {
    return this.reportedSize;
  }

  /** The grid the terminal should render. */
  get render(): GridSize {
    return this.reportedSize ?? this.localSize;
  }

  /** A new measurement of the pane; true when the local size changed (send `resize`). */
  measure(size: GridSize): boolean {
    if (size.cols < 1 || size.rows < 1 || sameSize(size, this.localSize)) return false;
    this.localSize = { cols: size.cols, rows: size.rows };
    return true;
  }

  /** The daemon's `pty_size` (`applyReportedSize`). Never changes the local size. */
  report(size: GridSize): void {
    this.reportedSize = { cols: size.cols, rows: size.rows };
  }
}

/** What a keydown in a focused terminal does. */
export type TerminalKeyAction = 'copy' | 'paste' | 'app' | 'terminal';

function only(event: KeyChordEvent, ctrl: boolean, shift: boolean): boolean {
  return event.ctrlKey === ctrl && event.shiftKey === shift && !event.altKey && !event.metaKey;
}

/**
 * REQ-WINAPP-068: Ctrl+Shift+C copies, Ctrl+C copies only while text is selected, Ctrl+Shift+V
 * pastes, app chords go to the app, and everything else (plain Ctrl+letter included) is the
 * terminal's.
 */
export function terminalKeyAction(
  event: KeyChordEvent,
  hasSelection: boolean,
  isAppChord: (event: KeyChordEvent) => boolean,
): TerminalKeyAction {
  if (event.code === 'KeyC' && only(event, true, true)) return 'copy';
  if (event.code === 'KeyC' && only(event, true, false) && hasSelection) return 'copy';
  if (event.code === 'KeyV' && only(event, true, true)) return 'paste';
  if (isAppChord(event)) return 'app';
  return 'terminal';
}

function appChord(event: KeyChordEvent): boolean {
  const bindings = useKeyBindingStore.getState();
  // While a shortcut is being recorded the recorder owns every key.
  if (bindings.recordingAction !== null) return true;
  return bindings.actionFor(event, true) !== null;
}

/** The URL schemes a Ctrl+click may hand to the system. */
export function isOpenableUrl(url: string): boolean {
  try {
    const { protocol } = new URL(url);
    return protocol === 'http:' || protocol === 'https:' || protocol === 'mailto:';
  } catch {
    return false;
  }
}

export interface ScrollableTerminalOptions {
  fontSize: number;
  /** Text xterm.js encoded from keys, paste and mouse reports (`send(source:data:)`). */
  onData: (data: string) => void;
  /** Binary-encoded reports (X10 mouse coordinates above 95), one char per byte. */
  onBinary: (data: string) => void;
  onFocus: () => void;
  onBlur: () => void;
  openLink?: (url: string) => void;
  clipboard?: Pick<Clipboard, 'readText' | 'writeText'>;
}

/** Which renderer is drawing: WebGL, or xterm.js's DOM renderer after a context loss. */
export type TerminalRenderer = 'webgl' | 'dom';

export class ScrollableTerminal {
  readonly element: HTMLDivElement;
  readonly term: Terminal;
  readonly geometry = new TerminalGeometry();
  private readonly fit = new FitAddon();
  private webgl: WebglAddon | null = null;
  private opened = false;
  private disposed = false;
  private readonly options: ScrollableTerminalOptions;
  private readonly openLink: (url: string) => void;
  private readonly onTextareaFocus: () => void;
  private readonly onTextareaBlur: () => void;

  constructor(options: ScrollableTerminalOptions) {
    this.options = options;
    this.openLink = options.openLink ?? openExternal;
    this.element = document.createElement('div');
    this.element.className = 'terminal-surface';
    this.term = new Terminal({
      allowProposedApi: true,
      scrollback: SCROLLBACK_LINES,
      fontFamily: terminalFontFamily(),
      fontSize: options.fontSize,
      theme: terminalTheme(),
      cols: DEFAULT_GRID.cols,
      rows: DEFAULT_GRID.rows,
      // ScrollableTerminal hides the cursor until the program draws one.
      cursorBlink: false,
      linkHandler: {
        activate: (event, text) => {
          this.activateLink(event, text);
        },
      },
    });
    this.term.loadAddon(this.fit);
    this.term.loadAddon(new Unicode11Addon());
    this.term.unicode.activeVersion = '11';
    this.term.loadAddon(
      new WebLinksAddon((event, uri) => {
        this.activateLink(event, uri);
      }),
    );
    this.term.attachCustomKeyEventHandler((event) => this.handleKey(event));
    this.term.onData(options.onData);
    this.term.onBinary(options.onBinary);
    this.onTextareaFocus = () => {
      options.onFocus();
    };
    this.onTextareaBlur = () => {
      options.onBlur();
    };
  }

  get renderer(): TerminalRenderer {
    return this.webgl === null ? 'dom' : 'webgl';
  }

  get isOpen(): boolean {
    return this.opened;
  }

  /** Create the DOM. Call once the element is in the document, so xterm.js can measure. */
  open(): void {
    if (this.opened || this.disposed) return;
    this.opened = true;
    this.term.open(this.element);
    const textarea = this.term.textarea;
    textarea?.addEventListener('focus', this.onTextareaFocus);
    textarea?.addEventListener('blur', this.onTextareaBlur);
    this.loadWebgl();
  }

  /** WebGL, falling back to the DOM renderer when WebGL is missing or its context is lost. */
  private loadWebgl(): void {
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        // Chromium drops the oldest context past 16; this pane keeps working on the DOM.
        webgl.dispose();
        if (this.webgl === webgl) this.webgl = null;
      });
      this.term.loadAddon(webgl);
      this.webgl = webgl;
    } catch (error) {
      console.warn('PurePoint: WebGL terminal renderer unavailable, using the DOM', error);
      this.webgl = null;
    }
  }

  private activateLink(event: MouseEvent, url: string): void {
    // Ctrl+click, like Windows Terminal; a plain click belongs to selection.
    if (!event.ctrlKey || !isOpenableUrl(url)) return;
    event.preventDefault();
    this.openLink(url);
  }

  private get clipboard(): Pick<Clipboard, 'readText' | 'writeText'> | null {
    return (
      this.options.clipboard ?? (typeof navigator === 'undefined' ? null : navigator.clipboard)
    );
  }

  /** `attachCustomKeyEventHandler`: false keeps xterm.js from handling the key. */
  private handleKey(event: KeyboardEvent): boolean {
    if (event.type !== 'keydown' && event.type !== 'keypress') return true;
    const action = terminalKeyAction(event, this.term.hasSelection(), appChord);
    switch (action) {
      case 'terminal':
        return true;
      case 'app':
        // Bubbles on to the window's accelerator handler (ContentView).
        return false;
      case 'copy':
        event.preventDefault();
        if (event.type === 'keydown') this.copySelection();
        return false;
      case 'paste':
        // Also stops WebView2's own paste, which would paste a second time.
        event.preventDefault();
        if (event.type === 'keydown') this.pasteClipboard();
        return false;
    }
  }

  copySelection(): void {
    const text = this.term.getSelection();
    if (text === '') return;
    this.clipboard?.writeText(text).catch((error: unknown) => {
      console.warn('PurePoint: copy failed', error);
    });
    this.term.clearSelection();
  }

  pasteClipboard(): void {
    const clipboard = this.clipboard;
    if (clipboard === null) return;
    clipboard
      .readText()
      .then((text) => {
        // `paste` normalizes line endings and adds the bracketed-paste markers when enabled.
        if (text !== '' && !this.disposed) this.term.paste(text);
      })
      .catch((error: unknown) => {
        console.warn('PurePoint: paste failed', error);
      });
  }

  /** Raw PTY bytes; `parsed` runs once xterm.js has processed them. */
  write(data: Uint8Array, parsed?: () => void): void {
    if (this.disposed) return;
    this.term.write(data, parsed);
  }

  /** ESC c before a replay (`reset`, `resync`). */
  reset(): void {
    if (!this.disposed) this.term.reset();
  }

  focus(): void {
    this.term.focus();
  }

  hasFocus(): boolean {
    const textarea = this.term.textarea;
    return textarea !== undefined && document.activeElement === textarea;
  }

  /** The grid that fits the element now, or null while it has no size (hidden, parked). */
  measure(): GridSize | null {
    if (!this.opened || this.element.clientWidth <= 1 || this.element.clientHeight <= 1) {
      return null;
    }
    const proposed = this.fit.proposeDimensions();
    if (
      proposed === undefined ||
      !Number.isFinite(proposed.cols) ||
      !Number.isFinite(proposed.rows)
    ) {
      return null;
    }
    return { cols: Math.max(2, proposed.cols), rows: Math.max(1, proposed.rows) };
  }

  /** `renderGrid`: resize the model to `size` and repaint. */
  renderGrid(size: GridSize): void {
    if (this.disposed) return;
    if (this.term.cols !== size.cols || this.term.rows !== size.rows) {
      this.term.resize(size.cols, size.rows);
    }
  }

  /** Repaint everything, after the element moved between containers (`viewDidMoveToWindow`). */
  refresh(): void {
    if (this.opened && !this.disposed) this.term.refresh(0, this.term.rows - 1);
  }

  setFontSize(size: number): void {
    if (this.term.options.fontSize !== size) this.term.options.fontSize = size;
  }

  /** `applyTheme`: the appearance changed. */
  applyTheme(): void {
    this.term.options.theme = terminalTheme();
    this.term.options.fontFamily = terminalFontFamily();
  }

  get cols(): number {
    return this.term.cols;
  }

  get rows(): number {
    return this.term.rows;
  }

  get fontFamily(): string {
    return this.term.options.fontFamily ?? '';
  }

  get fontSize(): number {
    return this.term.options.fontSize ?? 0;
  }

  /** The viewport point at the middle of the first cell of `needle`, if it is on screen. */
  locate(needle: string): { x: number; y: number } | null {
    const screen = this.element.querySelector('.xterm-screen');
    if (screen === null) return null;
    const rect = screen.getBoundingClientRect();
    const buffer = this.term.buffer.active;
    const cellWidth = rect.width / this.term.cols;
    const cellHeight = rect.height / this.term.rows;
    for (let row = 0; row < this.term.rows; row++) {
      const text = buffer.getLine(buffer.viewportY + row)?.translateToString(false) ?? '';
      const col = text.indexOf(needle);
      if (col >= 0) {
        return {
          x: Math.round(rect.left + (col + 0.5) * cellWidth),
          y: Math.round(rect.top + (row + 0.5) * cellHeight),
        };
      }
    }
    return null;
  }

  /** The active buffer's text, trailing blanks trimmed (the e2e hook reads it). */
  bufferText(): string {
    const buffer = this.term.buffer.active;
    const lines: string[] = [];
    for (let y = 0; y < buffer.length; y++) {
      lines.push(buffer.getLine(y)?.translateToString(true) ?? '');
    }
    return lines.join('\n').trimEnd();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    const textarea = this.term.textarea;
    textarea?.removeEventListener('focus', this.onTextareaFocus);
    textarea?.removeEventListener('blur', this.onTextareaBlur);
    this.term.dispose();
    this.element.remove();
  }
}
