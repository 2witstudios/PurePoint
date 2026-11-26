// Port of apps/purepoint-macos/purepoint-macos/Views/Terminal/TerminalPaneView.swift (WIN-7.3).
//
// `TerminalPaneController` is `TerminalPaneNSView`: one agent's terminal and its daemon attach
// lifecycle. The terminal is created when the pane is first mounted and the attach starts on the
// first layout that gives it a real size (the Mac's 1-column PTY guard). Output goes to xterm.js
// as raw bytes; `reset`/`resync` reset the terminal before the replay that follows; `pty_size`
// renders the daemon's grid (another client may own it); focus claims the PTY size and blur
// gives it up. A 5 s heartbeat reattaches a session that died while the agent lives.
//
// The cache (terminalViewCache.ts) keeps controllers alive while hidden; `mount` and `park`
// move the same element between the visible pane and a hidden parking element.
import {
  type AttachEvent,
  type AttachHandlers,
  type AttachOpenOptions,
  type AttachSession,
  openAttach,
} from '../../services/attach.ts';
import {
  type GridSize,
  ScrollableTerminal,
  type ScrollableTerminalOptions,
  type TerminalGeometry,
  type TerminalRenderer,
} from './scrollableTerminal.ts';
import { TERMINAL_DEFAULT_FONT_SIZE } from './terminalTheme.ts';
import { fnv1a, FNV_OFFSET, statsEnabled } from './terminalStats.ts';

/** What the controller needs from the terminal (ScrollableTerminal; a fake in unit tests). */
export interface TerminalSurface {
  readonly element: HTMLElement;
  readonly geometry: TerminalGeometry;
  readonly isOpen: boolean;
  readonly renderer: TerminalRenderer;
  open(): void;
  write(data: Uint8Array, parsed?: () => void): void;
  reset(): void;
  focus(): void;
  hasFocus(): boolean;
  measure(): GridSize | null;
  renderGrid(size: GridSize): void;
  refresh(): void;
  setFontSize(size: number): void;
  applyTheme(): void;
  bufferText?(): string;
  /** Viewport point (CSS px) of the first cell of `needle` on screen, for the e2e tests. */
  locate?(needle: string): { x: number; y: number } | null;
  readonly cols?: number;
  readonly rows?: number;
  readonly fontFamily?: string;
  readonly fontSize?: number;
  dispose(): void;
}

export type OpenAttach = (
  agentId: string,
  options: AttachOpenOptions,
  handlers: AttachHandlers,
) => Promise<AttachSession>;

export interface TerminalPaneOptions {
  open?: OpenAttach;
  createSurface?: (options: ScrollableTerminalOptions) => TerminalSurface;
  /** Live status: the controller's agent snapshot is stale (`isAgentAlive`). */
  isAgentAlive?: () => boolean;
  /** Output arrived while the pane was hidden (`onBackgroundOutput`). */
  onBackgroundOutput?: () => void;
  /** The daemon confirmed the agent is gone. */
  onGone?: () => void;
  machineId?: string;
  fontSize?: number;
  heartbeatMs?: number;
}

/** Bytes the terminal received and parsed; the checksum only while stats are enabled. */
export interface TerminalPaneStats {
  readonly bytesReceived: number;
  readonly bytesParsed: number;
  readonly checksum: number;
}

function toHex(data: string | Uint8Array): string {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
  return Array.from(bytes, (b) => b.toString(16).padStart(2, '0')).join('');
}

/** `startHeartbeat`'s interval. */
export const HEARTBEAT_MS = 5000;
/** A resize's SIGWINCH redraw is not new activity for this long (`resizeRedrawSettlesAt`). */
const RESIZE_REDRAW_SETTLE_MS = 1500;
/** The spinner stays at least this long once shown, so it does not flash. */
const SPINNER_MIN_MS = 500;

export class TerminalPaneController {
  readonly agentId: string;
  readonly element: HTMLDivElement;
  readonly surface: TerminalSurface;
  private readonly options: TerminalPaneOptions;
  private readonly openSession: OpenAttach;
  private session: AttachSession | null = null;
  /** Bumped per attach, so a superseded session's late messages are dropped. */
  private generation = 0;
  private attachStarted = false;
  private attachDone = false;
  /** The daemon closed the stream because the agent exited. */
  private streamEnded = false;
  private agentGone = false;
  private hasOutput = false;
  private shown = false;
  private disposed = false;
  private heartbeat: ReturnType<typeof setInterval> | null = null;
  private resizeObserver: ResizeObserver | null = null;
  private resizeRedrawSettlesAt = 0;
  private spinner: HTMLElement | null = null;
  private spinnerShownAt = 0;
  private bytesReceived = 0;
  private bytesParsed = 0;
  private checksum = FNV_OFFSET;
  /** Hex of each input sent, while stats are enabled (the e2e compares it with the PTY). */
  private sentInput: string[] = [];

  constructor(agentId: string, options: TerminalPaneOptions = {}) {
    this.agentId = agentId;
    this.options = options;
    this.openSession = options.open ?? openAttach;
    this.element = document.createElement('div');
    this.element.className = 'terminal-pane-view';
    this.element.dataset.agentId = agentId;
    const create = options.createSurface ?? ((o) => new ScrollableTerminal(o));
    this.surface = create({
      fontSize: options.fontSize ?? TERMINAL_DEFAULT_FONT_SIZE,
      onData: (data) => {
        this.send(data);
      },
      onBinary: (data) => {
        const bytes = new Uint8Array(data.length);
        for (let i = 0; i < data.length; i++) bytes[i] = data.charCodeAt(i) & 0xff;
        this.send(bytes);
      },
      onFocus: () => {
        void this.session?.claimSize();
      },
      onBlur: () => {
        void this.session?.blur();
      },
    });
    this.element.appendChild(this.surface.element);
  }

  get isAgentGone(): boolean {
    return this.agentGone;
  }

  get isShown(): boolean {
    return this.shown;
  }

  get isAttached(): boolean {
    return this.session !== null && !this.attachDone;
  }

  get stats(): TerminalPaneStats {
    return {
      bytesReceived: this.bytesReceived,
      bytesParsed: this.bytesParsed,
      checksum: this.checksum >>> 0,
    };
  }

  /** Show the terminal in `host` (`viewDidMoveToWindow` + `layout`). */
  mount(host: HTMLElement): void {
    if (this.disposed) return;
    if (this.element.parentElement !== host) host.appendChild(this.element);
    this.shown = true;
    if (!this.surface.isOpen) {
      this.surface.open();
      this.showSpinner();
    }
    if (this.resizeObserver === null && typeof ResizeObserver !== 'undefined') {
      this.resizeObserver = new ResizeObserver(() => {
        this.layout();
      });
      this.resizeObserver.observe(this.element);
    }
    this.layout();
    // Moved between containers: repaint every row.
    this.surface.refresh();
  }

  /** Hide the terminal, keeping it alive in `parking`. */
  park(parking: HTMLElement): void {
    this.shown = false;
    if (this.surface.hasFocus()) void this.session?.blur();
    if (!this.disposed) parking.appendChild(this.element);
  }

  /** `layout()`: measure, send the local size, and start the attach on the first real size. */
  layout(): void {
    if (this.disposed || !this.shown) return;
    const size = this.surface.measure();
    if (size === null) return;
    const geometry = this.surface.geometry;
    if (geometry.measure(size)) {
      this.resizeRedrawSettlesAt = Date.now() + RESIZE_REDRAW_SETTLE_MS;
      void this.session?.resize(size.cols, size.rows);
    }
    this.surface.renderGrid(geometry.render);
    if (!this.attachStarted) {
      this.attachStarted = true;
      this.startAttach();
      this.startHeartbeat();
    } else if (this.attachDone && this.canReattach) {
      this.startAttach();
    }
  }

  focus(): void {
    if (!this.disposed && this.shown) this.surface.focus();
  }

  setFontSize(size: number): void {
    this.surface.setFontSize(size);
    this.layout();
  }

  applyTheme(): void {
    this.surface.applyTheme();
  }

  /** Restart a session that died, if the pane can show it (`reconnectIfNeeded`). */
  reconnectIfNeeded(): void {
    if (this.agentGone || !this.attachStarted || !this.attachDone || !this.canReattach) return;
    if (this.surface.measure() === null) return;
    this.startAttach();
  }

  /** After an exit the terminal holds all the output; reattach only if the agent runs again. */
  private get canReattach(): boolean {
    return !this.streamEnded || (this.options.isAgentAlive?.() ?? false);
  }

  /** Start counting from zero (the e2e does this before a measured stream). */
  resetStats(): void {
    this.bytesReceived = 0;
    this.bytesParsed = 0;
    this.checksum = FNV_OFFSET;
    this.sentInput = [];
  }

  get inputLog(): readonly string[] {
    return this.sentInput;
  }

  private send(data: string | Uint8Array): void {
    if (statsEnabled() && this.sentInput.length < 1000) this.sentInput.push(toHex(data));
    void this.session?.input(data);
  }

  private startAttach(): void {
    const previous = this.session;
    this.session = null;
    if (previous !== null) void previous.close();
    this.attachDone = false;
    this.streamEnded = false;
    const generation = ++this.generation;
    const local = this.surface.geometry.local;
    const focused = this.surface.hasFocus();
    this.openSession(
      this.agentId,
      {
        cols: local.cols,
        rows: local.rows,
        focused,
        // Every attach replays the daemon's whole buffer; only the first lands in an empty terminal.
        resetBeforeReplay: previous !== null || this.hasOutput,
        ...(this.options.machineId === undefined ? {} : { machineId: this.options.machineId }),
      },
      {
        onOutput: (bytes) => {
          if (generation === this.generation) this.receive(bytes);
        },
        onEvent: (event) => {
          if (generation === this.generation) this.handleEvent(event);
        },
      },
    ).then(
      (session) => {
        if (this.disposed || generation !== this.generation) {
          void session.close();
          return;
        }
        this.session = session;
        // The local size or focus may have moved while the session opened.
        const now = this.surface.geometry.local;
        if (now.cols !== local.cols || now.rows !== local.rows) {
          void session.resize(now.cols, now.rows);
        }
        if (!focused && this.surface.hasFocus()) void session.claimSize();
      },
      (error: unknown) => {
        if (generation !== this.generation) return;
        console.warn(`PurePoint: attach to ${this.agentId} failed`, error);
        this.attachDone = true;
      },
    );
  }

  private handleEvent(event: AttachEvent): void {
    switch (event.type) {
      case 'reset':
      case 'resync':
        this.surface.reset();
        break;
      case 'pty_size':
        this.surface.geometry.report({ cols: event.cols, rows: event.rows });
        this.surface.renderGrid(this.surface.geometry.render);
        break;
      case 'live':
        this.removeSpinner();
        break;
      case 'exited':
        this.attachDone = true;
        this.streamEnded = true;
        this.removeSpinner();
        break;
      case 'gone':
        this.attachDone = true;
        this.agentGone = true;
        this.stopHeartbeat();
        this.removeSpinner();
        this.options.onGone?.();
        break;
      case 'failed':
        this.attachDone = true;
        break;
      case 'attach_ready':
      case 'retrying':
        break;
    }
  }

  private receive(bytes: Uint8Array): void {
    if (bytes.length === 0) return;
    if (!this.hasOutput) {
      this.hasOutput = true;
      this.removeSpinner();
    }
    this.bytesReceived += bytes.length;
    if (!this.shown && Date.now() >= this.resizeRedrawSettlesAt) {
      this.options.onBackgroundOutput?.();
    }
    if (statsEnabled()) {
      this.surface.write(bytes, () => {
        this.bytesParsed += bytes.length;
        this.checksum = fnv1a(this.checksum, bytes);
      });
    } else {
      this.surface.write(bytes);
    }
  }

  private startHeartbeat(): void {
    if (this.heartbeat !== null) return;
    this.heartbeat = setInterval(() => {
      this.reconnectIfNeeded();
    }, this.options.heartbeatMs ?? HEARTBEAT_MS);
  }

  private stopHeartbeat(): void {
    if (this.heartbeat !== null) clearInterval(this.heartbeat);
    this.heartbeat = null;
  }

  private showSpinner(): void {
    if (this.hasOutput || this.spinner !== null) return;
    const spinner = document.createElement('div');
    spinner.className = 'terminal-spinner';
    spinner.setAttribute('role', 'progressbar');
    spinner.setAttribute('aria-label', 'Connecting to agent');
    this.element.appendChild(spinner);
    this.spinner = spinner;
    this.spinnerShownAt = Date.now();
  }

  private removeSpinner(): void {
    const spinner = this.spinner;
    if (spinner === null) return;
    this.spinner = null;
    const remaining = SPINNER_MIN_MS - (Date.now() - this.spinnerShownAt);
    if (remaining <= 0) spinner.remove();
    else
      setTimeout(() => {
        spinner.remove();
      }, remaining);
  }

  /** `tearDown`. */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.shown = false;
    this.generation++;
    this.stopHeartbeat();
    this.resizeObserver?.disconnect();
    this.resizeObserver = null;
    this.spinner?.remove();
    this.spinner = null;
    const session = this.session;
    this.session = null;
    if (session !== null) void session.close();
    this.surface.dispose();
    this.element.remove();
  }
}
