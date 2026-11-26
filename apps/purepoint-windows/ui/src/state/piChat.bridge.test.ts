// @vitest-environment node
// The Point Guard chat (WIN-9.4) against the real bridge: apps/purepoint-mobile/bridge's
// `network.js` listener with its loopback desktop principal and `controller.js`, driving the
// deterministic `fixture.js` Pi child (no credentials, provider calls or session files).
// The transport is a Node `ws` client sending the Mac's upgrade headers, as the backend's
// `tokio-tungstenite` socket does (src-tauri/src/pointguard/chat.rs, tested there).
import { randomBytes, randomUUID } from 'node:crypto';
import { mkdtempSync, rmSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import WebSocket from 'ws';
import { afterEach, describe, expect, it } from 'vitest';
import { bytesToBase64, type PiExtensionDialog } from '../model/piChatDomain';
import { memoryRecoveryStore, type PiChatTransport } from '../services/pointGuardChat';
import { memorySecretStore, PiChatModel, type PiPreferences } from './piChat';

const bridgeDir = fileURLToPath(new URL('../../../../purepoint-mobile/bridge/', import.meta.url));

interface BridgeServer {
  address(): { port: number };
  shutdown(): Promise<void>;
}
interface BridgeController {
  refresh(): Promise<unknown>;
  dispose(): void;
}
interface BridgeRpc {
  close(): Promise<void>;
}
interface Sessions {
  list(): Promise<unknown[]>;
  path(id: string): Promise<string>;
  history(id: string): Promise<unknown>;
}
interface BridgeModules {
  Rpc: new (executable: string, args: string[], cwd: string, env: NodeJS.ProcessEnv) => BridgeRpc;
  Controller: new (rpc: BridgeRpc, sessions: Sessions) => BridgeController;
  serve: (
    controller: BridgeController,
    options: { host: string; port: number; localAdmin: { token: string; clientId: string } },
  ) => Promise<BridgeServer>;
}

async function bridgeModules(): Promise<BridgeModules> {
  const load = (name: string) =>
    import(/* @vite-ignore */ pathToFileURL(path.join(bridgeDir, name)).href) as Promise<
      Record<string, unknown>
    >;
  const [rpc, controller, network] = await Promise.all([
    load('rpc.js'),
    load('controller.js'),
    load('network.js'),
  ]);
  return {
    Rpc: rpc.Rpc as BridgeModules['Rpc'],
    Controller: controller.Controller as BridgeModules['Controller'],
    serve: network.serve as BridgeModules['serve'],
  };
}

const sessions: Sessions = {
  list: () =>
    Promise.resolve([
      {
        id: 'fixture-history',
        name: 'A previous thought',
        firstMessage: 'Where should we begin?',
        modified: '2026-11-01T10:00:00.000Z',
      },
    ]),
  path: (id) => Promise.resolve(id),
  history: (id) =>
    Promise.resolve({
      sessionId: id,
      title: 'A previous thought',
      messages: [
        { id: 'h1', role: 'user', text: 'Where should we begin?' },
        { id: 'h2', role: 'assistant', text: 'With one clear idea.' },
      ],
    }),
};

interface Bridge {
  endpoint: string;
  secret: string;
  clientId: string;
  shutdown: () => Promise<void>;
}

const cleanups: (() => Promise<void> | void)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

async function startBridge(
  port = 0,
  credentials: { secret: string; clientId: string } = {
    secret: randomBytes(32).toString('base64url'),
    clientId: randomUUID(),
  },
): Promise<Bridge> {
  const { Rpc, Controller, serve } = await bridgeModules();
  // A private profile for the fixture child: it never reaches the real one.
  const home = mkdtempSync(path.join(os.tmpdir(), 'pg-chat-'));
  const env = {
    ...process.env,
    HOME: home,
    USERPROFILE: home,
    APPDATA: path.join(home, 'AppData', 'Roaming'),
    LOCALAPPDATA: path.join(home, 'AppData', 'Local'),
  };
  const rpc = new Rpc(process.execPath, [path.join(bridgeDir, 'fixture.js')], bridgeDir, env);
  const controller = new Controller(rpc, sessions);
  await controller.refresh();
  const { secret, clientId } = credentials;
  const server = await serve(controller, {
    host: '127.0.0.1',
    port,
    localAdmin: { token: secret, clientId },
  });
  const bridge = {
    endpoint: `ws://127.0.0.1:${String(server.address().port)}/v1`,
    secret,
    clientId,
    shutdown: async () => {
      controller.dispose();
      await server.shutdown();
      await rpc.close();
      rmSync(home, { recursive: true, force: true });
    },
  };
  cleanups.push(bridge.shutdown);
  return bridge;
}

/** Credential Manager: the backend saves the managed secret here and the socket reads it. */
const savedSecrets = memorySecretStore();

/** The backend's `pointguard_chat_connection`: saves the secret, returns what the page sees. */
function managedConnection(bridge: Bridge, secret = bridge.secret) {
  savedSecrets.secrets.set(bridge.endpoint, secret);
  return { endpoint: bridge.endpoint, clientId: bridge.clientId };
}

/** The backend socket's behaviour: the secret saved for the endpoint, the Mac's headers, text
 * records, 1001 on close. */
const wsTransport: PiChatTransport = {
  open: ({ endpoint, clientId }, handlers) =>
    new Promise((resolve, reject) => {
      const secret = savedSecrets.secrets.get(endpoint) ?? '';
      const ws = new WebSocket(endpoint, {
        headers: { Authorization: `Bearer ${secret}`, 'X-PointGuard-Client-ID': clientId },
        maxPayload: 4 * 1024 * 1024,
      });
      let opened = false;
      let closedByPage = false;
      ws.on('open', () => {
        opened = true;
        resolve({
          send: (text) =>
            new Promise<void>((done, fail) => {
              ws.send(text, (error) => {
                if (error) fail(error);
                else done();
              });
            }),
          close: () => {
            closedByPage = true;
            ws.close(1001);
          },
        });
      });
      ws.on('message', (data: Buffer) => {
        handlers.onMessage(data.toString('utf8'));
      });
      ws.on('unexpected-response', (_request, response) => {
        reject(
          new Error(`Point Guard refused the chat connection (${String(response.statusCode)}).`),
        );
        ws.terminate();
      });
      ws.on('error', (error) => {
        if (!opened) reject(error);
      });
      ws.on('close', (code, reason) => {
        if (opened && !closedByPage) handlers.onClose({ code, reason: reason.toString('utf8') });
      });
    }),
};

function memoryPreferences(): PiPreferences & { values: Map<string, string> } {
  const values = new Map<string, string>();
  return {
    values,
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
  };
}

function makeChat(preferences = memoryPreferences(), recovery = memoryRecoveryStore()) {
  const chat = new PiChatModel({
    transport: wsTransport,
    recovery,
    secrets: savedSecrets,
    preferences,
    writerDelayMs: 0,
    retryUnitMs: 20,
  });
  cleanups.push(() => {
    chat.disconnect();
  });
  return chat;
}

/** Resolves once `predicate` holds for the chat's published state. */
function until(
  chat: PiChatModel,
  predicate: () => boolean,
  what: string,
  ms = 8000,
): Promise<void> {
  if (predicate()) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      unsubscribe();
      reject(
        new Error(
          `Timed out waiting for ${what}; status ${chat.connectionStatus}, error ${String(chat.error)}`,
        ),
      );
    }, ms);
    const unsubscribe = chat.store.subscribe(() => {
      if (!predicate()) return;
      clearTimeout(timer);
      unsubscribe();
      resolve();
    });
  });
}

async function connected(chat = makeChat()): Promise<{ chat: PiChatModel; bridge: Bridge }> {
  const bridge = await startBridge();
  const managed = managedConnection(bridge);
  chat.bindManagedConnection(managed.endpoint, managed.clientId);
  await until(chat, () => chat.connected, 'the connection');
  return { chat, bridge };
}

const idleWith = (chat: PiChatModel, text: string) => () =>
  chat.snapshot?.busy === false &&
  chat.snapshot.messages.some((m) => m.role === 'assistant' && m.text.includes(text));

const dialog = (chat: PiChatModel): PiExtensionDialog | undefined => chat.snapshot?.dialogs[0];

describe('PiChatModel against the bridge fixture', () => {
  it('given the managed connection should sync the authoritative snapshot', async () => {
    const { chat, bridge } = await connected();
    expect(chat.connectionStatus).toBe('Connected to Point Guard');
    expect(chat.clientId).toBe(bridge.clientId);
    expect(chat.endpoint).toBe(bridge.endpoint);
    expect(chat.snapshot).toMatchObject({
      version: 1,
      busy: false,
      sessionId: 'fixture-session',
      title: 'A little room to think',
      capabilities: ['images'],
    });
    expect(chat.error).toBeNull();
  });

  it('given a wrong secret should report the refusal and keep retrying', async () => {
    const bridge = await startBridge();
    const chat = makeChat();
    const managed = managedConnection(bridge, 'x'.repeat(32));
    chat.bindManagedConnection(managed.endpoint, managed.clientId);
    await until(chat, () => chat.error?.includes('(401)') === true, 'the refusal');
    expect(chat.connected).toBe(false);
    expect(chat.connectionStatus).toBe('Connection lost · Point Guard continues on this PC');
  });

  it('given a send should record its receipt and show the settled reply with its tool', async () => {
    const { chat } = await connected();
    chat.setDraft('Hello Point Guard');
    await chat.submit('send');
    expect(chat.draft).toBe('');
    expect(chat.submissions.at(-1)).toMatchObject({ status: 'Accepted', recoverable: false });
    await until(chat, idleWith(chat, 'Ready to work.'), 'the reply');
    const rows = chat.store.getState().transcriptRows;
    // The fixture keeps no native toolResult rows, so live tool activity follows the messages.
    expect(rows.map((r) => r.kind)).toEqual(['message', 'message', 'activity']);
    const activity = rows[2];
    expect(activity?.kind === 'activity' && activity.tools[0]?.name).toBe('read');
  });

  it('given an image attachment should send it as image content', async () => {
    const { chat } = await connected();
    const png = Uint8Array.from(
      atob(
        'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aTz0AAAAASUVORK5CYII=',
      ),
      (c) => c.charCodeAt(0),
    );
    chat.addAttachment({
      id: 'img',
      name: 'dot.png',
      mimeType: 'image/png',
      data: bytesToBase64(png),
    });
    await chat.submit('send');
    expect(chat.submissions.at(-1)?.status).toBe('Accepted');
    expect(chat.attachments).toEqual([]);
    await until(chat, idleWith(chat, 'Ready to work.'), 'the reply');
  });

  it('given a provider error should surface it on the reply', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-error');
    await chat.submit('send');
    await until(
      chat,
      () =>
        chat.snapshot?.messages.some(
          (m) => m.error?.includes('Fixture provider unavailable') === true,
        ) === true,
      'the failed reply',
    );
  });

  it('given a busy run should queue steer and follow-up, then Stop should cancel them for restore', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-slow');
    await chat.submit('send');
    await until(chat, () => chat.busy, 'the run');
    chat.setDraft('Steer this way');
    await chat.submit('steer');
    chat.setDraft('And after that');
    await chat.submit('after');
    expect(chat.submissions.slice(-2).map((s) => s.status)).toEqual(['Queued', 'Queued']);
    await until(chat, () => chat.snapshot?.queue.length === 2, 'the queue');
    expect(chat.snapshot?.queue.map((q) => [q.mode, q.text, q.clientId])).toEqual([
      ['steer', 'Steer this way', chat.clientId],
      ['after', 'And after that', chat.clientId],
    ]);

    expect(await chat.stop()).toBe(true);
    await until(chat, () => chat.recoverable.length === 2, 'the canceled messages');
    expect(chat.recoverable.map((s) => [s.text, s.status])).toEqual([
      ['Steer this way', 'Canceled by Stop'],
      ['And after that', 'Canceled by Stop'],
    ]);
    expect(chat.busy).toBe(false);
    expect(
      chat.snapshot?.messages.some((m) => m.error === 'Stopped before the response completed.'),
    ).toBe(true);

    const [first] = chat.recoverable;
    if (first) chat.restore(first);
    expect(chat.draft).toBe('Steer this way');
    expect(chat.recoverable).toHaveLength(1);
    // A later snapshot repeating the canceled IDs does not offer them again.
    chat.setDraft('');
    chat.setDraft('Fresh message');
    await chat.submit('send');
    await until(chat, idleWith(chat, 'Ready to work.'), 'the reply');
    expect(chat.recoverable).toHaveLength(1);
  });

  it('given a confirm dialog should answer it and hear the extension result', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-confirm');
    await chat.submit('send');
    await until(chat, () => dialog(chat)?.method === 'confirm', 'the dialog');
    const open = dialog(chat);
    if (open) await chat.answer(open, { confirmed: true });
    await until(chat, idleWith(chat, 'Extension received: true'), 'the answer');
    expect(chat.snapshot?.dialogs).toEqual([]);
  });

  it('given a select dialog should answer with an offered option ID', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-select');
    await chat.submit('send');
    await until(chat, () => dialog(chat)?.method === 'select', 'the dialog');
    const open = dialog(chat);
    expect(open?.options).toEqual(['Continue', 'Pause']);
    const pause = open?.optionIds?.[1];
    if (open && pause) await chat.answer(open, { optionId: pause });
    await until(chat, idleWith(chat, 'Extension received: Pause'), 'the answer');
  });

  it('given an input dialog should answer with text, and a cancelled editor should cancel', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-input');
    await chat.submit('send');
    await until(chat, () => dialog(chat)?.method === 'input', 'the input dialog');
    const input = dialog(chat);
    if (input) await chat.answer(input, { value: 'Blue' });
    await until(chat, idleWith(chat, 'Extension received: Blue'), 'the input answer');

    chat.setDraft('/fixture-dialog');
    await chat.submit('send');
    await until(chat, () => dialog(chat)?.method === 'editor', 'the editor dialog');
    const editor = dialog(chat);
    expect(editor?.prefill).toBe('A draft from the extension');
    if (editor) await chat.answer(editor, { cancelled: true });
    await until(chat, idleWith(chat, 'Extension interaction canceled.'), 'the cancel');
  });

  it('given an answer to an expired dialog should report the bridge rejection', async () => {
    const { chat } = await connected();
    await chat.answer({ id: 'gone', method: 'confirm' }, { confirmed: true });
    expect(chat.error).toBe('This extension request expired or was already answered.');
  });

  it('given an editor offer should keep the draft and merge the offer on request', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-draft');
    await chat.submit('send');
    await until(chat, () => chat.store.getState().editorOffer !== null, 'the editor offer');
    chat.setDraft('My own words');
    chat.useEditorOffer();
    expect(chat.draft).toBe(
      'My own words\n\nSuggested by an extension. Your existing draft is preserved.',
    );
    expect(chat.store.getState().editorOffer).toBeNull();
    await until(chat, idleWith(chat, 'Ready to work.'), 'the reply');
  });

  it('given native history should list, browse, resume and start conversations', async () => {
    const { chat } = await connected();
    await chat.loadConversations();
    expect(chat.store.getState().conversations).toEqual([
      { id: 'fixture-history', title: 'A previous thought', date: '2026-11-01T10:00:00.000Z' },
    ]);
    const [conversation] = chat.store.getState().conversations;
    if (!conversation) throw new Error('no conversation');
    await chat.browse(conversation);
    expect(chat.store.getState().browsing?.messages.map((m) => m.text)).toEqual([
      'Where should we begin?',
      'With one clear idea.',
    ]);
    expect(chat.canSend).toBe(false);

    const epoch = chat.snapshot?.epoch;
    await chat.changeSession('fixture-history');
    expect(chat.store.getState().browsing).toBeNull();
    expect(chat.snapshot?.sessionId).toBe('fixture-history');
    expect(chat.snapshot?.epoch).not.toBe(epoch);
    expect(chat.snapshot?.messages.map((m) => m.text)).toEqual([
      'Where should we begin?',
      'With one clear idea.',
    ]);

    await chat.changeSession();
    expect(chat.snapshot?.sessionId).not.toBe('fixture-history');
    expect(chat.snapshot?.messages).toEqual([]);
  });

  it('given a busy run should refuse a conversation change unless asked to stop first', async () => {
    const { chat } = await connected();
    chat.setDraft('/fixture-slow');
    await chat.submit('send');
    await until(chat, () => chat.busy, 'the run');
    await chat.changeSession();
    expect(chat.snapshot?.sessionId).toBe('fixture-session');
    await chat.changeSession(null, true);
    expect(chat.snapshot?.sessionId).not.toBe('fixture-session');
    expect(chat.busy).toBe(false);
  });

  it('given a private draft per conversation should keep each and survive a restart', async () => {
    const preferences = memoryPreferences();
    const recovery = memoryRecoveryStore();
    const { chat, bridge } = await connected(makeChat(preferences, recovery));
    chat.setDraft('Draft for the first conversation');
    chat.addAttachment({ id: 'note', name: 'note.txt', mimeType: 'text/plain', data: btoa('hi') });
    await chat.changeSession('fixture-history');
    expect(chat.draft).toBe('');
    expect(chat.attachments).toEqual([]);
    chat.setDraft('Draft for the history');
    await chat.changeSession();
    await chat.changeSession('fixture-history');
    expect(chat.draft).toBe('Draft for the history');
    await chat.flush();
    chat.disconnect();

    const again = makeChat(preferences, recovery);
    await again.loaded;
    again.bindManagedConnection(bridge.endpoint, bridge.clientId);
    await until(again, () => again.connected, 'the reconnection');
    expect(again.draft).toBe('Draft for the history');
    await again.changeSession();
    expect(again.draft).toBe('');
    await again.flush();
    const saved = JSON.parse(recovery.files.get('attachment-drafts') ?? '{}') as Record<
      string,
      { id: string }[]
    >;
    expect(saved[`${bridge.endpoint}:fixture-session`]?.map((f) => f.id)).toEqual(['note']);
  });

  it('given the bridge going away should mark the connection lost and reconnect when it returns', async () => {
    const { chat, bridge } = await connected();
    const epoch = chat.snapshot?.epoch;
    cleanups.splice(cleanups.indexOf(bridge.shutdown), 1);
    await bridge.shutdown();
    await until(chat, () => !chat.connected, 'the loss');
    expect(chat.connectionStatus).toBe('Connection lost · Point Guard continues on this PC');
    expect(chat.error).toContain('Check Point Guard on this PC');
    // The same listener returns with a fresh controller: a new epoch is accepted.
    await startBridge(Number(new URL(bridge.endpoint).port), bridge);
    await until(chat, () => chat.connected, 'the reconnection');
    expect(chat.snapshot?.epoch).not.toBe(epoch);
    expect(chat.error).toBeNull();
  });
});
