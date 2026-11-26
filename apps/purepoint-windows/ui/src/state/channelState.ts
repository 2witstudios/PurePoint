// Port of apps/purepoint-macos/purepoint-macos/State/ChannelState.swift (WIN-8.10).
//
// One project's channel: the loaded history pages (newest window plus explicitly loaded older
// pages), the open thread, drafts, the saved read cursor and the unread count the sidebar shows.
// It polls `channel_read` with the store revision while visible (3 s) and in the background
// (15 s), refreshing every loaded page and the open thread when the revision changes.
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in a Zustand store (`channel.store`);
// what Swift marks `@ObservationIgnored` stays private. Swift's `UserDefaults` is an injected
// `Storage`-shaped key/value store (localStorage by default) with the same keys:
// `channel.<scope>.draft`, `.read`, `.seen` and `.reply.<message id>`.
//
// Differences from Swift:
// - Task cancellation is a generation token: a read that finishes after `shutdown`, a new
//   search or a mark-unread is dropped, as Swift's `token == generation` check drops it.
// - `seen` is stored as a JSON array of strings (Swift stores a string array).

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { Request, Response } from '../protocol';
import { THUMBS_UP, type ChannelHistory, type ChannelMessage } from '../model/channelModel';
import { browserStorage } from './storage';

/** The `UserDefaults` subset ChannelState uses; DOM `Storage` satisfies it. */
export interface ChannelStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export function memoryChannelStorage(): ChannelStorage {
  const values = new Map<string, string>();
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => {
      values.set(key, value);
    },
    removeItem: (key) => {
      values.delete(key);
    },
  };
}

export interface ChannelStateFields {
  readonly messages: readonly ChannelMessage[];
  readonly replyCounts: Readonly<Record<string, number>>;
  readonly selfAuthorId: string;
  readonly readError: string | null;
  readonly mutationError: string | null;
  readonly isLoading: boolean;
  readonly isSending: boolean;
  readonly hasMore: boolean;
  readonly latestSequence: number;
  readonly readSequence: number;
  readonly draft: string;
  readonly query: string;
  readonly threadId: string | null;
  readonly threadMessages: readonly ChannelMessage[];
  readonly threadHasMore: boolean;
  readonly unreadReplyTarget: string | null;
  readonly replyDraft: string;
  /** Swift's computed `unreadCount`, kept current whenever its inputs change. */
  readonly unreadCount: number;
}

export interface ChannelStateOptions {
  readonly projectRoot: string;
  /** Prefix scope for persisted drafts and read marks: the project's key (its root locally). */
  readonly defaultsScope?: string;
  readonly storage?: ChannelStorage;
  readonly sendRequest: (request: Request) => Promise<Response>;
  /** The poll interval's sleep; resolves early when `signal` aborts. */
  readonly sleep?: (ms: number, signal: AbortSignal) => Promise<void>;
}

/** Poll intervals: while a channel view is visible, and in the background (sidebar badge). */
export const VISIBLE_POLL_MS = 3_000;
export const BACKGROUND_POLL_MS = 15_000;

function defaultSleep(ms: number, signal: AbortSignal): Promise<void> {
  return new Promise((resolve) => {
    if (signal.aborted) {
      resolve();
      return;
    }
    const timer = setTimeout(resolve, ms);
    signal.addEventListener('abort', () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

class ChannelFailure extends Error {}

function errorMessage(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

function deduplicated(values: readonly ChannelMessage[]): ChannelMessage[] {
  const byId = new Map<string, ChannelMessage>();
  for (const value of values) byId.set(value.id, value);
  return [...byId.values()].sort((a, b) => a.sequence - b.sequence);
}

function parseSequence(value: string | null): number {
  if (value === null || !/^\d+$/.test(value)) return 0;
  return Number(value);
}

export class ChannelState {
  readonly projectRoot: string;
  readonly defaultsScope: string;
  readonly store: StoreApi<ChannelStateFields>;

  private readonly sendRequest: (request: Request) => Promise<Response>;
  private readonly storage: ChannelStorage;
  private readonly sleep: (ms: number, signal: AbortSignal) => Promise<void>;
  private polling: AbortController | null = null;
  private reading = false;
  private readWaiters: (() => void)[] = [];
  private refreshing = false;
  private refreshAgain = false;
  private revision: number | null = null;
  private oldest: number | null = null;
  private generation = 0;
  private viewers = 0;
  private backgroundEnabled = false;
  private loadedBefore: number[] = [];
  private threadOldest: number | null = null;
  private unreadAuthors = new Map<number, string>();
  private unreadMessages = new Map<number, ChannelMessage>();
  private seenSequences = new Set<number>();
  private unreadScannedThrough = 0;

  constructor(options: ChannelStateOptions) {
    this.projectRoot = options.projectRoot;
    this.defaultsScope = options.defaultsScope ?? options.projectRoot;
    this.sendRequest = options.sendRequest;
    this.storage = options.storage ?? browserStorage() ?? memoryChannelStorage();
    this.sleep = options.sleep ?? defaultSleep;
    let seen: unknown;
    try {
      seen = JSON.parse(this.storage.getItem(this.key('seen')) ?? '[]');
    } catch {
      seen = [];
    }
    if (Array.isArray(seen)) {
      for (const value of seen) {
        if (typeof value === 'string' && /^\d+$/.test(value)) this.seenSequences.add(Number(value));
      }
    }
    this.store = createStore<ChannelStateFields>()(() => ({
      messages: [],
      replyCounts: {},
      selfAuthorId: '',
      readError: null,
      mutationError: null,
      isLoading: false,
      isSending: false,
      hasMore: false,
      latestSequence: 0,
      readSequence: parseSequence(this.storage.getItem(this.key('read'))),
      draft: this.storage.getItem(this.key('draft')) ?? '',
      query: '',
      threadId: null,
      threadMessages: [],
      threadHasMore: false,
      unreadReplyTarget: null,
      replyDraft: '',
      unreadCount: 0,
    }));
  }

  // MARK: Observable accessors

  private get s(): ChannelStateFields {
    return this.store.getState();
  }

  private set(fields: Partial<ChannelStateFields>): void {
    this.store.setState(fields);
  }

  get messages(): readonly ChannelMessage[] {
    return this.s.messages;
  }
  get threadMessages(): readonly ChannelMessage[] {
    return this.s.threadMessages;
  }
  get threadId(): string | null {
    return this.s.threadId;
  }
  get hasMore(): boolean {
    return this.s.hasMore;
  }
  get threadHasMore(): boolean {
    return this.s.threadHasMore;
  }
  get selfAuthorId(): string {
    return this.s.selfAuthorId;
  }
  get readSequence(): number {
    return this.s.readSequence;
  }
  get latestSequence(): number {
    return this.s.latestSequence;
  }
  get query(): string {
    return this.s.query;
  }
  get unreadCount(): number {
    return this.s.unreadCount;
  }
  get unreadReplyTarget(): string | null {
    return this.s.unreadReplyTarget;
  }
  /** `mutationError ?? readError`. */
  get error(): string | null {
    return this.s.mutationError ?? this.s.readError;
  }
  get topLevelMessages(): ChannelMessage[] {
    return this.s.messages.filter((message) => message.parent_id === null);
  }

  get draft(): string {
    return this.s.draft;
  }
  get replyDraft(): string {
    return this.s.replyDraft;
  }

  /** The channel draft, persisted as it changes (Swift's `didSet`). */
  setDraft(value: string): void {
    this.set({ draft: value });
    this.storage.setItem(this.key('draft'), value);
  }

  /** The open thread's reply draft, persisted per thread. */
  setReplyDraft(value: string): void {
    this.set({ replyDraft: value });
    this.saveReplyDraft();
  }

  /** The search text; `search()` applies it. */
  setQuery(value: string): void {
    this.set({ query: value });
  }

  dismissMutationError(): void {
    this.set({ mutationError: null });
  }

  private key(suffix: string): string {
    return `channel.${this.defaultsScope}.${suffix}`;
  }

  private publishUnread(): void {
    const { readSequence, selfAuthorId } = this.s;
    let count = 0;
    for (const [sequence, author] of this.unreadAuthors) {
      if (sequence > readSequence && author !== selfAuthorId && !this.seenSequences.has(sequence)) {
        count += 1;
      }
    }
    if (count !== this.s.unreadCount) this.set({ unreadCount: count });
  }

  // MARK: Polling

  /** A channel view appeared: poll every 3 s. */
  start(): void {
    this.viewers += 1;
    this.ensurePolling();
  }

  /** The project opened: keep the sidebar badge current every 15 s. */
  startBackground(): void {
    this.backgroundEnabled = true;
    this.ensurePolling();
  }

  private ensurePolling(): void {
    if (this.polling !== null) return;
    const controller = new AbortController();
    this.polling = controller;
    void (async () => {
      const aborted = () => controller.signal.aborted;
      while (!aborted()) {
        await this.refresh();
        if (aborted()) return;
        await this.sleep(
          this.viewers === 0 ? BACKGROUND_POLL_MS : VISIBLE_POLL_MS,
          controller.signal,
        );
      }
    })();
  }

  /** A channel view disappeared. */
  stop(): void {
    this.viewers = Math.max(0, this.viewers - 1);
    if (this.viewers === 0 && !this.backgroundEnabled) this.cancelPolling();
  }

  shutdown(): void {
    this.cancelPolling();
    this.viewers = 0;
    this.backgroundEnabled = false;
    this.generation += 1;
  }

  private cancelPolling(): void {
    this.polling?.abort();
    this.polling = null;
  }

  // MARK: Reads

  private async history(
    options: { after?: number; before?: number; parent?: string; known?: number | null } = {},
  ): Promise<ChannelHistory> {
    const { after, before, parent, known } = options;
    const query = this.s.query;
    const response = await this.sendRequest({
      type: 'channel_read',
      project_root: this.projectRoot,
      after: after ?? null,
      before: before ?? null,
      query: after !== undefined || parent !== undefined || query === '' ? null : query,
      parent_id: parent ?? null,
      known_revision: known ?? null,
    });
    switch (response.type) {
      case 'channel_history':
        return response;
      case 'error':
        throw new ChannelFailure(response.message);
      default:
        throw new ChannelFailure('Unexpected channel response');
    }
  }

  /** Apply the query: start over from the latest window. */
  async search(): Promise<void> {
    this.generation += 1;
    this.loadedBefore = [];
    this.oldest = null;
    this.revision = null;
    await this.refresh();
  }

  private async acquireRead(): Promise<void> {
    if (this.reading) {
      await new Promise<void>((resolve) => {
        this.readWaiters.push(resolve);
      });
    } else {
      this.reading = true;
    }
  }

  private releaseRead(): void {
    const next = this.readWaiters.shift();
    if (next === undefined) this.reading = false;
    else next();
  }

  async refresh(): Promise<void> {
    if (this.refreshing) {
      this.refreshAgain = true;
      return;
    }
    this.refreshing = true;
    try {
      await this.acquireRead();
      try {
        await this.refreshLoaded();
      } finally {
        this.releaseRead();
      }
    } finally {
      this.refreshing = false;
      if (this.refreshAgain) {
        this.refreshAgain = false;
        void this.refresh();
      }
    }
  }

  private async refreshLoaded(): Promise<void> {
    const token = this.generation;
    this.set({ isLoading: this.s.messages.length === 0 });
    try {
      const latest = await this.history({ known: this.s.query === '' ? this.revision : null });
      if (token !== this.generation) return;
      this.set({ selfAuthorId: latest.self_author_id, latestSequence: latest.latest_sequence });
      if (!latest.unchanged) {
        let all = [...latest.messages];
        const counts = { ...latest.reply_counts };
        let oldestCursor = latest.oldest_sequence;
        let more = latest.has_more;
        const desiredOldest = this.loadedBefore.length === 0 ? null : this.oldest;
        const refreshedCursors: number[] = [];
        while (
          more &&
          oldestCursor !== null &&
          desiredOldest !== null &&
          oldestCursor > desiredOldest
        ) {
          const page = await this.history({ before: oldestCursor });
          if (token !== this.generation) return;
          refreshedCursors.push(oldestCursor);
          all = [...all, ...page.messages];
          Object.assign(counts, page.reply_counts);
          oldestCursor = page.oldest_sequence;
          more = page.has_more;
        }
        this.loadedBefore = refreshedCursors;
        this.oldest = oldestCursor;
        this.revision = latest.revision;
        this.set({ messages: deduplicated(all), replyCounts: counts, hasMore: more });
        const threadId = this.s.threadId;
        if (threadId !== null) await this.refreshThread(threadId, token);
      }
      await this.scanUnread(token);
      this.set({ readError: null });
    } catch (error) {
      if (token === this.generation) this.set({ readError: errorMessage(error) });
    } finally {
      this.set({ isLoading: false });
      this.publishUnread();
    }
  }

  /** Walk the messages created after the read cursor, so unread counts cover unloaded pages. */
  private async scanUnread(token: number): Promise<void> {
    let cursor = Math.max(this.s.readSequence, this.unreadScannedThrough);
    while (cursor < this.s.latestSequence) {
      const page = await this.history({ after: cursor });
      if (token !== this.generation) return;
      const created = page.messages.filter((message) => message.sequence > cursor);
      if (created.length === 0) break;
      for (const message of created) {
        this.unreadAuthors.set(message.sequence, message.author.id);
        this.unreadMessages.set(message.sequence, message);
      }
      cursor = Math.max(...created.map((message) => message.sequence));
      this.unreadScannedThrough = cursor;
      if (!page.has_more) break;
    }
    this.advanceReadCursor();
  }

  async loadOlder(): Promise<void> {
    await this.acquireRead();
    try {
      const cursor = this.oldest;
      if (cursor === null || !this.s.hasMore) return;
      const token = this.generation;
      try {
        const page = await this.history({ before: cursor });
        if (token !== this.generation) return;
        this.loadedBefore.push(cursor);
        this.oldest = page.oldest_sequence;
        this.set({
          messages: deduplicated([...this.s.messages, ...page.messages]),
          replyCounts: { ...this.s.replyCounts, ...page.reply_counts },
          hasMore: page.has_more,
        });
      } catch (error) {
        this.set({ readError: errorMessage(error) });
      }
    } finally {
      this.releaseRead();
    }
  }

  async openThread(id: string): Promise<void> {
    this.saveReplyDraft();
    this.threadOldest = null;
    this.set({
      threadId: id,
      threadMessages: [],
      threadHasMore: false,
      unreadReplyTarget: null,
      replyDraft: this.storage.getItem(this.key(`reply.${id}`)) ?? '',
    });
    await this.acquireRead();
    try {
      if (this.s.threadId !== id) return;
      try {
        await this.refreshThread(id, this.generation);
      } catch (error) {
        this.set({ readError: errorMessage(error) });
      }
    } finally {
      this.releaseRead();
    }
  }

  private async refreshThread(id: string, token: number): Promise<void> {
    const page = await this.history({ parent: id });
    if (token !== this.generation || this.s.threadId !== id) return;
    let all = page.messages.filter((message) => message.parent_id === id);
    let cursor = page.oldest_sequence;
    let more = page.has_more;
    // Refresh all previously loaded replies, including edits outside the newest page.
    const previousOldest = this.threadOldest;
    while (more && cursor !== null && previousOldest !== null && cursor > previousOldest) {
      const older = await this.history({ before: cursor, parent: id });
      if (token !== this.generation || this.s.threadId !== id) return;
      all = [...all, ...older.messages.filter((message) => message.parent_id === id)];
      cursor = older.oldest_sequence;
      more = older.has_more;
    }
    this.threadOldest = cursor;
    this.set({ threadMessages: deduplicated(all), threadHasMore: more });
  }

  async loadOlderReplies(): Promise<void> {
    await this.acquireRead();
    try {
      const id = this.s.threadId;
      const cursor = this.threadOldest;
      if (id === null || cursor === null || !this.s.threadHasMore) return;
      try {
        const page = await this.history({ before: cursor, parent: id });
        if (this.s.threadId !== id) return;
        this.threadOldest = page.oldest_sequence;
        this.set({
          threadMessages: deduplicated([
            ...this.s.threadMessages,
            ...page.messages.filter((message) => message.parent_id === id),
          ]),
          threadHasMore: page.has_more,
        });
      } catch (error) {
        this.set({ readError: errorMessage(error) });
      }
    } finally {
      this.releaseRead();
    }
  }

  saveReplyDraft(): void {
    const threadId = this.s.threadId;
    if (threadId !== null) this.storage.setItem(this.key(`reply.${threadId}`), this.s.replyDraft);
  }

  /** Close the open thread, keeping its reply draft. */
  closeThread(): void {
    this.saveReplyDraft();
    this.set({ threadId: null });
  }

  // MARK: Unread

  /** A message scrolled into view (at least half visible) while the window is active. */
  markVisible(message: ChannelMessage): void {
    if (this.s.query !== '') return;
    this.seenSequences.add(message.sequence);
    this.advanceReadCursor();
  }

  private advanceReadCursor(): void {
    let readSequence = this.s.readSequence;
    const selfAuthorId = this.s.selfAuthorId;
    while (readSequence < this.unreadScannedThrough) {
      const next = readSequence + 1;
      const author = this.unreadAuthors.get(next);
      if (author === undefined || (author !== selfAuthorId && !this.seenSequences.has(next))) break;
      readSequence = next;
      this.seenSequences.delete(next);
      this.unreadAuthors.delete(next);
      this.unreadMessages.delete(next);
    }
    this.set({ readSequence });
    this.persistReadMarks();
    this.publishUnread();
  }

  private persistReadMarks(): void {
    this.storage.setItem(this.key('read'), String(this.s.readSequence));
    this.storage.setItem(this.key('seen'), JSON.stringify([...this.seenSequences].map(String)));
  }

  firstUnread(): ChannelMessage | null {
    const { readSequence, selfAuthorId } = this.s;
    let first: ChannelMessage | null = null;
    for (const message of this.unreadMessages.values()) {
      if (
        message.sequence > readSequence &&
        message.author.id !== selfAuthorId &&
        !this.seenSequences.has(message.sequence) &&
        (first === null || message.sequence < first.sequence)
      ) {
        first = message;
      }
    }
    return first;
  }

  /** Load pages (and the thread) until the first unread message is loaded; returns its ID. */
  async revealFirstUnread(): Promise<string | null> {
    const first = this.firstUnread();
    if (first === null) return null;
    while (!this.s.messages.some((message) => message.id === first.id) && this.s.hasMore) {
      const previous = this.oldest;
      await this.loadOlder();
      if (this.oldest === previous) break;
    }
    const parent = first.parent_id;
    if (parent !== null) {
      await this.openThread(parent);
      while (
        this.s.threadId === parent &&
        !this.s.threadMessages.some((message) => message.id === first.id) &&
        this.s.threadHasMore
      ) {
        const previous = this.threadOldest;
        await this.loadOlderReplies();
        if (this.threadOldest === previous) break;
      }
      if (this.s.threadId === parent) this.set({ unreadReplyTarget: first.id });
    }
    return first.id;
  }

  markUnread(message: ChannelMessage): void {
    this.generation += 1;
    const readSequence = Math.min(this.s.readSequence, message.sequence - 1);
    this.seenSequences = new Set([...this.seenSequences].filter((s) => s < message.sequence));
    this.unreadScannedThrough = Math.min(this.unreadScannedThrough, readSequence);
    this.set({ readSequence });
    this.persistReadMarks();
    this.publishUnread();
    void this.refresh();
  }

  markRead(): void {
    if (this.s.query !== '') return;
    this.unreadAuthors = new Map();
    this.unreadMessages = new Map();
    this.seenSequences = new Set();
    this.set({ readSequence: this.s.latestSequence });
    this.storage.setItem(this.key('read'), String(this.s.readSequence));
    this.storage.removeItem(this.key('seen'));
    this.publishUnread();
  }

  // MARK: Writes

  async send(reply = false): Promise<void> {
    const targetParent = reply ? this.s.threadId : null;
    const text = reply ? this.s.replyDraft : this.s.draft;
    if (this.s.isSending || text.trim() === '') return;
    if (reply) this.saveReplyDraft();
    this.set({ isSending: true });
    try {
      await this.mutate({
        type: 'channel_send',
        project_root: this.projectRoot,
        text,
        parent_id: targetParent,
      });
      if (reply && targetParent !== null) {
        const savedKey = this.key(`reply.${targetParent}`);
        if (this.storage.getItem(savedKey) === text) this.storage.removeItem(savedKey);
        if (this.s.threadId === targetParent && this.s.replyDraft === text) {
          this.set({ replyDraft: '' });
          this.saveReplyDraft();
        }
      } else if (!reply && this.s.draft === text) {
        this.setDraft('');
      }
      this.set({ mutationError: null });
      this.revision = null;
      await this.refresh();
    } catch (error) {
      this.set({
        mutationError: `Send was not confirmed. Check the channel before trying again. ${errorMessage(error)}`,
      });
    } finally {
      this.set({ isSending: false });
    }
  }

  async edit(message: ChannelMessage, text: string): Promise<void> {
    try {
      await this.mutate({
        type: 'channel_edit',
        project_root: this.projectRoot,
        message_id: message.id,
        text,
      });
      this.set({ mutationError: null });
      this.revision = null;
      await this.refresh();
    } catch (error) {
      this.set({ mutationError: errorMessage(error) });
    }
  }

  /** Toggle this client's 👍 on the message. */
  async react(message: ChannelMessage): Promise<void> {
    const selfAuthorId = this.s.selfAuthorId;
    const active = !message.reactions.some(
      (reaction) => reaction.emoji === THUMBS_UP && reaction.author_ids.includes(selfAuthorId),
    );
    try {
      await this.mutate({
        type: 'channel_react',
        project_root: this.projectRoot,
        message_id: message.id,
        active,
      });
      this.set({ mutationError: null });
      this.revision = null;
      await this.refresh();
    } catch (error) {
      this.set({ mutationError: errorMessage(error) });
    }
  }

  private async mutate(request: Request): Promise<void> {
    const response = await this.sendRequest(request);
    switch (response.type) {
      case 'channel_message':
        return;
      case 'error':
        throw new ChannelFailure(response.message);
      default:
        throw new ChannelFailure('Unexpected channel response');
    }
  }
}
