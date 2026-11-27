// Port of apps/purepoint-macos/purepoint-macos/Models/PiChatDomain.swift (WIN-9.4): the
// desktop adapter's types for the Point Guard bridge's v1 chat contract
// (apps/purepoint-mobile/bridge/controller.js `snapshot()`, network.js receipts).
//
// Differences from the Swift:
// - Attachment bytes are base64 (`data`), the same text Swift's `Data` encodes to in JSON, so
//   a composer attachment round-trips through the recovery files unchanged.
// - `PiLocalRecoveryStore` and `PiCoalescedWriter` are services here
//   (`services/pointGuardChat.ts`); the rendered Markdown blocks are WIN-9.6's ChatMarkdown.
// - `PiConnectionAddress`/`PiPairingCode` keep the Mac's rules; the Windows backend itself
//   only opens the local bridge (`src-tauri/src/pointguard/chat.rs`).

// MARK: - Draft

/** The composer draft; restoring appends below what is already typed. */
export class PiDraftState {
  constructor(public text = '') {}
  takeSubmission(): string {
    const value = this.text;
    this.text = '';
    return value;
  }
  restore(value: string): void {
    if (value === '') return;
    this.text = this.text === '' ? value : this.text + '\n\n' + value;
  }
}

// MARK: - Attachments

export interface PiComposerAttachment {
  readonly id: string;
  readonly name: string;
  readonly mimeType: string;
  /** The prepared bytes, base64. */
  readonly data: string;
}

export const COMPOSER_LIMIT_COUNT = 4;
export const COMPOSER_LIMIT_BYTES = 512 * 1024;
export const MESSAGE_LIMIT_BYTES = 65536;

export class PiComposerError extends Error {}

export function isImage(file: PiComposerAttachment): boolean {
  return file.mimeType.startsWith('image/');
}

/**
 * The composer chip's thumbnail: a `data:` URL built from the attachment's base64, since the
 * app CSP's `img-src` allows `data:` but not `blob:`. `null` for files that are not images.
 */
export function attachmentThumbnailSrc(file: PiComposerAttachment): string | null {
  return isImage(file) ? `data:${file.mimeType};base64,${file.data}` : null;
}

/** The decoded size of a base64 string. */
export function base64Bytes(data: string): number {
  const length = data.replace(/=+$/, '').length;
  return Math.floor((length * 3) / 4);
}

export function attachmentBytes(files: readonly PiComposerAttachment[]): number {
  return files.reduce((sum, file) => sum + base64Bytes(file.data), 0);
}

export function bytesToBase64(bytes: Uint8Array): string {
  let binary = '';
  for (let i = 0; i < bytes.length; i += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(binary);
}

export function base64ToBytes(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

export const utf8Bytes = (text: string): number => new TextEncoder().encode(text).length;

/** The user-perceived characters (Swift `Character`s) of `text`. */
export function characters(text: string): string[] {
  const segmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' });
  return Array.from(segmenter.segment(text), (s) => s.segment);
}

/** The first `count` characters, as Swift's `String.prefix`. */
export function prefixCharacters(text: string, count: number): string {
  // A character is at least one UTF-16 unit, so a short text is already within the prefix.
  if (text.length <= count) return text;
  return characters(text).slice(0, count).join('');
}

export interface PiImageContent {
  type: 'image';
  mimeType: string;
  data: string;
}

export interface PiComposerPayload {
  readonly message: string;
  readonly images: PiImageContent[];
}

/** The `send` op's text and images: text files inline, images as image content. */
export function makeComposerPayload(
  text: string,
  attachments: readonly PiComposerAttachment[],
): PiComposerPayload {
  if (
    attachments.length > COMPOSER_LIMIT_COUNT ||
    attachmentBytes(attachments) > COMPOSER_LIMIT_BYTES
  )
    throw new PiComposerError('Attach up to four files, totaling 512 KiB after preparation.');
  let message = text;
  const images: PiImageContent[] = [];
  for (const file of attachments) {
    if (isImage(file)) {
      images.push({ type: 'image', mimeType: file.mimeType, data: file.data });
    } else {
      let contents: string;
      try {
        contents = new TextDecoder('utf-8', { fatal: true }).decode(base64ToBytes(file.data));
      } catch {
        throw new PiComposerError('This file is not UTF-8 text.');
      }
      const name = prefixCharacters(file.name.replace(/\n/g, ' ').replace(/\r/g, ' '), 120);
      message += `\n\n--- Attachment: ${name} ---\n${contents}\n--- End attachment ---`;
    }
  }
  if (message.trim() === '' && images.length > 0) message = 'Please review the attached images.';
  if (message.trim() === '' || utf8Bytes(message) > MESSAGE_LIMIT_BYTES)
    throw new PiComposerError('Message and attached text must fit within 64 KiB.');
  return { message, images };
}

// MARK: - Snapshot order

/** Accepts a snapshot only when it is newer than the last one of its epoch. */
export class PiSnapshotCursor {
  private epoch = '';
  private revision = -1;
  accept(epoch: string, revision: number): boolean {
    if (this.epoch === epoch && revision <= this.revision) return false;
    this.epoch = epoch;
    this.revision = revision;
    return true;
  }
  reset(): void {
    this.epoch = '';
    this.revision = -1;
  }
}

// MARK: - Addresses and pairing

/** A Point Guard chat address: ws or wss, `/v1`, loopback or the tailnet (`PiConnectionAddress`). */
export function piConnectionURL(input: string): URL | null {
  let url: URL;
  try {
    url = new URL(input.trim());
  } catch {
    return null;
  }
  if (!['ws:', 'wss:'].includes(url.protocol)) return null;
  const host = url.hostname.toLowerCase();
  if (
    host === '' ||
    url.username !== '' ||
    url.password !== '' ||
    url.search !== '' ||
    url.hash !== '' ||
    input.includes('?') ||
    input.includes('#') ||
    url.pathname !== '/v1'
  )
    return null;
  const labels = host.split('.');
  const numericIPv4 =
    labels.length === 4 &&
    labels.every((l) => /^\d{1,3}$/.test(l) && (l === '0' || !l.startsWith('0')));
  const [first, second] = numericIPv4 ? labels.map(Number) : [];
  const tailnetIPv4 =
    numericIPv4 &&
    first === 100 &&
    second !== undefined &&
    second >= 64 &&
    second <= 127 &&
    labels.every((l) => Number(l) <= 255);
  const allowed =
    tailnetIPv4 ||
    host === '127.0.0.1' ||
    host === 'localhost' ||
    host === '[::1]' ||
    host === '::1' ||
    host.startsWith('fd7a:115c:a1e0:') ||
    host.startsWith('[fd7a:115c:a1e0:') ||
    host.endsWith('.ts.net');
  return allowed ? url : null;
}

/** The canonical endpoint text (the Swift `url.absoluteString`), or null. */
export function piEndpoint(input: string): string | null {
  const url = piConnectionURL(input);
  return url === null ? null : `${url.protocol}//${url.host}${url.pathname}`;
}

/** A pairing secret the bridge can take as a bearer header. */
export function validPairingSecret(secret: string): boolean {
  return (
    characters(secret).length >= 32 &&
    utf8Bytes(secret) <= 1024 &&
    !secret.includes('\n') &&
    !secret.includes('\r')
  );
}

export interface PiPairingCode {
  type: 'pi-mobile-pairing';
  version: 1;
  endpoint: string;
  secret: string;
}

export function parsePairingCode(text: string): PiPairingCode | null {
  if (utf8Bytes(text) > 8192) return null;
  let code: unknown;
  try {
    code = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof code !== 'object' || code === null) return null;
  const c = code as Record<string, unknown>;
  if (
    c.type !== 'pi-mobile-pairing' ||
    c.version !== 1 ||
    typeof c.endpoint !== 'string' ||
    typeof c.secret !== 'string' ||
    piConnectionURL(c.endpoint) === null ||
    !validPairingSecret(c.secret)
  )
    return null;
  return { type: 'pi-mobile-pairing', version: 1, endpoint: c.endpoint, secret: c.secret };
}

// MARK: - Wire records

export interface PiChatMessage {
  readonly id: string;
  readonly role: string;
  readonly text: string;
  readonly activity?: string | null;
  readonly error?: string | null;
}

export interface PiToolActivity {
  readonly id: string;
  readonly name: string;
  readonly state: string;
  readonly text: string;
}

export interface PiQueuedText {
  readonly id: string;
  readonly clientId: string | null;
  readonly mode: string;
  readonly text: string;
}

export interface PiExtensionDialog {
  readonly id: string;
  readonly method: string;
  readonly title?: string | null;
  readonly message?: string | null;
  readonly prefill?: string | null;
  readonly placeholder?: string | null;
  readonly options?: string[] | null;
  readonly optionIds?: string[] | null;
}

export interface PiEditorText {
  readonly id: string;
  readonly text: string;
}

export interface PiCanceledText {
  readonly clientId: string | null;
  readonly id: string;
  readonly text: string;
  readonly sessionId: string;
}

/** The bridge's authoritative state (`Controller.snapshot()`). */
export interface PiSnapshot {
  readonly version: number;
  readonly epoch: string;
  readonly revision: number;
  readonly busy: boolean;
  readonly runId?: string | null;
  readonly sessionId: string;
  readonly title: string;
  readonly messages: PiChatMessage[];
  readonly tools: PiToolActivity[];
  readonly queue: PiQueuedText[];
  readonly dialogs: PiExtensionDialog[];
  readonly notices: string[];
  readonly error?: string | null;
  readonly editor?: PiEditorText | null;
  readonly canceled?: PiCanceledText[] | null;
  readonly capabilities?: string[] | null;
}

export interface PiConversation {
  readonly id: string;
  readonly title: string;
  readonly date?: string | null;
}

export interface PiHistory {
  readonly sessionId: string;
  readonly title: string;
  readonly messages: PiChatMessage[];
}

/** A message the composer sent, kept until its delivery is known (or for recovery). */
export interface PiSubmission {
  readonly id: string;
  readonly text: string;
  readonly sessionId: string;
  readonly status: string;
  readonly recoverable: boolean;
  readonly attachments?: PiComposerAttachment[] | null;
}

export type JsonValue =
  null | boolean | number | string | JsonValue[] | { [key: string]: JsonValue };

/** Every non-snapshot record: `receipt`, `editor`, `permissions` and terminal records. */
export interface PiWireEnvelope {
  readonly type: string;
  readonly id?: string;
  readonly ok?: boolean;
  readonly data?: JsonValue;
  readonly error?: string;
  readonly text?: string;
  readonly terminals?: string;
}

export class PiDecodeError extends Error {}

const isObject = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);
const isString = (value: unknown): value is string => typeof value === 'string';
const optionalString = (value: unknown) => value === undefined || value === null || isString(value);
const isStrings = (value: unknown): value is string[] =>
  Array.isArray(value) && value.every(isString);

function decodeMessage(value: unknown): PiChatMessage {
  if (
    !isObject(value) ||
    !isString(value.id) ||
    !isString(value.role) ||
    !isString(value.text) ||
    !optionalString(value.activity) ||
    !optionalString(value.error)
  )
    throw new PiDecodeError('Invalid message in bridge snapshot.');
  return value as unknown as PiChatMessage;
}

function decodeAll<T>(value: unknown, item: (x: unknown) => T, what: string): T[] {
  if (!Array.isArray(value)) throw new PiDecodeError(`Invalid ${what} in bridge snapshot.`);
  return value.map(item);
}

function require<T>(ok: boolean, value: T, what: string): T {
  if (!ok) throw new PiDecodeError(`Invalid ${what} in bridge snapshot.`);
  return value;
}

/** Decode a snapshot strictly, as Swift's `Codable` does: a missing required field throws. */
export function decodeSnapshot(value: unknown): PiSnapshot {
  if (!isObject(value)) throw new PiDecodeError('Invalid bridge snapshot.');
  const v = value;
  if (
    typeof v.version !== 'number' ||
    !isString(v.epoch) ||
    typeof v.revision !== 'number' ||
    typeof v.busy !== 'boolean' ||
    !optionalString(v.runId) ||
    !isString(v.sessionId) ||
    !isString(v.title) ||
    !isStrings(v.notices) ||
    !optionalString(v.error)
  )
    throw new PiDecodeError('Invalid bridge snapshot.');
  const tools = decodeAll(
    v.tools,
    (t) =>
      require(isObject(t) &&
        isString(t.id) &&
        isString(t.name) &&
        isString(t.state) &&
        isString(t.text), t as PiToolActivity, 'tool'),
    'tools',
  );
  const queue = decodeAll(
    v.queue,
    (q) =>
      require(isObject(q) &&
        isString(q.id) &&
        optionalString(q.clientId) &&
        isString(q.mode) &&
        isString(q.text), q as PiQueuedText, 'queue'),
    'queue',
  );
  const dialogs = decodeAll(
    v.dialogs,
    (d) =>
      require(isObject(d) &&
        isString(d.id) &&
        isString(d.method) &&
        (d.options === undefined || d.options === null || isStrings(d.options)) &&
        (d.optionIds === undefined ||
          d.optionIds === null ||
          isStrings(d.optionIds)), d as PiExtensionDialog, 'dialog'),
    'dialogs',
  );
  const editor = v.editor;
  if (
    editor !== undefined &&
    editor !== null &&
    !(isObject(editor) && isString(editor.id) && isString(editor.text))
  )
    throw new PiDecodeError('Invalid editor offer in bridge snapshot.');
  const canceled =
    v.canceled === undefined || v.canceled === null
      ? v.canceled
      : decodeAll(
          v.canceled,
          (c) =>
            require(isObject(c) &&
              isString(c.id) &&
              isString(c.text) &&
              isString(c.sessionId) &&
              optionalString(c.clientId), c as PiCanceledText, 'canceled message'),
          'canceled messages',
        );
  if (v.capabilities !== undefined && v.capabilities !== null && !isStrings(v.capabilities))
    throw new PiDecodeError('Invalid bridge capabilities.');
  return {
    ...(v as unknown as PiSnapshot),
    messages: decodeAll(v.messages, decodeMessage, 'messages'),
    tools,
    queue,
    dialogs,
    ...(canceled === undefined ? {} : { canceled }),
  };
}

export function decodeConversations(value: unknown): PiConversation[] {
  if (!Array.isArray(value)) throw new PiDecodeError('Invalid conversation list.');
  return value.map((c) => {
    if (!isObject(c) || !isString(c.id) || !isString(c.title) || !optionalString(c.date))
      throw new PiDecodeError('Invalid conversation list.');
    return { id: c.id, title: c.title, date: c.date ?? null };
  });
}

export function decodeHistory(value: unknown): PiHistory {
  if (!isObject(value) || !isString(value.sessionId) || !isString(value.title))
    throw new PiDecodeError('Invalid conversation history.');
  return {
    sessionId: value.sessionId,
    title: value.title,
    messages: decodeAll(value.messages, decodeMessage, 'messages'),
  };
}

// MARK: - Transcript rows

export type PiTranscriptRow =
  | { readonly kind: 'message'; readonly id: string; readonly message: PiChatMessage }
  | { readonly kind: 'activity'; readonly id: string; readonly tools: PiToolActivity[] };

/**
 * Messages and tool activity as display rows: consecutive tools group into one activity row,
 * native `toolResult` rows merge with their live tool, and blank assistant rows are dropped.
 */
export function makeTranscriptRows(
  messages: readonly PiChatMessage[],
  tools: readonly PiToolActivity[],
): PiTranscriptRow[] {
  const rows: PiTranscriptRow[] = [];
  const represented = new Set<string>();
  const appendTool = (tool: PiToolActivity) => {
    const last = rows.at(-1);
    if (last?.kind === 'activity') {
      rows[rows.length - 1] = { ...last, tools: [...last.tools, tool] };
    } else {
      rows.push({ kind: 'activity', id: 'activity:' + tool.id, tools: [tool] });
    }
  };
  for (const message of messages) {
    if (message.role === 'toolResult') {
      // The pinned bridge keys native rows as role-timestamp-toolCallId.
      // Match the complete tool ID suffix, preserving IDs that contain hyphens.
      const live = tools.find((tool) => message.id.endsWith('-' + tool.id));
      const id = live?.id ?? message.id;
      if (represented.has(id)) continue;
      represented.add(id);
      const text = message.text + (message.error != null ? '\n\n' + message.error : '');
      appendTool({
        id,
        name: message.activity ?? live?.name ?? 'Tool',
        state: message.error == null ? (live?.state ?? 'finished') : 'failed',
        text,
      });
    } else if (
      message.role !== 'assistant' ||
      message.text.trim() !== '' ||
      message.error != null
    ) {
      rows.push({ kind: 'message', id: 'message:' + message.id, message });
    }
  }
  for (const tool of tools) if (!represented.has(tool.id)) appendTool(tool);
  return rows;
}

/**
 * How much of a transcript the eager chat renders. The bridge bounds snapshots by bytes
 * (1 MiB), not rows, so only the newest page renders until the reader asks for earlier ones.
 * Anchored by row ID, so rows arriving at the end, or the bridge trimming the oldest rows,
 * never pull already-loaded history out from under the reader.
 */
export class PiTranscriptWindow {
  static readonly page = 200;
  private anchor: string | null = null;

  firstIndex(rows: readonly { id: string }[]): number {
    if (this.anchor !== null) {
      const index = rows.findIndex((row) => row.id === this.anchor);
      if (index >= 0) return index;
    }
    return Math.max(0, rows.length - PiTranscriptWindow.page);
  }
  visible<Row extends { id: string }>(rows: readonly Row[]): Row[] {
    return rows.slice(this.firstIndex(rows));
  }
  showEarlier(rows: readonly { id: string }[]): void {
    const first = Math.max(0, this.firstIndex(rows) - PiTranscriptWindow.page);
    this.anchor = rows[first]?.id ?? null;
  }
  reset(): void {
    this.anchor = null;
  }
}

// MARK: - Incoming records

export type PiIncomingRecord =
  | { kind: 'snapshot'; snapshot: PiSnapshot; rows: PiTranscriptRow[] }
  | { kind: 'envelope'; envelope: PiWireEnvelope };

/** Decode one text record from the bridge. */
export function decodeIncoming(text: string): PiIncomingRecord {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    throw new PiDecodeError('Unsupported message from bridge.');
  }
  if (!isObject(value) || !isString(value.type))
    throw new PiDecodeError('Unsupported message from bridge.');
  if (value.type === 'snapshot') {
    const snapshot = decodeSnapshot(value);
    return {
      kind: 'snapshot',
      snapshot,
      rows: makeTranscriptRows(snapshot.messages, snapshot.tools),
    };
  }
  return { kind: 'envelope', envelope: value as unknown as PiWireEnvelope };
}
