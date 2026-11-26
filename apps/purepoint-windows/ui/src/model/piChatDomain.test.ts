// Port of the PiChatDomain cases (WIN-9.4): composer payloads, addresses, pairing codes,
// transcript rows and windows, strict snapshot decoding.
import { describe, expect, it } from 'vitest';
import {
  base64Bytes,
  bytesToBase64,
  decodeIncoming,
  decodeSnapshot,
  makeComposerPayload,
  makeTranscriptRows,
  parsePairingCode,
  piEndpoint,
  PiDraftState,
  PiSnapshotCursor,
  PiTranscriptWindow,
  prefixCharacters,
  type PiComposerAttachment,
} from './piChatDomain';

const text = (name: string, body: string): PiComposerAttachment => ({
  id: name,
  name,
  mimeType: 'text/plain',
  data: bytesToBase64(new TextEncoder().encode(body)),
});

describe('PiDraftState', () => {
  it('given a restore should append below the typed draft', () => {
    const draft = new PiDraftState('typed');
    draft.restore('');
    draft.restore('restored');
    expect(draft.text).toBe('typed\n\nrestored');
    expect(draft.takeSubmission()).toBe('typed\n\nrestored');
    expect(draft.text).toBe('');
  });
});

describe('makeComposerPayload', () => {
  it('given text files should inline them and images should become image content', () => {
    const payload = makeComposerPayload('Look', [
      text('notes\n.txt', 'hello'),
      { id: 'i', name: 'a.png', mimeType: 'image/png', data: 'AAAA' },
    ]);
    expect(payload.message).toBe(
      'Look\n\n--- Attachment: notes .txt ---\nhello\n--- End attachment ---',
    );
    expect(payload.images).toEqual([{ type: 'image', mimeType: 'image/png', data: 'AAAA' }]);
  });

  it('given only images should supply the review prompt', () => {
    const payload = makeComposerPayload('  ', [
      { id: 'i', name: 'a.png', mimeType: 'image/png', data: 'AAAA' },
    ]);
    expect(payload.message).toBe('Please review the attached images.');
  });

  it('given too many files, non-UTF-8 text or an empty message should refuse', () => {
    expect(() =>
      makeComposerPayload(
        'x',
        Array.from({ length: 5 }, (_, i) => text(String(i), 'a')),
      ),
    ).toThrow('Attach up to four files');
    expect(() =>
      makeComposerPayload('x', [
        {
          id: 'b',
          name: 'b.bin',
          mimeType: 'text/plain',
          data: bytesToBase64(Uint8Array.of(0xff, 0xfe)),
        },
      ]),
    ).toThrow('not UTF-8');
    expect(() => makeComposerPayload(' ', [])).toThrow('64 KiB');
    expect(() => makeComposerPayload('x'.repeat(65537), [])).toThrow('64 KiB');
  });

  it('given base64 should count decoded bytes', () => {
    expect(base64Bytes(bytesToBase64(new Uint8Array(10)))).toBe(10);
    expect(base64Bytes('')).toBe(0);
  });
});

describe('addresses and pairing', () => {
  it('given chat addresses should accept loopback and the tailnet only', () => {
    expect(piEndpoint(' ws://127.0.0.1:8787/v1 ')).toBe('ws://127.0.0.1:8787/v1');
    expect(piEndpoint('wss://100.100.1.2:8787/v1')).toBe('wss://100.100.1.2:8787/v1');
    expect(piEndpoint('wss://host.tail1234.ts.net/v1')).toBe('wss://host.tail1234.ts.net/v1');
    expect(piEndpoint('ws://[::1]:9/v1')).toBe('ws://[::1]:9/v1');
    for (const bad of [
      'http://127.0.0.1:1/v1',
      'ws://8.8.8.8:1/v1',
      'ws://100.128.0.1:1/v1',
      'ws://100.064.0.1:1/v1',
      'ws://127.0.0.1:1/v2',
      'ws://127.0.0.1:1/v1?x=1',
      'ws://user:pw@127.0.0.1:1/v1',
    ])
      expect(piEndpoint(bad), bad).toBeNull();
  });

  it('given a pairing QR should parse only the v1 shape', () => {
    const code = {
      type: 'pi-mobile-pairing',
      version: 1,
      endpoint: 'ws://127.0.0.1:1/v1',
      secret: 'k'.repeat(32),
    };
    expect(parsePairingCode(JSON.stringify(code))).toEqual(code);
    expect(parsePairingCode(JSON.stringify({ ...code, version: 2 }))).toBeNull();
    expect(parsePairingCode(JSON.stringify({ ...code, secret: 'short' }))).toBeNull();
    expect(parsePairingCode('nope')).toBeNull();
  });

  it('given a long text should cut it by characters, not code units', () => {
    expect(prefixCharacters('👍🏽👍🏽', 1)).toBe('👍🏽');
  });
});

describe('snapshots', () => {
  const base = {
    type: 'snapshot',
    version: 1,
    epoch: 'e',
    revision: 1,
    busy: false,
    runId: null,
    sessionId: 's',
    title: 't',
    messages: [],
    tools: [],
    queue: [],
    dialogs: [],
    notices: [],
    error: null,
    editor: null,
    canceled: [],
    capabilities: ['images'],
  };

  it('given the cursor should take a newer revision or a new epoch only', () => {
    const cursor = new PiSnapshotCursor();
    expect(cursor.accept('e', 1)).toBe(true);
    expect(cursor.accept('e', 1)).toBe(false);
    expect(cursor.accept('e', 0)).toBe(false);
    expect(cursor.accept('f', 0)).toBe(true);
    cursor.reset();
    expect(cursor.accept('f', 0)).toBe(true);
  });

  it('given a record should decode snapshots strictly and keep other envelopes', () => {
    const record = decodeIncoming(JSON.stringify(base));
    expect(record.kind).toBe('snapshot');
    expect(() => decodeSnapshot({ ...base, messages: [{ id: 1 }] })).toThrow('Invalid message');
    expect(() => decodeSnapshot({ ...base, epoch: undefined })).toThrow('Invalid bridge snapshot');
    expect(decodeIncoming('{"type":"receipt","id":"a","ok":true}')).toEqual({
      kind: 'envelope',
      envelope: { type: 'receipt', id: 'a', ok: true },
    });
    expect(() => decodeIncoming('[1]')).toThrow('Unsupported message');
  });
});

describe('transcript rows', () => {
  it('given tool results should merge them with live tools and group activity', () => {
    const rows = makeTranscriptRows(
      [
        { id: 'u', role: 'user', text: 'Go' },
        { id: 'toolResult-5-call-a-1', role: 'toolResult', text: 'read ok', activity: 'read' },
        { id: 'toolResult-6-call-b', role: 'toolResult', text: 'oops', error: 'denied' },
        { id: 'blank', role: 'assistant', text: '  ' },
        { id: 'a', role: 'assistant', text: 'Done' },
      ],
      [
        { id: 'call-a-1', name: 'read', state: 'running', text: '' },
        { id: 'call-c', name: 'bash', state: 'running', text: 'ls' },
      ],
    );
    expect(rows.map((r) => r.id)).toEqual([
      'message:u',
      'activity:call-a-1',
      'message:a',
      'activity:call-c',
    ]);
    const group = rows[1];
    expect(group?.kind === 'activity' && group.tools).toEqual([
      { id: 'call-a-1', name: 'read', state: 'running', text: 'read ok' },
      { id: 'toolResult-6-call-b', name: 'Tool', state: 'failed', text: 'oops\n\ndenied' },
    ]);
  });

  it('given a long transcript should show the newest page and keep the earlier anchor', () => {
    const rows = Array.from({ length: 450 }, (_, i) => ({ id: String(i) }));
    const window = new PiTranscriptWindow();
    expect(window.visible(rows)[0]?.id).toBe('250');
    window.showEarlier(rows);
    expect(window.visible(rows)[0]?.id).toBe('50');
    expect(window.visible([...rows, { id: 'new' }])[0]?.id).toBe('50');
    window.showEarlier(rows);
    expect(window.firstIndex(rows)).toBe(0);
    window.reset();
    expect(window.firstIndex(rows)).toBe(250);
  });
});
