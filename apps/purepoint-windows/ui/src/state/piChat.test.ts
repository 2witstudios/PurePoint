// The Point Guard chat's recovery paths (WIN-9.4) over a scripted socket: lost and missing
// receipts, failed persistence, saved recovery across restarts, stale snapshots and records
// the contract does not define. The contract itself is exercised against the real bridge in
// piChat.bridge.test.ts.
import { afterEach, describe, expect, it, vi } from 'vitest';
import type { PiSnapshot, PiSubmission } from '../model/piChatDomain';
import {
  memoryRecoveryStore,
  type PiChatTransport,
  type PiSocketHandlers,
} from '../services/pointGuardChat';
import {
  memorySecretStore,
  PiChatModel,
  pointGuardChatBinding,
  retainedSubmissions,
  type PiPreferences,
} from './piChat';

const ENDPOINT = 'ws://127.0.0.1:4100/v1';
const SECRET = 's'.repeat(40);
const CLIENT = '6F9619FF-8B86-D011-B42D-00C04FC964FF';

function snapshot(patch: Partial<PiSnapshot> = {}): PiSnapshot {
  return {
    version: 1,
    epoch: 'e1',
    revision: 1,
    busy: false,
    runId: null,
    sessionId: 's1',
    title: 'Point Guard',
    messages: [],
    tools: [],
    queue: [],
    dialogs: [],
    notices: [],
    error: null,
    editor: null,
    canceled: [],
    capabilities: ['images'],
    ...patch,
  };
}

interface Sent {
  op: string;
  id: string;
  [key: string]: unknown;
}

/** A socket the test answers by hand; `sync` is answered with `state` unless told otherwise. */
class ScriptedBridge {
  handlers: PiSocketHandlers | null = null;
  sent: Sent[] = [];
  opens = 0;
  closes = 0;
  refuse: string | null = null;
  autoSync = true;
  state = snapshot();
  readonly transport: PiChatTransport = {
    open: (_credentials, handlers) => {
      this.opens++;
      if (this.refuse !== null) return Promise.reject(new Error(this.refuse));
      this.handlers = handlers;
      return Promise.resolve({
        send: (text: string) => {
          const record = JSON.parse(text) as Sent;
          this.sent.push(record);
          if (record.op === 'sync' && this.autoSync)
            queueMicrotask(() => {
              this.receipt(record.id, { type: 'snapshot', ...this.state });
            });
          return Promise.resolve();
        },
        close: () => {
          this.closes++;
          this.handlers = null;
        },
      });
    },
  };
  push(record: unknown): void {
    this.handlers?.onMessage(JSON.stringify(record));
  }
  receipt(id: string, data: unknown = {}, ok = true, error?: string): void {
    this.push({ type: 'receipt', id, ok, data, ...(error === undefined ? {} : { error }) });
  }
  last(op: string): Sent | undefined {
    return this.sent.filter((x) => x.op === op).at(-1);
  }
}

function memoryPreferences(): PiPreferences {
  const values = new Map<string, string>();
  return { getItem: (k) => values.get(k) ?? null, setItem: (k, v) => values.set(k, v) };
}

async function connected(
  bridge = new ScriptedBridge(),
  recovery = memoryRecoveryStore(),
  options: { receiptTimeoutMs?: number } = {},
) {
  const chat = new PiChatModel({
    transport: bridge.transport,
    recovery,
    secrets: memorySecretStore({ [ENDPOINT]: SECRET }),
    preferences: memoryPreferences(),
    writerDelayMs: 0,
    retryUnitMs: 5,
    ...options,
  });
  chat.bindManagedConnection(ENDPOINT, CLIENT);
  await vi.waitFor(() => {
    expect(chat.connected).toBe(true);
  });
  return { chat, bridge, recovery };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('PiChatModel connection', () => {
  it('given the managed hooks should bind and drop the local chat', async () => {
    const bridge = new ScriptedBridge();
    const chat = new PiChatModel({
      transport: bridge.transport,
      recovery: memoryRecoveryStore(),
      secrets: memorySecretStore({ [ENDPOINT]: SECRET }),
      preferences: memoryPreferences(),
    });
    const hooks = pointGuardChatBinding(chat);
    hooks.onConnection({ endpoint: ENDPOINT, clientId: CLIENT });
    await vi.waitFor(() => {
      expect(chat.connected).toBe(true);
    });
    expect(bridge.sent[0]).toMatchObject({ version: 1, op: 'sync', clientId: CLIENT });
    hooks.onDisconnect();
    expect(chat.connected).toBe(false);
    expect(chat.connectionStatus).toBe('Not connected');
    expect(bridge.closes).toBe(1);
  });

  it('given an invalid identity, address or secret should refuse to connect', async () => {
    const bridge = new ScriptedBridge();
    const chat = new PiChatModel({
      transport: bridge.transport,
      recovery: memoryRecoveryStore(),
      secrets: memorySecretStore({ [ENDPOINT]: SECRET }),
      preferences: memoryPreferences(),
    });
    chat.bindManagedConnection(ENDPOINT, 'not-a-uuid');
    expect(chat.error).toBe('Invalid managed desktop identity.');
    await chat.pair('ws://8.8.8.8:1/v1', SECRET);
    expect(chat.error).toContain('ending in /v1');
    await chat.pair(ENDPOINT, 'short');
    expect(chat.error).toContain('at least 32 characters');
    expect(bridge.opens).toBe(0);
  });

  it('given no saved secret should ask for one and open nothing', async () => {
    const bridge = new ScriptedBridge();
    const chat = new PiChatModel({
      transport: bridge.transport,
      recovery: memoryRecoveryStore(),
      secrets: memorySecretStore(),
      preferences: memoryPreferences(),
    });
    chat.bindManagedConnection(ENDPOINT, CLIENT);
    await vi.waitFor(() => {
      expect(chat.error).toBe('Add the pairing secret in Connection settings.');
    });
    expect(bridge.opens).toBe(0);
  });

  it('given a refused connection should back off and retry', async () => {
    const bridge = new ScriptedBridge();
    bridge.refuse = 'Point Guard rejected the pairing secret.';
    const chat = new PiChatModel({
      transport: bridge.transport,
      recovery: memoryRecoveryStore(),
      secrets: memorySecretStore({ [ENDPOINT]: SECRET }),
      preferences: memoryPreferences(),
      retryUnitMs: 5,
    });
    chat.bindManagedConnection(ENDPOINT, CLIENT);
    await vi.waitFor(() => {
      expect(bridge.opens).toBeGreaterThanOrEqual(3);
    });
    expect(chat.error).toContain('Point Guard rejected the pairing secret.');
    bridge.refuse = null;
    await vi.waitFor(() => {
      expect(chat.connected).toBe(true);
    });
    chat.disconnect();
  });

  it('given a revoked socket (4001) should report it and reconnect', async () => {
    const { chat, bridge } = await connected();
    bridge.handlers?.onClose({ code: 4001, reason: 'Device trust revoked; re-pair deliberately' });
    expect(chat.connected).toBe(false);
    expect(chat.error).toContain('Device trust revoked');
    await vi.waitFor(() => {
      expect(bridge.opens).toBe(2);
      expect(chat.connected).toBe(true);
    });
    chat.disconnect();
  });

  it('given a record the contract does not define should drop the connection', async () => {
    const { chat, bridge } = await connected();
    bridge.push({ type: 'mystery' });
    expect(chat.connected).toBe(false);
    expect(chat.error).toContain('Unsupported bridge response');
    chat.disconnect();
  });

  it('given a snapshot of another contract version should drop the connection', async () => {
    const { chat, bridge } = await connected();
    bridge.push({ type: 'snapshot', ...snapshot({ version: 2, revision: 9 }) });
    expect(chat.error).toContain('Unsupported bridge version');
    chat.disconnect();
  });

  it('given an older revision of the same epoch should keep the newer snapshot', async () => {
    const { chat, bridge } = await connected();
    bridge.push({ type: 'snapshot', ...snapshot({ revision: 5, title: 'Newer' }) });
    bridge.push({ type: 'snapshot', ...snapshot({ revision: 4, title: 'Older' }) });
    expect(chat.snapshot?.title).toBe('Newer');
    bridge.push({
      type: 'snapshot',
      ...snapshot({ epoch: 'e2', revision: 1, title: 'New epoch' }),
    });
    expect(chat.snapshot?.title).toBe('New epoch');
    chat.disconnect();
  });

  it('given a repeated editor offer should offer it once', async () => {
    const { chat, bridge } = await connected();
    bridge.push({ type: 'editor', id: 'offer-1', text: 'Try this' });
    expect(chat.store.getState().editorOffer).toBe('Try this');
    chat.dismissEditorOffer();
    bridge.push({
      type: 'snapshot',
      ...snapshot({ revision: 2, editor: { id: 'offer-1', text: 'Try this' } }),
    });
    expect(chat.store.getState().editorOffer).toBeNull();
    chat.disconnect();
  });

  it('given more than sixteen actions in flight should refuse another', async () => {
    const { chat } = await connected();
    for (let i = 0; i < 16; i++) void chat.loadConversations();
    await chat.loadConversations();
    expect(chat.error).toBe(
      'Too many actions in flight. Wait for Point Guard before trying again.',
    );
    chat.disconnect();
  });
});

describe('PiChatModel submission recovery', () => {
  it('given no receipt should mark the message recoverable as delivery uncertain', async () => {
    const { chat, bridge } = await connected(undefined, undefined, { receiptTimeoutMs: 20 });
    chat.setDraft('Did this arrive?');
    await chat.submit('send');
    expect(bridge.last('send')).toMatchObject({
      text: 'Did this arrive?',
      mode: 'send',
      epoch: 'e1',
    });
    expect(chat.recoverable).toHaveLength(1);
    expect(chat.recoverable[0]?.status).toContain('Delivery is uncertain');
    expect(chat.draft).toBe('');
    chat.disconnect();
  });

  it('given the connection lost mid-send should keep the message for restore', async () => {
    const { chat, bridge } = await connected();
    chat.setDraft('Halfway there');
    const sending = chat.submit('send');
    await vi.waitFor(() => {
      expect(bridge.last('send')).toBeDefined();
    });
    expect(chat.submissions.at(-1)?.status).toBe('Sending');
    chat.dismissSubmission(chat.submissions.at(-1)?.id ?? '');
    expect(chat.error).toContain("Wait for this message's delivery result");
    bridge.handlers?.onClose({ code: null, reason: 'socket reset' });
    await sending;
    expect(chat.recoverable[0]).toMatchObject({ text: 'Halfway there', recoverable: true });
    expect(chat.recoverable[0]?.status).toContain('Connection interrupted');
    const [saved] = chat.recoverable;
    if (saved) chat.restore(saved);
    expect(chat.draft).toBe('Halfway there');
    expect(chat.submissions).toEqual([]);
    chat.disconnect();
  });

  it('given a rejected send should keep it recoverable with the bridge error', async () => {
    const { chat, bridge } = await connected();
    chat.setDraft('Too soon');
    const sending = chat.submit('send');
    await vi.waitFor(() => {
      expect(bridge.last('send')).toBeDefined();
    });
    bridge.receipt(
      bridge.last('send')?.id ?? '',
      null,
      false,
      'Point Guard is running. Choose Steer or After reply.',
    );
    await sending;
    expect(chat.recoverable[0]?.status).toBe(
      'Point Guard is running. Choose Steer or After reply.',
    );
    expect(chat.error).toBe('Point Guard is running. Choose Steer or After reply.');
    chat.disconnect();
  });

  it('given recovery that cannot be saved should not send the message', async () => {
    const recovery = memoryRecoveryStore();
    const { chat, bridge } = await connected(undefined, recovery);
    recovery.failing = true;
    chat.setDraft('Keep me safe');
    await chat.submit('send');
    expect(bridge.last('send')).toBeUndefined();
    expect(chat.error).toMatch(/^Message not sent: could not save recovery on this PC\./);
    expect(chat.recoverable[0]?.text).toBe('Keep me safe');
    chat.disconnect();
  });

  it('given a saved in-flight message after a restart should offer it as delivery uncertain', async () => {
    const saved: PiSubmission[] = [
      { id: 'a', text: 'Was sending', sessionId: 's1', status: 'Sending', recoverable: false },
      { id: 'b', text: 'Done', sessionId: 's1', status: 'Accepted', recoverable: false },
    ];
    const recovery = memoryRecoveryStore({ submissions: JSON.stringify(saved) });
    const chat = new PiChatModel({
      transport: new ScriptedBridge().transport,
      recovery,
      preferences: memoryPreferences(),
      writerDelayMs: 0,
    });
    await chat.loaded;
    expect(chat.submissions.map((s) => [s.id, s.status, s.recoverable])).toEqual([
      ['a', 'Delivery uncertain', true],
      ['b', 'Accepted', false],
    ]);
  });

  it('given damaged recovery files should start empty', async () => {
    const recovery = memoryRecoveryStore({
      submissions: '{not json',
      'attachment-drafts': JSON.stringify({ key: [{ id: 1 }] }),
    });
    const chat = new PiChatModel({
      transport: new ScriptedBridge().transport,
      recovery,
      preferences: memoryPreferences(),
    });
    await chat.loaded;
    expect(chat.submissions).toEqual([]);
    expect(chat.attachments).toEqual([]);
  });

  it('given a queued send should record it as queued and a handled one as handled', async () => {
    const { chat, bridge } = await connected();
    bridge.push({ type: 'snapshot', ...snapshot({ revision: 2, busy: true, runId: 'r1' }) });
    chat.setDraft('Next');
    const queued = chat.submit('after');
    await vi.waitFor(() => {
      expect(bridge.last('send')).toBeDefined();
    });
    bridge.receipt(bridge.last('send')?.id ?? '', { disposition: 'queued' });
    await queued;
    expect(chat.submissions.at(-1)?.status).toBe('Queued');
    bridge.push({ type: 'snapshot', ...snapshot({ revision: 3 }) });
    chat.setDraft('/handled');
    const handled = chat.submit('send');
    await vi.waitFor(() => {
      expect(bridge.sent.filter((x) => x.op === 'send')).toHaveLength(2);
    });
    bridge.receipt(bridge.last('send')?.id ?? '', { disposition: 'handled' });
    await handled;
    expect(chat.submissions.at(-1)?.status).toBe('Handled by extension');
    chat.disconnect();
  });

  it('given a bridge without image support should refuse to send images', async () => {
    const bridge = new ScriptedBridge();
    bridge.state = snapshot({ capabilities: null });
    const { chat } = await connected(bridge);
    chat.addAttachment({ id: 'i', name: 'a.png', mimeType: 'image/png', data: 'iVBORw0K' });
    await chat.submit('send');
    expect(chat.error).toBe('Restart the updated Point Guard bridge before sending images.');
    expect(bridge.last('send')).toBeUndefined();
    chat.disconnect();
  });

  it('given a canceled message from another client should not offer it here', async () => {
    const { chat, bridge } = await connected();
    bridge.push({
      type: 'snapshot',
      ...snapshot({
        revision: 2,
        canceled: [
          { id: 'x:1', clientId: 'phone', text: 'Theirs', sessionId: 's1' },
          { id: 'y:1', clientId: CLIENT, text: 'Mine', sessionId: 's1' },
        ],
      }),
    });
    expect(chat.recoverable.map((s) => s.text)).toEqual(['Mine']);
    chat.disconnect();
  });

  it('given more than fifty receipts should keep in-flight and recovery ahead of settled ones', () => {
    const items: PiSubmission[] = [
      ...Array.from({ length: 30 }, (_, i) => ({
        id: `settled-${String(i)}`,
        text: '',
        sessionId: 's',
        status: 'Accepted',
        recoverable: false,
        attachments: [{ id: 'f', name: 'f', mimeType: 'text/plain', data: '' }],
      })),
      ...Array.from({ length: 30 }, (_, i) => ({
        id: `saved-${String(i)}`,
        text: '',
        sessionId: 's',
        status: 'Canceled by Stop',
        recoverable: true,
      })),
      { id: 'sending', text: '', sessionId: 's', status: 'Sending', recoverable: false },
    ];
    const kept = retainedSubmissions(items);
    expect(kept).toHaveLength(50);
    expect(kept.filter((x) => x.recoverable)).toHaveLength(30);
    expect(kept.at(-1)?.id).toBe('sending');
    expect(kept[0]?.id).toBe('settled-11');
    expect(kept[0]?.attachments).toBeNull();
  });
});

describe('PiChatModel preview', () => {
  it('given the demo should reply without a bridge', async () => {
    const chat = new PiChatModel({
      transport: new ScriptedBridge().transport,
      recovery: memoryRecoveryStore(),
      preferences: memoryPreferences(),
      demoDelayMs: 1,
      writerDelayMs: 0,
    });
    chat.exploreDemo();
    expect(chat.connectionStatus).toBe('Preview');
    chat.setDraft('Hi');
    await chat.submit('send');
    expect(chat.submissions.at(-1)?.status).toBe('Accepted');
    expect(chat.snapshot?.messages.map((m) => m.role)).toEqual(['assistant', 'user', 'assistant']);
    await chat.loadConversations();
    expect(chat.store.getState().conversations).toHaveLength(1);
  });
});

describe('PiChatModel managed drafts', () => {
  it('given a relaunch onto a new managed port should recover the draft', async () => {
    const SECOND = 'ws://127.0.0.1:4200/v1';
    const preferences = memoryPreferences();
    const recovery = memoryRecoveryStore();
    const make = (bridge: ScriptedBridge) =>
      new PiChatModel({
        transport: bridge.transport,
        recovery,
        secrets: memorySecretStore({ [ENDPOINT]: SECRET, [SECOND]: SECRET }),
        preferences,
        writerDelayMs: 0,
      });
    const first = make(new ScriptedBridge());
    first.bindManagedConnection(ENDPOINT, CLIENT);
    await vi.waitFor(() => {
      expect(first.connected).toBe(true);
    });
    first.setDraft('Half a thought');
    await first.flush();
    first.disconnect();

    const second = make(new ScriptedBridge());
    second.bindManagedConnection(SECOND, CLIENT);
    await vi.waitFor(() => {
      expect(second.connected).toBe(true);
    });
    expect(second.draft).toBe('Half a thought');
    second.disconnect();
  });
});
