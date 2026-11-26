// TerminalPaneController's attach wiring (WIN-7.3), against a fake terminal and attach service:
// output, reset, the PTY size from another client, resize, focus/blur, reattach and teardown.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type {
  AttachEvent,
  AttachHandlers,
  AttachOpenOptions,
  AttachSession,
} from '../../services/attach.ts';
import {
  type GridSize,
  type ScrollableTerminalOptions,
  TerminalGeometry,
} from './scrollableTerminal.ts';
import { TerminalPaneController, type TerminalSurface } from './terminalPaneView.ts';

class FakeSurface implements TerminalSurface {
  readonly element = document.createElement('div');
  readonly geometry = new TerminalGeometry();
  isOpen = false;
  renderer = 'dom' as const;
  written: Uint8Array[] = [];
  resets = 0;
  focused = false;
  size: GridSize | null = { cols: 100, rows: 30 };
  rendered: GridSize | null = null;
  disposed = false;
  readonly options: ScrollableTerminalOptions;
  constructor(options: ScrollableTerminalOptions) {
    this.options = options;
  }
  open() {
    this.isOpen = true;
  }
  write(data: Uint8Array, parsed?: () => void) {
    this.written.push(data);
    parsed?.();
  }
  reset() {
    this.resets++;
  }
  focus() {
    this.focused = true;
    this.options.onFocus();
  }
  blurNow() {
    this.focused = false;
    this.options.onBlur();
  }
  hasFocus() {
    return this.focused;
  }
  measure() {
    return this.size;
  }
  renderGrid(size: GridSize) {
    this.rendered = size;
  }
  refresh() {}
  setFontSize() {}
  applyTheme() {}
  dispose() {
    this.disposed = true;
  }
}

interface FakeSession extends AttachSession {
  handlers: AttachHandlers;
  options: AttachOpenOptions;
  calls: string[];
}

function harness(extra: { isAgentAlive?: () => boolean } = {}) {
  const sessions: FakeSession[] = [];
  const surfaces: FakeSurface[] = [];
  const open = vi.fn((agentId: string, options: AttachOpenOptions, handlers: AttachHandlers) => {
    const calls: string[] = [];
    const session: FakeSession = {
      id: sessions.length + 1,
      agentId,
      ended: null,
      handlers,
      options,
      calls,
      input: (data) => {
        calls.push(`input:${typeof data === 'string' ? data : Array.from(data).join(',')}`);
        return Promise.resolve();
      },
      deliver: () => Promise.resolve(true),
      resize: (cols, rows) => {
        calls.push(`resize:${String(cols)}x${String(rows)}`);
        return Promise.resolve();
      },
      claimSize: () => {
        calls.push('claim');
        return Promise.resolve();
      },
      blur: () => {
        calls.push('blur');
        return Promise.resolve();
      },
      close: () => {
        calls.push('close');
        return Promise.resolve();
      },
    };
    sessions.push(session);
    return Promise.resolve(session);
  });
  const onGone = vi.fn();
  const onBackgroundOutput = vi.fn();
  const pane = new TerminalPaneController('ag-1', {
    open,
    createSurface: (options) => {
      const created = new FakeSurface(options);
      surfaces.push(created);
      return created;
    },
    onGone,
    onBackgroundOutput,
    heartbeatMs: 5000,
    ...extra,
  });
  const surface = surfaces[0];
  if (surface === undefined) throw new Error('no surface');
  const host = document.createElement('div');
  const parking = document.createElement('div');
  return {
    pane,
    surface,
    sessions,
    open,
    host,
    parking,
    onGone,
    onBackgroundOutput,
  };
}

function emit(session: FakeSession | undefined, event: AttachEvent) {
  session?.handlers.onEvent?.(event);
}

async function flush() {
  await Promise.resolve();
  await Promise.resolve();
}

describe('TerminalPaneController', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('opens the terminal and attaches at its measured size on the first layout', async () => {
    const { pane, surface, open, host } = harness();
    expect(open).not.toHaveBeenCalled();
    pane.mount(host);
    expect(surface.isOpen).toBe(true);
    expect(host.contains(pane.element)).toBe(true);
    expect(open).toHaveBeenCalledTimes(1);
    expect(open.mock.calls[0]?.[1]).toMatchObject({
      cols: 100,
      rows: 30,
      focused: false,
      resetBeforeReplay: false,
    });
    expect(surface.rendered).toEqual({ cols: 100, rows: 30 });
    await flush();
    expect(pane.isAttached).toBe(true);
  });

  it('waits for a real size before attaching (the 1-column guard)', () => {
    const { pane, surface, open, host } = harness();
    surface.size = null;
    pane.mount(host);
    expect(open).not.toHaveBeenCalled();
    surface.size = { cols: 80, rows: 24 };
    pane.layout();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('writes output bytes as they are and resets before a replay', async () => {
    const { pane, surface, sessions, host } = harness();
    pane.mount(host);
    await flush();
    const session = sessions[0];
    session?.handlers.onOutput(new Uint8Array([0x1b, 0x5b, 0x41]));
    expect(surface.written).toEqual([new Uint8Array([0x1b, 0x5b, 0x41])]);
    emit(session, { type: 'reset' });
    emit(session, { type: 'resync' });
    expect(surface.resets).toBe(2);
    expect(pane.stats.bytesReceived).toBe(3);
  });

  it('sends keystrokes and binary reports as input', async () => {
    const { pane, surface, sessions, host } = harness();
    pane.mount(host);
    await flush();
    surface.options.onData('\x03');
    surface.options.onBinary('\x1b[M\xff');
    expect(sessions[0]?.calls).toEqual(['input:\x03', 'input:27,91,77,255']);
  });

  it("renders another client's PTY size without changing its own", async () => {
    const { pane, surface, sessions, host } = harness();
    pane.mount(host);
    await flush();
    emit(sessions[0], {
      type: 'pty_size',
      cols: 70,
      rows: 20,
      ownerClientId: 'mac:x',
      owned: false,
    });
    expect(surface.rendered).toEqual({ cols: 70, rows: 20 });
    expect(surface.geometry.local).toEqual({ cols: 100, rows: 30 });
    // A pane resize still asks for the local size.
    surface.size = { cols: 120, rows: 40 };
    pane.layout();
    expect(sessions[0]?.calls).toContain('resize:120x40');
    expect(surface.rendered).toEqual({ cols: 70, rows: 20 });
  });

  it('claims the PTY size on focus and gives it up on blur', async () => {
    const { pane, surface, sessions, host } = harness();
    pane.mount(host);
    await flush();
    pane.focus();
    surface.blurNow();
    expect(sessions[0]?.calls).toEqual(['claim', 'blur']);
  });

  it('claims the size once the session opens if focus arrived first', async () => {
    const { pane, sessions, host } = harness();
    pane.mount(host);
    pane.focus();
    await flush();
    expect(sessions[0]?.calls).toContain('claim');
  });

  it('reattaches a failed session on the heartbeat, resetting before the replay', async () => {
    const { pane, sessions, open, host } = harness();
    pane.mount(host);
    await flush();
    sessions[0]?.handlers.onOutput(new Uint8Array([0x41]));
    emit(sessions[0], { type: 'failed', message: 'pipe closed' });
    vi.advanceTimersByTime(5000);
    expect(open).toHaveBeenCalledTimes(2);
    expect(open.mock.calls[1]?.[1]).toMatchObject({ resetBeforeReplay: true });
    expect(sessions[0]?.calls).toContain('close');
  });

  it('does not reattach after the agent exited unless it runs again', async () => {
    let alive = false;
    const { pane, sessions, open, host } = harness({ isAgentAlive: () => alive });
    pane.mount(host);
    await flush();
    emit(sessions[0], { type: 'exited' });
    vi.advanceTimersByTime(15_000);
    expect(open).toHaveBeenCalledTimes(1);
    alive = true;
    vi.advanceTimersByTime(5000);
    expect(open).toHaveBeenCalledTimes(2);
  });

  it('reports a gone agent and stops the heartbeat', async () => {
    const { pane, sessions, open, host, onGone } = harness({ isAgentAlive: () => true });
    pane.mount(host);
    await flush();
    emit(sessions[0], { type: 'gone' });
    expect(pane.isAgentGone).toBe(true);
    expect(onGone).toHaveBeenCalledTimes(1);
    vi.advanceTimersByTime(30_000);
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('drops messages from a superseded session', async () => {
    const { pane, surface, sessions, host } = harness();
    pane.mount(host);
    await flush();
    emit(sessions[0], { type: 'failed', message: 'x' });
    pane.reconnectIfNeeded();
    await flush();
    sessions[0]?.handlers.onOutput(new Uint8Array([1]));
    expect(surface.written).toHaveLength(0);
    sessions[1]?.handlers.onOutput(new Uint8Array([2]));
    expect(surface.written).toEqual([new Uint8Array([2])]);
  });

  it('notes output that arrives while parked', async () => {
    const { pane, sessions, host, parking, onBackgroundOutput } = harness();
    pane.mount(host);
    await flush();
    vi.advanceTimersByTime(2000); // past the resize redraw
    pane.park(parking);
    expect(parking.contains(pane.element)).toBe(true);
    sessions[0]?.handlers.onOutput(new Uint8Array([1]));
    expect(onBackgroundOutput).toHaveBeenCalledTimes(1);
  });

  it('closes the session and the terminal on dispose', async () => {
    const { pane, surface, sessions, host } = harness();
    pane.mount(host);
    await flush();
    pane.dispose();
    expect(sessions[0]?.calls).toContain('close');
    expect(surface.disposed).toBe(true);
    expect(host.contains(pane.element)).toBe(false);
    vi.advanceTimersByTime(10_000);
    expect(sessions).toHaveLength(1);
  });
});
