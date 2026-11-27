// Port of apps/purepoint-macos/purepoint-macos/State/PiChatModel.swift (WIN-9.4), macOS side:
// the Point Guard chat over the bridge's WebSocket v1 contract.
//
// - Authoritative snapshots: the bridge's `snapshot` records (and every `sync` receipt) replace
//   the conversation; an older revision of the same epoch is ignored (`PiSnapshotCursor`).
// - Requests are `{version: 1, clientId, id, op, ...}`; each resolves on its `receipt`, or fails
//   after 35 s as "delivery uncertain". At most 16 are in flight.
// - Sends are recorded as submissions, persisted (confirmed) before the record leaves, so a
//   message whose delivery is in doubt — a lost connection, a missing receipt, a Stop that
//   cancelled it from the queue — is offered for restore rather than resent.
// - The private draft (per endpoint and session) and attachment drafts survive restarts.
//
// Shape (DEC-14): published values live in a Zustand store (`chat.store`). The socket is held by
// the backend (`services/pointGuardChat.ts`, see there for why); the managed runtime binds it
// with `bindManagedConnection`, wired by `pointGuardChatBinding` to WIN-9.3's
// `onConnection`/`onDisconnect` hooks. Pairing secrets are in Credential Manager (WIN-9.7,
// `services/pairingSecret.ts`). The iPad pairing paths (`#if os(iOS)`) are not ported.
// Windows wording: "this PC" where the Mac says "your Mac".

import { createStore, type StoreApi } from 'zustand/vanilla';
import {
  attachmentBytes,
  COMPOSER_LIMIT_BYTES,
  COMPOSER_LIMIT_COUNT,
  decodeConversations,
  decodeHistory,
  decodeIncoming,
  decodeSnapshot,
  makeComposerPayload,
  makeTranscriptRows,
  piEndpoint,
  prefixCharacters,
  PiDraftState,
  PiSnapshotCursor,
  validPairingSecret,
  type JsonValue,
  type PiComposerAttachment,
  type PiConversation,
  type PiExtensionDialog,
  type PiHistory,
  type PiIncomingRecord,
  type PiSnapshot,
  type PiSubmission,
  type PiTranscriptRow,
} from '../model/piChatDomain';
import { credentialManagerSecrets } from '../services/pairingSecret';
import {
  PiCoalescedWriter,
  tauriChatTransport,
  tauriRecoveryStore,
  type PiChatTransport,
  type PiRecoveryStore,
  type PiSecretStore,
  type PiSocket,
} from '../services/pointGuardChat';
import type { PointGuardChatConnection } from '../services/pointguard';
import { browserStorage } from './storage';

export const LOCAL_DEVICE = 'PC';
const RECOVERY_LIMIT = 50;
const PENDING_LIMIT = 16;

export { memorySecretStore, type PiSecretStore } from '../services/pointGuardChat';

/** The preferences the Swift keeps in UserDefaults. */
export interface PiPreferences {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

const KEY_CLIENT_ID = 'pointguard.clientId';
const KEY_ENDPOINT = 'pointguard.endpoint';
const KEY_DRAFTS = 'pointguard.drafts';
/** The drafts key's endpoint part for the app-owned bridge. */
const MANAGED_DRAFT_ENDPOINT = 'managed';
const KEY_CANCELED = 'pointguard.canceledIds';

export interface PiChatFields {
  readonly snapshot: PiSnapshot | null;
  readonly transcriptRows: readonly PiTranscriptRow[];
  readonly connected: boolean;
  readonly connectionStatus: string;
  readonly error: string | null;
  readonly draft: string;
  readonly attachments: readonly PiComposerAttachment[];
  readonly submissions: readonly PiSubmission[];
  readonly conversations: readonly PiConversation[];
  readonly browsing: PiHistory | null;
  readonly editorOffer: string | null;
  readonly changingSession: boolean;
  readonly endpoint: string;
  readonly demo: boolean;
}

export interface PiChatOptions {
  transport?: PiChatTransport;
  recovery?: PiRecoveryStore;
  secrets?: PiSecretStore;
  preferences?: PiPreferences | null;
  /** Milliseconds per second of reconnect backoff (tests shrink it). */
  retryUnitMs?: number;
  receiptTimeoutMs?: number;
  writerDelayMs?: number;
  demoDelayMs?: number;
  uuid?: () => string;
}

interface Pending {
  resolve: (value: JsonValue) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

function describe(error: unknown): string {
  if (typeof error === 'string') return error;
  if (error instanceof Error) return error.message;
  return String(error);
}

function field(value: JsonValue | undefined, key: string): JsonValue {
  if (value !== null && value !== undefined && typeof value === 'object' && !Array.isArray(value))
    return value[key] ?? null;
  return null;
}

/** Keep the newest 1000 distinct IDs, in order. */
function recentCanceledIds(ids: readonly string[]): string[] {
  const seen = new Set<string>();
  const kept: string[] = [];
  for (const id of [...ids].reverse()) {
    if (kept.length >= 1000) break;
    if (seen.has(id)) continue;
    seen.add(id);
    kept.push(id);
  }
  return kept.reverse();
}

/**
 * In-flight sends outrank recovery, which outranks settled receipts. Within each priority
 * retain the newest, then preserve chronological display order. Settled receipts drop files.
 */
export function retainedSubmissions(items: readonly PiSubmission[]): PiSubmission[] {
  const priority = (item: PiSubmission) =>
    item.status === 'Sending' ? 2 : item.recoverable ? 1 : 0;
  const kept = new Set(
    items
      .map((item, index) => ({ index, rank: priority(item) }))
      .sort((a, b) => (a.rank === b.rank ? b.index - a.index : b.rank - a.rank))
      .map((x) => x.index)
      .slice(0, RECOVERY_LIMIT),
  );
  return items.flatMap((item, index) => {
    if (!kept.has(index)) return [];
    return !item.recoverable && item.status !== 'Sending' && item.attachments
      ? [{ ...item, attachments: null }]
      : [item];
  });
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

function validAttachment(value: unknown): value is PiComposerAttachment {
  return (
    isRecord(value) &&
    typeof value.id === 'string' &&
    typeof value.name === 'string' &&
    typeof value.mimeType === 'string' &&
    typeof value.data === 'string'
  );
}

function parseAttachmentDrafts(json: string | null): Map<string, PiComposerAttachment[]> {
  const drafts = new Map<string, PiComposerAttachment[]>();
  if (json === null) return drafts;
  try {
    const value: unknown = JSON.parse(json);
    if (!isRecord(value)) return drafts;
    for (const [key, files] of Object.entries(value))
      if (Array.isArray(files) && files.every(validAttachment)) drafts.set(key, files);
  } catch {
    // Unreadable recovery is ignored, as the Swift `try?` does.
  }
  return drafts;
}

function parseSubmissions(json: string | null): PiSubmission[] {
  if (json === null) return [];
  try {
    const value: unknown = JSON.parse(json);
    if (!Array.isArray(value)) return [];
    const valid = value.every(
      (s) =>
        isRecord(s) &&
        typeof s.id === 'string' &&
        typeof s.text === 'string' &&
        typeof s.sessionId === 'string' &&
        typeof s.status === 'string' &&
        typeof s.recoverable === 'boolean' &&
        (s.attachments === undefined ||
          s.attachments === null ||
          (Array.isArray(s.attachments) && s.attachments.every(validAttachment))),
    );
    return valid ? (value as PiSubmission[]) : [];
  } catch {
    return [];
  }
}

function emptyFields(endpoint: string): PiChatFields {
  return {
    snapshot: null,
    transcriptRows: [],
    connected: false,
    connectionStatus: 'Not connected',
    error: null,
    draft: '',
    attachments: [],
    submissions: [],
    conversations: [],
    browsing: null,
    editorOffer: null,
    changingSession: false,
    endpoint,
    demo: false,
  };
}

/** The Point Guard chat. */
export class PiChatModel {
  readonly store: StoreApi<PiChatFields>;
  private readonly transport: PiChatTransport;
  private readonly recoveryStore: PiRecoveryStore;
  private readonly secrets: PiSecretStore;
  private readonly preferences: PiPreferences | null;
  private readonly writer: PiCoalescedWriter;
  private readonly retryUnitMs: number;
  private readonly receiptTimeoutMs: number;
  private readonly demoDelayMs: number;
  private readonly uuid: () => string;

  private socket: PiSocket | null = null;
  private opening = false;
  private retryTimer: ReturnType<typeof setTimeout> | null = null;
  private pending = new Map<string, Pending>();
  private drafts = new Map<string, string>();
  private attachmentDrafts = new Map<string, PiComposerAttachment[]>();
  private readonly recoveryTask: Promise<void>;
  private connectionTask: object | null = null;
  private recoveryLoaded = false;
  private readonly cursor = new PiSnapshotCursor();
  private readonly seenEditorIds = new Set<string>();
  private canceledIdHistory: string[] = [];
  private seenCanceledIds = new Set<string>();
  private generation = 0;
  private foreground = true;
  private wantsConnection = false;
  private retryDelay = 2;
  private _clientId: string;
  /** The app-owned bridge's address while bound; its port changes with every launch. */
  private managedEndpoint: string | null = null;

  constructor(options: PiChatOptions = {}) {
    this.transport = options.transport ?? tauriChatTransport;
    this.recoveryStore = options.recovery ?? tauriRecoveryStore;
    this.secrets = options.secrets ?? credentialManagerSecrets;
    this.preferences = options.preferences === undefined ? browserStorage() : options.preferences;
    this.writer = new PiCoalescedWriter(options.writerDelayMs ?? 250);
    this.retryUnitMs = options.retryUnitMs ?? 1000;
    this.receiptTimeoutMs = options.receiptTimeoutMs ?? 35_000;
    this.demoDelayMs = options.demoDelayMs ?? 800;
    this.uuid = options.uuid ?? (() => crypto.randomUUID());

    this._clientId = this.pref(KEY_CLIENT_ID) ?? this.uuid().toUpperCase();
    this.setPref(KEY_CLIENT_ID, this._clientId);
    this.store = createStore<PiChatFields>()(() => emptyFields(this.pref(KEY_ENDPOINT) ?? ''));
    try {
      const saved: unknown = JSON.parse(this.pref(KEY_DRAFTS) ?? '{}');
      if (isRecord(saved))
        for (const [key, value] of Object.entries(saved))
          if (typeof value === 'string') this.drafts.set(key, value);
    } catch {
      // A damaged draft list starts empty.
    }
    let savedCanceled: string[] = [];
    try {
      const saved: unknown = JSON.parse(this.pref(KEY_CANCELED) ?? '[]');
      if (Array.isArray(saved))
        savedCanceled = saved.filter((x): x is string => typeof x === 'string');
    } catch {
      // As above.
    }
    this.canceledIdHistory = recentCanceledIds(savedCanceled);
    this.seenCanceledIds = new Set(this.canceledIdHistory);
    this.setDraft(this.drafts.get(this.draftKey) ?? '');
    if (this.canceledIdHistory.join('\n') !== savedCanceled.join('\n')) this.saveCanceledIds();
    this.recoveryTask = this.loadRecovery();
  }

  // MARK: - Published values

  private get s(): PiChatFields {
    return this.store.getState();
  }
  private set(patch: Partial<PiChatFields>): void {
    this.store.setState(patch);
  }
  get snapshot(): PiSnapshot | null {
    return this.s.snapshot;
  }
  get connected(): boolean {
    return this.s.connected;
  }
  get connectionStatus(): string {
    return this.s.connectionStatus;
  }
  get error(): string | null {
    return this.s.error;
  }
  set error(value: string | null) {
    this.set({ error: value });
  }
  get draft(): string {
    return this.s.draft;
  }
  get attachments(): readonly PiComposerAttachment[] {
    return this.s.attachments;
  }
  get submissions(): readonly PiSubmission[] {
    return this.s.submissions;
  }
  get endpoint(): string {
    return this.s.endpoint;
  }
  get demo(): boolean {
    return this.s.demo;
  }
  get clientId(): string {
    return this._clientId;
  }
  get busy(): boolean {
    return this.s.snapshot?.busy ?? false;
  }
  get canSend(): boolean {
    const s = this.s;
    return (
      s.connected &&
      s.browsing === null &&
      (s.snapshot?.error ?? null) === null &&
      (!s.demo || !this.busy) &&
      !s.changingSession &&
      (!this.busy || s.attachments.length === 0) &&
      (s.draft.trim() !== '' || s.attachments.length > 0)
    );
  }
  /**
   * A play sends its own text, so it needs an idle live session but not a draft. A message
   * still on its way counts as busy, since `busy` only turns true with the next snapshot.
   */
  get canSendPlay(): boolean {
    const s = this.s;
    return (
      s.connected &&
      s.snapshot !== null &&
      s.browsing === null &&
      (s.snapshot.error ?? null) === null &&
      !s.changingSession &&
      !this.busy &&
      !s.submissions.some((x) => x.status === 'Sending')
    );
  }
  get draftKey(): string {
    // The managed bridge listens on a new port each launch, so its drafts are keyed by role,
    // not address: a draft typed before a relaunch is recovered after it (WIN-9.5).
    const endpoint =
      this.managedEndpoint !== null && this.s.endpoint === this.managedEndpoint
        ? MANAGED_DRAFT_ENDPOINT
        : this.s.endpoint;
    return endpoint + ':' + (this.s.snapshot?.sessionId ?? 'local');
  }
  get recoverable(): PiSubmission[] {
    return this.s.submissions.filter((x) => x.recoverable);
  }
  /** Resolves once saved recovery has loaded (tests and the first send wait for it). */
  get loaded(): Promise<void> {
    return this.recoveryTask;
  }

  // MARK: - Persistence

  private pref(key: string): string | null {
    try {
      return this.preferences?.getItem(key) ?? null;
    } catch {
      return null;
    }
  }
  private setPref(key: string, value: string): void {
    try {
      this.preferences?.setItem(key, value);
    } catch {
      // Unavailable storage keeps the value for this run.
    }
  }

  private async loadRecovery(): Promise<void> {
    const [attachmentJson, submissionJson] = await Promise.all([
      this.recoveryStore.load('attachment-drafts').catch(() => null),
      this.recoveryStore.load('submissions').catch(() => null),
    ]);
    // Edits made while loading always win over saved recovery.
    const saved = parseAttachmentDrafts(attachmentJson);
    for (const [key, files] of this.attachmentDrafts) saved.set(key, files);
    this.attachmentDrafts = saved;
    const currentIds = new Set(this.s.submissions.map((x) => x.id));
    const restored = parseSubmissions(submissionJson)
      .filter((x) => !currentIds.has(x.id))
      .map((x) =>
        x.status === 'Sending' ? { ...x, status: 'Delivery uncertain', recoverable: true } : x,
      );
    this.setSubmissions([...restored, ...this.s.submissions]);
    this.set({ attachments: this.attachmentDrafts.get(this.draftKey) ?? this.s.attachments });
    this.recoveryLoaded = true;
    this.saveSubmissions();
    this.saveAttachmentDraft();
  }

  private saveCanceledIds(): void {
    const ids = this.canceledIdHistory;
    this.writer.schedule('canceled', () => {
      this.setPref(KEY_CANCELED, JSON.stringify(ids));
    });
  }

  private saveDraft(): void {
    this.drafts.set(this.draftKey, prefixCharacters(this.s.draft, 65536));
    if (this.drafts.size > 100) {
      const key = [...this.drafts.keys()].find((k) => k !== this.draftKey);
      if (key !== undefined) this.drafts.delete(key);
    }
    // Written now, not through the coalesced writer: a quit (Point Guard's quit gate exits the
    // app without the page taking part) must never find a draft write still waiting on a timer.
    // The list was already serialized for this edit, so the write itself is the only extra cost.
    this.setPref(KEY_DRAFTS, JSON.stringify(Object.fromEntries(this.drafts)));
  }

  private saveSubmissions(): void {
    if (!this.recoveryLoaded) return;
    const value = JSON.stringify(this.s.submissions);
    this.writer.schedule('submissions', async () => {
      try {
        await this.recoveryStore.save('submissions', value);
      } catch {
        if (this.s.error?.startsWith('Message not sent:') !== true)
          this.error = `Could not save message recovery on this ${LOCAL_DEVICE}.`;
      }
    });
  }

  private saveAttachmentDraft(): void {
    this.attachmentDrafts.set(this.draftKey, [...this.s.attachments]);
    while (this.attachmentDrafts.size > 10) {
      const key = [...this.attachmentDrafts.keys()].find((k) => k !== this.draftKey);
      if (key === undefined) break;
      this.attachmentDrafts.delete(key);
    }
    if (!this.recoveryLoaded) return;
    const value = JSON.stringify(Object.fromEntries(this.attachmentDrafts));
    this.writer.schedule('attachments', async () => {
      try {
        await this.recoveryStore.save('attachment-drafts', value);
      } catch {
        this.error = `Could not save attachments on this ${LOCAL_DEVICE}. Keep the app open until sending.`;
      }
    });
  }

  /** Write every pending preference and recovery file now. */
  flush(): Promise<void> {
    return this.writer.flush();
  }

  // MARK: - Composer

  setDraft(value: string): void {
    this.set({ draft: value });
    this.saveDraft();
  }

  setAttachments(value: readonly PiComposerAttachment[]): void {
    this.set({ attachments: [...value] });
    this.saveAttachmentDraft();
  }

  private setSubmissions(next: readonly PiSubmission[]): void {
    if (next.filter((x) => x.recoverable || x.status === 'Sending').length > RECOVERY_LIMIT)
      this.error =
        'Message recovery is full. Older recovery receipts could not be retained; inspect native history before sending again. Restore or dismiss saved recovery to make room.';
    this.set({ submissions: retainedSubmissions(next) });
    this.saveSubmissions();
  }

  addAttachment(file: PiComposerAttachment): void {
    if (
      this.s.attachments.length >= COMPOSER_LIMIT_COUNT ||
      attachmentBytes([...this.s.attachments, file]) > COMPOSER_LIMIT_BYTES
    ) {
      this.error = 'Attach up to four files, totaling 512 KiB after preparation.';
      return;
    }
    this.setAttachments([...this.s.attachments, file]);
  }

  removeAttachment(id: string): void {
    this.setAttachments(this.s.attachments.filter((x) => x.id !== id));
  }

  // MARK: - Connection

  /**
   * Bind the app-owned bridge's local chat (WIN-9.3 `onConnection`). The backend has already
   * saved its secret under `endpoint`.
   */
  bindManagedConnection(endpoint: string, clientId: string): void {
    if (!UUID.test(clientId)) {
      this.error = 'Invalid managed desktop identity.';
      return;
    }
    this.disconnect();
    this._clientId = clientId;
    this.setPref(KEY_CLIENT_ID, clientId);
    this.managedEndpoint = piEndpoint(endpoint);
    this.connectSaved(endpoint);
  }

  /** Save the secret for `endpoint` and connect to it. */
  async pair(endpoint: string, secret: string): Promise<void> {
    const address = piEndpoint(endpoint);
    if (address === null) {
      this.error = 'Enter a Tailscale address ending in /v1, such as ws://100.100.1.2:8787/v1.';
      return;
    }
    if (!validPairingSecret(secret)) {
      this.error = 'Enter the pairing secret from Point Guard (at least 32 characters).';
      return;
    }
    try {
      await this.secrets.save(secret, address);
    } catch (error) {
      this.error = describe(error);
      return;
    }
    this.connectSaved(address);
  }

  /**
   * Connect to `endpoint` with its saved secret: after Settings imports a pairing secret
   * (`services/pairingSecret.ts` `importPairing`), or from `pair`.
   */
  connectSaved(endpoint: string): void {
    const address = piEndpoint(endpoint);
    if (address === null) {
      this.error = 'Enter a Tailscale address ending in /v1, such as ws://100.100.1.2:8787/v1.';
      return;
    }
    this.saveDraft();
    this.saveAttachmentDraft();
    if (address !== this.managedEndpoint) this.managedEndpoint = null;
    this.disconnect();
    this.set({ endpoint: address, snapshot: null, transcriptRows: [], demo: false });
    this.setPref(KEY_ENDPOINT, address);
    this.setDraft(this.drafts.get(this.draftKey) ?? '');
    this.setAttachments(this.attachmentDrafts.get(this.draftKey) ?? []);
    this.wantsConnection = true;
    this.connect();
  }

  connect(): void {
    if (this.s.demo || !this.foreground || this.socket !== null || this.opening) return;
    const address = this.s.endpoint;
    if (piEndpoint(address) === null) {
      if (address !== '')
        this.error = 'Enter the bridge address ending in /v1, or scan its pairing QR.';
      return;
    }
    if (this.connectionTask !== null) return;
    this.wantsConnection = true;
    this.set({ connectionStatus: 'Connecting to Point Guard…' });
    const task = {};
    this.connectionTask = task;
    void (async () => {
      await this.recoveryTask;
      const saved = await this.secrets.has(address).catch(() => false);
      if (this.connectionTask !== task) return;
      this.connectionTask = null;
      if (
        !this.foreground ||
        !this.wantsConnection ||
        this.s.endpoint !== address ||
        this.socket !== null ||
        this.opening
      )
        return;
      this.startConnection(address, saved);
    })();
  }

  private startConnection(endpoint: string, saved: boolean): void {
    if (!saved) {
      this.error = 'Add the pairing secret in Connection settings.';
      return;
    }
    this.wantsConnection = true;
    this.clearRetry();
    const current = ++this.generation;
    this.cursor.reset();
    this.set({ connectionStatus: 'Connecting to Point Guard…', error: null });
    this.opening = true;
    this.transport
      .open(
        { endpoint, clientId: this._clientId },
        {
          onMessage: (text) => {
            if (current !== this.generation) return;
            try {
              this.receive(decodeIncoming(text));
            } catch (error) {
              this.connectionLost(describe(error));
            }
          },
          onClose: (close) => {
            if (current !== this.generation) return;
            this.connectionLost(
              close.reason === '' ? 'Point Guard closed the connection.' : close.reason,
            );
          },
        },
      )
      .then(async (socket) => {
        if (current !== this.generation) {
          socket.close();
          return;
        }
        this.opening = false;
        this.socket = socket;
        await this.syncState();
        if (current !== this.generation) return;
        this.set({ connected: true, connectionStatus: 'Connected to Point Guard' });
        this.retryDelay = 2;
      })
      .catch((error: unknown) => {
        if (current === this.generation) this.connectionLost(describe(error));
      });
  }

  setForeground(active: boolean): void {
    this.foreground = active;
    if (active) {
      if (this.wantsConnection) this.connect();
    } else {
      void this.writer.flush();
      this.detach();
      this.set({
        connectionStatus: `Paused on this ${LOCAL_DEVICE} · Point Guard keeps running`,
      });
    }
  }

  disconnect(): void {
    this.wantsConnection = false;
    this.clearRetry();
    this.detach();
    this.set({ connectionStatus: 'Not connected' });
  }

  private clearRetry(): void {
    if (this.retryTimer !== null) clearTimeout(this.retryTimer);
    this.retryTimer = null;
  }

  private detach(): void {
    this.generation++;
    this.connectionTask = null;
    this.opening = false;
    this.set({ connected: false });
    this.socket?.close();
    this.socket = null;
    const outstanding = [...this.pending.values()];
    this.pending = new Map();
    for (const p of outstanding) {
      clearTimeout(p.timer);
      p.reject(
        new Error(
          'Connection interrupted. Delivery may be uncertain. Inspect the conversation before sending again.',
        ),
      );
    }
  }

  private connectionLost(reason: string): void {
    this.detach();
    this.set({
      connectionStatus: `Connection lost · Point Guard continues on this ${LOCAL_DEVICE}`,
      error: `${reason} Check Point Guard on this ${LOCAL_DEVICE}, then its address and pairing secret.`,
    });
    if (!this.foreground || !this.wantsConnection) return;
    const delay = this.retryDelay;
    this.retryDelay = Math.min(this.retryDelay * 2, 30);
    this.clearRetry();
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null;
      this.connect();
    }, delay * this.retryUnitMs);
  }

  // MARK: - Records

  private receive(incoming: PiIncomingRecord): void {
    if (incoming.kind === 'snapshot') {
      this.apply(incoming.snapshot, incoming.rows);
      return;
    }
    const record = incoming.envelope;
    switch (record.type) {
      case 'receipt': {
        const id = record.id;
        const p = id === undefined ? undefined : this.pending.get(id);
        if (id === undefined || p === undefined) return;
        this.pending.delete(id);
        clearTimeout(p.timer);
        if (record.ok === true) p.resolve(record.data ?? null);
        else p.reject(new Error(record.error ?? 'Point Guard rejected this action.'));
        return;
      }
      case 'editor':
        if (record.id !== undefined && !this.seenEditorIds.has(record.id)) {
          this.seenEditorIds.add(record.id);
          this.set({ editorOffer: record.text ?? null });
        }
        return;
      default:
        throw new Error('Unsupported bridge response. Update the app and bridge together.');
    }
  }

  private apply(state: PiSnapshot, rows?: PiTranscriptRow[]): void {
    if (state.version !== 1)
      throw new Error('Unsupported bridge version. Update the app and bridge together.');
    if (!this.cursor.accept(state.epoch, state.revision)) return;
    const previous = this.s.snapshot?.sessionId ?? null;
    const localDraft = this.s.draft;
    const localAttachments = this.s.attachments;
    const oldKey = this.draftKey;
    if (previous !== state.sessionId) this.saveDraft();
    this.set({
      snapshot: state,
      transcriptRows: rows ?? makeTranscriptRows(state.messages, state.tools),
    });
    const previousCanceled = this.canceledIdHistory.join('\n');
    const mine = (state.canceled ?? []).filter((x) => x.clientId === this._clientId);
    const added = mine
      .filter((x) => !this.seenCanceledIds.has(x.id))
      .map((x): PiSubmission => ({
        id: x.id,
        text: x.text,
        sessionId: x.sessionId,
        status: 'Canceled by Stop',
        recoverable: true,
      }));
    for (const x of added) this.seenCanceledIds.add(x.id);
    if (added.length) this.setSubmissions([...this.s.submissions, ...added]);
    // Refresh the current snapshot's IDs in wire order so older history
    // cannot evict them while the bridge continues including them.
    this.canceledIdHistory = recentCanceledIds([
      ...this.canceledIdHistory,
      ...mine.map((x) => x.id),
    ]);
    this.seenCanceledIds = new Set(this.canceledIdHistory);
    if (previousCanceled !== this.canceledIdHistory.join('\n')) this.saveCanceledIds();
    const offered = state.editor;
    if (offered && !this.seenEditorIds.has(offered.id)) {
      this.seenEditorIds.add(offered.id);
      this.set({ editorOffer: offered.text });
    }
    if (previous !== state.sessionId) {
      const next = new PiDraftState(this.drafts.get(this.draftKey) ?? '');
      if (previous === null && localDraft !== '' && next.text !== localDraft) {
        next.restore(localDraft);
        this.drafts.delete(oldKey);
      }
      this.setDraft(next.text);
      const nextAttachments = [...(this.attachmentDrafts.get(this.draftKey) ?? [])];
      if (previous === null) {
        for (const file of localAttachments)
          if (!nextAttachments.some((x) => x.id === file.id)) nextAttachments.push(file);
        this.attachmentDrafts.delete(oldKey);
      }
      this.setAttachments(nextAttachments);
    }
    if (state.error != null) this.error = state.error;
  }

  private async syncState(): Promise<void> {
    const current = this.generation;
    const data = await this.call('sync');
    const state = decodeSnapshot(data);
    if (current !== this.generation)
      throw new Error('Connection changed while refreshing. Reconnect before continuing.');
    this.apply(state);
  }

  private call(
    op: string,
    fields: Record<string, JsonValue> = {},
    id = this.uuid(),
  ): Promise<JsonValue> {
    const socket = this.socket;
    if (socket === null) return Promise.reject(new Error('Connect to Point Guard before sending.'));
    if (this.pending.size >= PENDING_LIMIT)
      return Promise.reject(
        new Error('Too many actions in flight. Wait for Point Guard before trying again.'),
      );
    const record = { ...fields, version: 1, clientId: this._clientId, id, op };
    const text = JSON.stringify(record);
    return new Promise<JsonValue>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (!this.pending.delete(id)) return;
        reject(
          new Error(
            `No acknowledgement from Point Guard on this ${LOCAL_DEVICE}. Delivery is uncertain; inspect history before sending again.`,
          ),
        );
      }, this.receiptTimeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      socket.send(text).catch((error: unknown) => {
        const p = this.pending.get(id);
        if (p === undefined) return;
        this.pending.delete(id);
        clearTimeout(p.timer);
        reject(new Error(describe(error)));
      });
    });
  }

  // MARK: - Sending

  submit(mode: 'send' | 'steer' | 'after'): Promise<void> {
    if (!this.canSend) return Promise.resolve();
    return this.send(this.s.draft, [...this.s.attachments], mode, true);
  }

  /** Sends a Point Guard play's prompt, leaving any half-typed draft in place. */
  submitPlay(text: string): Promise<void> {
    if (!this.canSendPlay || text.trim() === '') return Promise.resolve();
    return this.send(text, [], 'send', false);
  }

  private async send(
    text: string,
    files: PiComposerAttachment[],
    mode: string,
    fromDraft: boolean,
  ): Promise<void> {
    const state = this.s.snapshot;
    if (state === null) return;
    if (
      this.s.submissions.filter((x) => x.recoverable || x.status === 'Sending').length >=
      RECOVERY_LIMIT
    ) {
      this.error =
        'Message recovery is full. Restore or dismiss a saved recovery before sending another message.';
      return;
    }
    let content;
    try {
      content = makeComposerPayload(text, files);
    } catch (error) {
      this.error = describe(error);
      return;
    }
    if (
      content.images.length > 0 &&
      !this.s.demo &&
      state.capabilities?.includes('images') !== true
    ) {
      this.error = 'Restart the updated Point Guard bridge before sending images.';
      return;
    }
    const id = this.uuid();
    this.setSubmissions([
      ...this.s.submissions,
      {
        id,
        text,
        sessionId: state.sessionId,
        status: 'Sending',
        recoverable: false,
        attachments: files,
      },
    ]);
    if (fromDraft) {
      this.setDraft('');
      this.setAttachments([]);
    }
    // Hydration and confirmed persistence precede any transmission, while
    // the composer remains editable. Sending receipts cannot be dismissed.
    await this.recoveryTask;
    const receipts = JSON.stringify(this.s.submissions);
    try {
      await this.writer.writeAndConfirm(() => this.recoveryStore.save('submissions', receipts));
    } catch {
      const guidance = `Message not sent: could not save recovery on this ${LOCAL_DEVICE}. Free storage, then restore the saved message and try again. Keep the app open to retain its text and files.`;
      this.updateSubmission(id, guidance, true);
      this.error = guidance;
      return;
    }
    try {
      if (this.s.demo) {
        await this.demoReply(content.message);
        this.updateSubmission(id, 'Accepted', false);
        return;
      }
      const data = await this.call(
        'send',
        {
          epoch: state.epoch,
          text: content.message,
          mode,
          images: content.images as unknown as JsonValue,
        },
        id,
      );
      const disposition = field(data, 'disposition');
      this.updateSubmission(
        id,
        disposition === 'queued'
          ? 'Queued'
          : disposition === 'handled'
            ? 'Handled by extension'
            : 'Accepted',
        false,
      );
    } catch (error) {
      this.updateSubmission(id, describe(error), true);
      this.error = describe(error);
    }
  }

  private updateSubmission(id: string, status: string, recoverable: boolean): void {
    if (!this.s.submissions.some((x) => x.id === id)) return;
    this.setSubmissions(
      this.s.submissions.map((x) => (x.id === id ? { ...x, status, recoverable } : x)),
    );
  }

  restore(submission: PiSubmission): void {
    if (this.s.submissions.find((x) => x.id === submission.id)?.status === 'Sending') {
      this.error = "Wait for this message's delivery result before restoring it.";
      return;
    }
    const files = (submission.attachments ?? []).filter(
      (file) => !this.s.attachments.some((x) => x.id === file.id),
    );
    if (
      this.s.attachments.length + files.length > COMPOSER_LIMIT_COUNT ||
      attachmentBytes([...this.s.attachments, ...files]) > COMPOSER_LIMIT_BYTES
    ) {
      this.error = 'Remove a draft attachment before restoring these files.';
      return;
    }
    const draft = new PiDraftState(this.s.draft);
    draft.restore(submission.text);
    this.setDraft(draft.text);
    this.setAttachments([...this.s.attachments, ...files]);
    this.setSubmissions(this.s.submissions.filter((x) => x.id !== submission.id));
  }

  dismissSubmission(id: string): void {
    if (this.s.submissions.find((x) => x.id === id)?.status === 'Sending') {
      this.error = "Wait for this message's delivery result before dismissing it.";
      return;
    }
    this.setSubmissions(this.s.submissions.filter((x) => x.id !== id));
  }

  useEditorOffer(): void {
    const offer = this.s.editorOffer;
    if (offer === null) return;
    const draft = new PiDraftState(this.s.draft);
    draft.restore(offer);
    this.setDraft(draft.text);
    this.set({ editorOffer: null });
  }

  dismissEditorOffer(): void {
    this.set({ editorOffer: null });
  }

  // MARK: - Run, history and sessions

  /** Stop the current run; true once it stopped (or nothing ran). */
  async stop(): Promise<boolean> {
    const state = this.s.snapshot;
    const run = state?.runId;
    if (!state || !run) return !this.busy;
    try {
      await this.call('stop', { epoch: state.epoch, runId: run });
      await this.syncState();
      return true;
    } catch (error) {
      this.error = describe(error);
      return false;
    }
  }

  async loadConversations(): Promise<void> {
    if (this.s.demo) {
      this.set({ conversations: [{ id: 'previous', title: 'A previous thought', date: null }] });
      return;
    }
    try {
      const value = field(await this.call('sessions'), 'sessions');
      this.set({ conversations: decodeConversations(value) });
    } catch (error) {
      this.error = describe(error);
    }
  }

  async browse(conversation: PiConversation): Promise<void> {
    if (this.s.demo) {
      this.set({
        browsing: {
          sessionId: conversation.id,
          title: conversation.title,
          messages: [
            { id: 'old', role: 'assistant', text: 'There is room here for a fresh idea.' },
          ],
        },
      });
      return;
    }
    try {
      const value = await this.call('history', { sessionId: conversation.id });
      this.set({ browsing: decodeHistory(value) });
    } catch (error) {
      this.error = describe(error);
    }
  }

  closeBrowsing(): void {
    this.set({ browsing: null });
  }

  /** New conversation (no `id`) or resume `id`; a busy run must be stopped first. */
  async changeSession(id: string | null = null, stopFirst = false): Promise<void> {
    const state = this.s.snapshot;
    if (this.s.changingSession || state === null) return;
    this.set({ changingSession: true });
    try {
      if (this.busy && !(stopFirst && (await this.stop()))) return;
      const fields: Record<string, JsonValue> = { epoch: state.epoch };
      if (id !== null) fields.sessionId = id;
      await this.call(id === null ? 'new' : 'resume', fields);
      await this.syncState();
      this.set({ browsing: null });
    } catch (error) {
      this.error = describe(error);
    } finally {
      this.set({ changingSession: false });
    }
  }

  async answer(
    dialog: PiExtensionDialog,
    answer: { value?: string; optionId?: string; confirmed?: boolean; cancelled?: boolean } = {},
  ): Promise<void> {
    const state = this.s.snapshot;
    if (state === null) return;
    const fields: Record<string, JsonValue> = {
      epoch: state.epoch,
      dialogId: dialog.id,
      cancelled: answer.cancelled ?? false,
    };
    if (answer.optionId !== undefined) fields.optionId = answer.optionId;
    if (answer.value !== undefined) fields.value = answer.value;
    if (answer.confirmed !== undefined) fields.confirmed = answer.confirmed;
    try {
      await this.call('answer', fields);
    } catch (error) {
      this.error = describe(error);
    }
  }

  // MARK: - Preview

  exploreDemo(): void {
    this.disconnect();
    this.cursor.reset();
    const snapshot: PiSnapshot = {
      version: 1,
      epoch: 'preview',
      revision: 1,
      busy: false,
      sessionId: 'preview',
      title: 'PurePoint',
      messages: [{ id: 'welcome', role: 'assistant', text: 'What would you like to work on?' }],
      tools: [],
      queue: [],
      dialogs: [],
      notices: [],
    };
    this.set({
      demo: true,
      connected: true,
      connectionStatus: 'Preview',
      error: null,
      snapshot,
      transcriptRows: makeTranscriptRows(snapshot.messages, snapshot.tools),
    });
  }

  private async demoReply(text: string): Promise<void> {
    const old = this.s.snapshot;
    if (old === null) return;
    const demoGeneration = this.generation;
    const messages = [...old.messages, { id: this.uuid(), role: 'user', text }];
    const running: PiSnapshot = {
      ...old,
      revision: old.revision + 1,
      busy: true,
      runId: 'preview-run',
      messages,
      tools: [
        { id: 'demo-tool', name: 'read', state: 'running', text: 'Reading the project notes…' },
      ],
    };
    this.set({
      snapshot: running,
      transcriptRows: makeTranscriptRows(running.messages, running.tools),
    });
    await new Promise((resolve) => setTimeout(resolve, this.demoDelayMs));
    if (!this.s.demo || this.generation !== demoGeneration) return;
    const done: PiSnapshot = {
      ...old,
      revision: old.revision + 2,
      busy: false,
      runId: null,
      messages: [
        ...messages,
        {
          id: this.uuid(),
          role: 'assistant',
          text: 'I’ve read the project notes. Here’s the next step.\n\n```ts\nconst nextStep = "Build something useful";\n```',
        },
      ],
      tools: [{ id: 'demo-tool', name: 'read', state: 'finished', text: 'Project notes read.' }],
    };
    this.set({ snapshot: done, transcriptRows: makeTranscriptRows(done.messages, done.tools) });
  }
}

/**
 * The managed runtime's chat hooks (WIN-9.3 `PointGuardServiceOptions`): bind the local chat
 * when Point Guard is ready and drop it when the owned runtime is gone, as the Swift
 * `PointGuardServiceModel` calls `chat.bindManagedConnection` and `chat.disconnect`.
 */
export function pointGuardChatBinding(chat: PiChatModel): {
  onConnection: (connection: PointGuardChatConnection) => void;
  onDisconnect: () => void;
} {
  return {
    onConnection: (connection) => {
      chat.bindManagedConnection(connection.endpoint, connection.clientId);
    },
    onDisconnect: () => {
      chat.disconnect();
    },
  };
}
