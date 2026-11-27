// Front end of the attach sessions (WIN-5.4): `src-tauri/src/attach/`, ported from
// Services/DaemonAttachSession.swift. Each terminal pane opens one session, which holds its
// own daemon connection. Output arrives over a Tauri Channel as raw bytes (an ArrayBuffer,
// never a JSON number array); every other message is an {@link AttachEvent}, in stream order.
import { Channel, invoke } from '@tauri-apps/api/core';

export type AttachSessionId = number;

/** What the backend reports besides output bytes (`session::AttachEvent`). */
export type AttachEvent =
  /** The daemon replays `bufferedBytes` before live output. */
  | { type: 'attach_ready'; bufferedBytes: number }
  /** Reset the terminal (ESC c) before the replay that follows. */
  | { type: 'reset' }
  /** The replay is done; output from here on is live. */
  | { type: 'live' }
  /** The daemon lost its place: reset the terminal and render what follows. */
  | { type: 'resync' }
  /** The PTY size the daemon applied; `owned` when this pane's session owns it. */
  | { type: 'pty_size'; cols: number; rows: number; ownerClientId: string | null; owned: boolean }
  /** The connection failed; the session reconnects after `delayMs`. */
  | { type: 'retrying'; attempt: number; delayMs: number; message: string }
  /** The agent exited and the stream ended; the session does not reattach. */
  | { type: 'exited' }
  /** The daemon no longer knows the agent. */
  | { type: 'gone' }
  /** The retries ran out. */
  | { type: 'failed'; message: string };

/** The session ended for good: exited, gone or failed. */
export type AttachEnd = Extract<AttachEvent, { type: 'exited' | 'gone' | 'failed' }>;

export interface AttachHandlers {
  /** PTY bytes, to write to xterm.js as they are. */
  onOutput: (bytes: Uint8Array) => void;
  onEvent?: (event: AttachEvent) => void;
}

export interface AttachOpenOptions {
  cols: number;
  rows: number;
  /** The pane has focus, so the session claims the PTY size when it attaches. */
  focused?: boolean;
  /** The terminal already shows output; reset it before the first replay. */
  resetBeforeReplay?: boolean;
  /** A remote machine's daemon instead of the local one. */
  machineId?: string;
}

/** One pane's attach session. */
export interface AttachSession {
  readonly id: AttachSessionId;
  readonly agentId: string;
  /** Set once the session ended for good. */
  readonly ended: AttachEnd | null;
  /** Keystrokes or pasted text; dropped while the connection is down. `submit` adds Enter. */
  input(data: string | Uint8Array, options?: { submit?: boolean }): Promise<void>;
  /** Input that must not be lost: waits up to 5 s for a connection; true once written. */
  deliver(data: string | Uint8Array): Promise<boolean>;
  /** The pane's new size in cells. */
  resize(cols: number, rows: number): Promise<void>;
  /** Call on pane focus only: claim the PTY size. */
  claimSize(): Promise<void>;
  /** The pane lost focus. */
  blur(): Promise<void>;
  close(): Promise<void>;
}

/** A channel message: raw output or a JSON event. */
export type AttachMessage = ArrayBuffer | Uint8Array | AttachEvent;

function wireData(data: string | Uint8Array): string | number[] {
  return typeof data === 'string' ? data : Array.from(data);
}

/**
 * Runs sends one after another: each starts once the one before it has settled. Tauri runs every
 * async command invocation as its own task on a multi-threaded runtime, so two `attach_input`
 * calls in flight together can reach the session in either order, and fast typing came out
 * transposed ("menu" as "meun"). The Mac writes keystrokes to its socket in order on one queue.
 */
function serialSender(): <T>(send: () => Promise<T>) => Promise<T> {
  let tail: Promise<unknown> = Promise.resolve();
  return <T>(send: () => Promise<T>): Promise<T> => {
    const result = tail.then(send, send);
    tail = result.catch(() => undefined);
    return result;
  };
}

function isEnd(event: AttachEvent): event is AttachEnd {
  return event.type === 'exited' || event.type === 'gone' || event.type === 'failed';
}

/** Route one channel message to the handlers. */
export function dispatchAttachMessage(message: AttachMessage, handlers: AttachHandlers): void {
  if (message instanceof ArrayBuffer) {
    handlers.onOutput(new Uint8Array(message));
  } else if (message instanceof Uint8Array) {
    handlers.onOutput(message);
  } else {
    handlers.onEvent?.(message);
  }
}

/** Open an attach session for `agentId`. */
export async function openAttach(
  agentId: string,
  options: AttachOpenOptions,
  handlers: AttachHandlers,
): Promise<AttachSession> {
  let ended: AttachEnd | null = null;
  const channel = new Channel<AttachMessage>((message) => {
    if (!(message instanceof ArrayBuffer) && !(message instanceof Uint8Array) && isEnd(message)) {
      ended = message;
    }
    dispatchAttachMessage(message, handlers);
  });
  const id = await invoke<AttachSessionId>('attach_open', {
    agentId,
    cols: options.cols,
    rows: options.rows,
    onEvent: channel,
    focused: options.focused ?? false,
    resetBeforeReplay: options.resetBeforeReplay ?? false,
    machineId: options.machineId ?? null,
  });
  // Keystrokes, pastes and delivered input reach the PTY in the order they were sent.
  const inOrder = serialSender();
  return {
    id,
    agentId,
    get ended() {
      return ended;
    },
    async input(data, inputOptions) {
      await inOrder(() =>
        invoke<boolean>('attach_input', {
          sessionId: id,
          data: wireData(data),
          submit: inputOptions?.submit ?? false,
          deliver: false,
        }),
      );
    },
    deliver(data) {
      return inOrder(() =>
        invoke<boolean>('attach_input', {
          sessionId: id,
          data: wireData(data),
          submit: false,
          deliver: true,
        }),
      );
    },
    async resize(cols, rows) {
      await invoke('attach_resize', { sessionId: id, cols, rows });
    },
    async claimSize() {
      await invoke('attach_claim_size', { sessionId: id });
    },
    async blur() {
      await invoke('attach_blur', { sessionId: id });
    },
    async close() {
      await invoke('attach_close', { sessionId: id });
    },
  };
}
