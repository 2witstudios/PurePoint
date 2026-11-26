// Front end of the Point Guard chat transport (WIN-9.4): `src-tauri/src/pointguard/chat.rs`
// and `recovery.rs`.
//
// The bridge's v1 chat socket authenticates only by the upgrade's `Authorization: Bearer
// <secret>` and `X-PointGuard-Client-ID` headers and refuses any `Origin`, so a page cannot
// open it. The backend holds the socket and relays each text record over a Tauri Channel;
// the chat state (`state/piChat.ts`) sees only the {@link PiChatTransport} interface, which
// tests replace with a Node `ws` client against the real bridge.
import { Channel, invoke } from '@tauri-apps/api/core';

/** Why a socket ended without being closed by the page. */
export interface PiSocketClose {
  /** The close frame's code (1009 backlog, 4001 revoked), or null for a transport failure. */
  code: number | null;
  reason: string;
}

export interface PiSocketHandlers {
  onMessage: (text: string) => void;
  onClose: (close: PiSocketClose) => void;
}

/** One open chat socket. */
export interface PiSocket {
  send(text: string): Promise<void>;
  /** Close with 1001 (going away). No `onClose` follows. */
  close(): void;
}

export interface PiSocketCredentials {
  endpoint: string;
  secret: string;
  clientId: string;
}

/** Opens chat sockets with the Mac's headers. */
export interface PiChatTransport {
  open(credentials: PiSocketCredentials, handlers: PiSocketHandlers): Promise<PiSocket>;
}

type BackendEvent =
  { type: 'message'; text: string } | { type: 'closed'; code: number | null; reason: string };

/** The backend-held socket. */
export const tauriChatTransport: PiChatTransport = {
  async open({ endpoint, secret, clientId }, handlers) {
    let closed = false;
    const channel = new Channel<BackendEvent>();
    channel.onmessage = (event) => {
      if (closed) return;
      if (event.type === 'message') handlers.onMessage(event.text);
      else {
        closed = true;
        handlers.onClose({ code: event.code, reason: event.reason });
      }
    };
    const socketId = await invoke<number>('pointguard_chat_open', {
      endpoint,
      secret,
      clientId,
      onEvent: channel,
    });
    return {
      send: (text) => invoke('pointguard_chat_send', { socketId, text }),
      close: () => {
        closed = true;
        void invoke('pointguard_chat_close', { socketId }).catch(() => {});
      },
    };
  },
};

// MARK: - Pairing secrets (the Swift PiPairingSecret)

/** Where the pairing secret for an endpoint lives (`services/pairingSecret.ts`). */
export interface PiSecretStore {
  /** The saved secret, or '' when there is none. */
  read(endpoint: string): Promise<string>;
  save(secret: string, endpoint: string): Promise<void>;
}

/** Secrets kept for this run only, for previews and tests. */
export function memorySecretStore(): PiSecretStore & { secrets: Map<string, string> } {
  const secrets = new Map<string, string>();
  return {
    secrets,
    read: (endpoint) => Promise.resolve(secrets.get(endpoint) ?? ''),
    save: (secret, endpoint) => {
      secrets.set(endpoint, secret);
      return Promise.resolve();
    },
  };
}

// MARK: - Recovery files (the Swift PiLocalRecoveryStore)

export type PiRecoveryName = 'attachment-drafts' | 'submissions';

/** The chat's private recovery files. */
export interface PiRecoveryStore {
  load(name: PiRecoveryName): Promise<string | null>;
  save(name: PiRecoveryName, json: string): Promise<void>;
}

export const tauriRecoveryStore: PiRecoveryStore = {
  load: (name) => invoke<string | null>('pointguard_recovery_load', { name }),
  save: async (name, json) => {
    await invoke('pointguard_recovery_save', { name, json });
  },
};

/** An in-memory store, for previews and tests. */
export function memoryRecoveryStore(
  initial: Partial<Record<PiRecoveryName, string>> = {},
): PiRecoveryStore & { files: Map<PiRecoveryName, string>; failing: boolean } {
  const files = new Map(Object.entries(initial) as [PiRecoveryName, string][]);
  const store = {
    files,
    failing: false,
    load: (name: PiRecoveryName) => Promise.resolve(files.get(name) ?? null),
    save: (name: PiRecoveryName, json: string) => {
      if (store.failing) return Promise.reject(new Error('disk full'));
      files.set(name, json);
      return Promise.resolve();
    },
  };
  return store;
}

// MARK: - Coalesced writes (the Swift PiCoalescedWriter)

/**
 * Serial writes off the keystroke path. A pending value for a key is replaced by a newer one;
 * writes run one at a time, so an older value never lands after a newer one.
 */
export class PiCoalescedWriter {
  private readonly pending = new Map<string, () => Promise<void> | void>();
  private readonly timers = new Map<string, ReturnType<typeof setTimeout>>();
  private chain: Promise<void> = Promise.resolve();

  constructor(private readonly delayMs = 250) {}

  private run(work: () => Promise<void> | void): Promise<void> {
    const next = this.chain.then(async () => {
      await work();
    });
    this.chain = next.catch(() => {});
    return next;
  }

  schedule(key: string, work: () => Promise<void> | void): void {
    const already = this.pending.has(key);
    this.pending.set(key, work);
    if (already) return;
    this.timers.set(
      key,
      setTimeout(() => {
        this.timers.delete(key);
        const action = this.pending.get(key);
        this.pending.delete(key);
        if (action) void this.run(action).catch(() => {});
      }, this.delayMs),
    );
  }

  /** Run every pending write now; resolves once they (and earlier writes) finished. */
  flush(): Promise<void> {
    for (const timer of this.timers.values()) clearTimeout(timer);
    this.timers.clear();
    const work = [...this.pending.values()];
    this.pending.clear();
    for (const action of work) void this.run(action).catch(() => {});
    return this.chain;
  }

  /** Drain older coalesced values, then run `work`, rejecting with its failure. */
  async writeAndConfirm(work: () => Promise<void> | void): Promise<void> {
    void this.flush();
    await this.run(work);
  }
}
