// Port of apps/purepoint-macos/purepoint-macos/State/{SessionListState,StreamAccumulator}.swift
// (WIN-8.16): the indexed agent sessions with search and time-period grouping, over
// `services/conversations.ts`, and the delta-buffering state machine that turns a streamed
// assistant response into content blocks.
import { createStore, type StoreApi } from 'zustand/vanilla';

import {
  splitContentBlocks,
  type ChatMessage,
  type ContentBlock,
  type StreamContentBlock,
  type StreamEvent,
} from '../model/content';
import {
  conversationSnippets,
  listConversations,
  type AgentSource,
  type Conversation,
  type ListConversationsOptions,
} from '../services/conversations';

// MARK: - ConversationSection

export type ConversationSectionId = 'today' | 'yesterday' | 'this-week' | 'this-month' | 'older';

/** `ConversationSection`. */
export interface ConversationSection {
  readonly id: ConversationSectionId;
  readonly title: string;
  readonly sessions: Conversation[];
}

function startOfDay(ms: number): number {
  const date = new Date(ms);
  date.setHours(0, 0, 0, 0);
  return date.getTime();
}

function addingDays(ms: number, days: number): number {
  const date = new Date(ms);
  date.setDate(date.getDate() + days);
  return date.getTime();
}

/**
 * Whole days from `from` to `to` in the local calendar, as Foundation's
 * `dateComponents([.day], from:to:)` counts them (a day keeps its wall-clock time across DST).
 */
function wholeDaysBetween(from: number, to: number): number {
  if (to < from) return -wholeDaysBetween(to, from);
  let days = Math.floor((to - from) / 86_400_000);
  while (days > 0 && addingDays(from, days) > to) days -= 1;
  while (addingDays(from, days + 1) <= to) days += 1;
  return days;
}

/**
 * `ConversationSection.grouped`: conversations by time period (today, yesterday, this week, this
 * month, older), each in input order, dropping repeated ids and empty periods.
 */
export function groupConversations(
  conversations: readonly Conversation[],
  now: number = Date.now(),
): ConversationSection[] {
  const today: Conversation[] = [];
  const yesterday: Conversation[] = [];
  const thisWeek: Conversation[] = [];
  const thisMonth: Conversation[] = [];
  const older: Conversation[] = [];

  const todayStart = startOfDay(now);
  const yesterdayStart = addingDays(todayStart, -1);
  const tomorrowStart = addingDays(todayStart, 1);

  const seen = new Set<string>();
  for (const session of conversations) {
    if (seen.has(session.id)) continue;
    seen.add(session.id);
    const date = session.modifiedAt;
    if (date >= todayStart && date < tomorrowStart) {
      today.push(session);
    } else if (date >= yesterdayStart && date < todayStart) {
      yesterday.push(session);
    } else {
      const daysAgo = wholeDaysBetween(date, now);
      if (daysAgo < 7) thisWeek.push(session);
      else if (daysAgo < 30) thisMonth.push(session);
      else older.push(session);
    }
  }

  const sections: ConversationSection[] = [];
  if (today.length > 0) sections.push({ id: 'today', title: 'Today', sessions: today });
  if (yesterday.length > 0) {
    sections.push({ id: 'yesterday', title: 'Yesterday', sessions: yesterday });
  }
  if (thisWeek.length > 0) {
    sections.push({ id: 'this-week', title: 'This Week', sessions: thisWeek });
  }
  if (thisMonth.length > 0) {
    sections.push({ id: 'this-month', title: 'This Month', sessions: thisMonth });
  }
  if (older.length > 0) sections.push({ id: 'older', title: 'Older', sessions: older });
  return sections;
}

/** `localizedCaseInsensitiveContains`. */
function containsIgnoringCase(haystack: string, needle: string): boolean {
  return haystack.toLocaleLowerCase().includes(needle.toLocaleLowerCase());
}

/** `SessionListState.filteredSessions`: sessions matching the trimmed query (all when empty). */
export function filterConversations(
  sessions: readonly Conversation[],
  searchQuery: string,
): Conversation[] {
  const query = searchQuery.trim();
  if (query === '') return [...sessions];
  return sessions.filter((session) => {
    const haystacks = [
      session.title,
      session.projectName,
      session.workspaceName,
      session.projectPath ?? '',
      session.gitBranch ?? '',
      session.agentSource,
      ...session.previewSnippets,
    ];
    return haystacks.some((haystack) => containsIgnoringCase(haystack, query));
  });
}

// MARK: - SessionListState

export interface SessionListSnapshot {
  readonly sessions: Conversation[];
  readonly searchQuery: string;
  readonly isLoadingSessions: boolean;
}

/** The index commands `SessionListState` reads, injectable for tests. */
export interface SessionListService {
  list(source: AgentSource, options: ListConversationsOptions): Promise<Conversation[]>;
  snippets(transcriptPath: string): Promise<string[]>;
}

const CONVERSATION_SERVICE: SessionListService = {
  list: listConversations,
  snippets: (transcriptPath) => conversationSnippets(transcriptPath),
};

const newestFirst = (a: Conversation, b: Conversation) => b.modifiedAt - a.modifiedAt;

async function orEmpty<T>(promise: Promise<T[]>): Promise<T[]> {
  try {
    return await promise;
  } catch {
    return [];
  }
}

/** `SessionListState`: every agent's indexed sessions, searchable and grouped by period. */
export class SessionListState {
  readonly store: StoreApi<SessionListSnapshot>;
  private readonly service: SessionListService;
  private readonly now: () => number;

  constructor(options: { service?: SessionListService; now?: () => number } = {}) {
    this.service = options.service ?? CONVERSATION_SERVICE;
    this.now = options.now ?? Date.now;
    this.store = createStore<SessionListSnapshot>()(() => ({
      sessions: [],
      searchQuery: '',
      isLoadingSessions: false,
    }));
  }

  get sessions(): Conversation[] {
    return this.store.getState().sessions;
  }

  set sessions(sessions: Conversation[]) {
    this.store.setState({ sessions });
  }

  get searchQuery(): string {
    return this.store.getState().searchQuery;
  }

  set searchQuery(searchQuery: string) {
    this.store.setState({ searchQuery });
  }

  get isLoadingSessions(): boolean {
    return this.store.getState().isLoadingSessions;
  }

  get filteredSessions(): Conversation[] {
    return filterConversations(this.sessions, this.searchQuery);
  }

  get groupedSessions(): ConversationSection[] {
    return groupConversations(this.filteredSessions, this.now());
  }

  /**
   * Phase 1 loads the Claude, Codex and OpenCode indexes together; phase 2 adds Claude's loose
   * transcripts and enriches Codex sessions with their metadata; phase 3 fills in Claude preview
   * snippets. A source that fails contributes nothing. Does nothing while a refresh is running.
   */
  async refreshSessions(): Promise<void> {
    if (this.isLoadingSessions) return;
    this.store.setState({ isLoadingSessions: true });

    const [claude, codex, opencode] = await Promise.all([
      orEmpty(this.service.list('claude', { phase: 'indexed' })),
      orEmpty(this.service.list('codex', { phase: 'indexed' })),
      orEmpty(this.service.list('opencode', { phase: 'indexed' })),
    ]);
    const all = [...claude, ...codex, ...opencode].sort(newestFirst);
    this.sessions = all;

    const existingClaudeIds = all
      .filter((session) => session.agentSource === 'claude')
      .map((session) => session.sessionId);
    const [loose, enrichedCodex] = await Promise.all([
      orEmpty(this.service.list('claude', { phase: 'loose', excluding: existingClaudeIds })),
      orEmpty(this.service.list('codex', { phase: 'loose' })),
    ]);

    let sessions = this.sessions;
    if (loose.length > 0) sessions = [...sessions, ...loose].sort(newestFirst);
    if (enrichedCodex.length > 0) {
      const enriched = new Map(enrichedCodex.map((session) => [session.id, session]));
      sessions = sessions.map((session) => enriched.get(session.id) ?? session);
    }
    this.store.setState({ sessions, isLoadingSessions: false });

    await this.enrichSnippets();
  }

  /** Preview snippets for the first `limit` Claude sessions that have none. */
  async enrichSnippets(limit = 50): Promise<void> {
    const toEnrich = this.sessions
      .slice(0, limit)
      .filter(
        (session) =>
          session.previewSnippets.length === 0 &&
          session.agentSource === 'claude' &&
          session.transcriptPath !== null,
      );
    if (toEnrich.length === 0) return;

    const results = await Promise.all(
      toEnrich.map(async (session) => {
        const snippets = await orEmpty(this.service.snippets(session.transcriptPath ?? ''));
        return [session.id, snippets] as const;
      }),
    );
    const byId = new Map(results.filter(([, snippets]) => snippets.length > 0));
    if (byId.size === 0) return;
    this.sessions = this.sessions.map((session) => {
      const snippets = byId.get(session.id);
      return snippets === undefined ? session : { ...session, previewSnippets: snippets };
    });
  }
}

// MARK: - StreamAccumulator

/** `StreamEventResult`: what the owner of the chat must act on. */
export type StreamEventResult =
  { kind: 'none' } | { kind: 'sessionId'; sessionId: string } | { kind: 'error'; message: string };

const NONE: StreamEventResult = { kind: 'none' };

/** The delay before buffered deltas are flushed to the UI. */
export const STREAM_FLUSH_DELAY_MS = 50;

function lastIndexOf(messages: readonly ChatMessage[], id: string): number {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    if (messages[i]?.id === id) return i;
  }
  return -1;
}

/**
 * `StreamAccumulator`: buffers text deltas of a streamed assistant response, flushes them on a
 * throttle, and reconciles with the final assistant snapshot. Like Swift's `inout`, `messages` is
 * updated in place; the message being changed is replaced with a new object.
 */
export class StreamAccumulator {
  private streamingTextValue = '';
  private pendingFlush: ReturnType<typeof setTimeout> | null = null;
  private pendingAssistantBlocks: StreamContentBlock[] | null = null;

  /** Called when the throttle fires, with the assistant message ID; the owner flushes the text. */
  flushHandler: ((assistantMessageId: string) => void) | null = null;

  get streamingText(): string {
    return this.streamingTextValue;
  }

  /** Process one stream event. */
  handle(
    event: StreamEvent,
    assistantMessageId: string,
    messages: ChatMessage[],
  ): StreamEventResult {
    const index = lastIndexOf(messages, assistantMessageId);
    if (index === -1) return NONE;

    switch (event.type) {
      case 'assistant':
        this.pendingAssistantBlocks = event.content;
        return NONE;
      case 'contentBlockStart':
        this.flushPendingDeltas(index, messages);
        this.updateBlocks(messages, index, (blocks) => [
          ...blocks,
          { type: 'toolUse', id: event.id, name: event.name, input: '{}', status: 'running' },
        ]);
        this.streamingTextValue = '';
        return NONE;
      case 'contentBlockDelta':
        this.streamingTextValue += event.delta;
        this.scheduleFlush(assistantMessageId);
        return NONE;
      case 'toolResult':
        this.flushPendingDeltas(index, messages);
        this.updateBlocks(messages, index, (blocks) => [
          ...blocks.map((block): ContentBlock =>
            block.type === 'toolUse' && block.id === event.toolUseId
              ? { ...block, status: event.isError ? 'failed' : 'completed' }
              : block,
          ),
          {
            type: 'toolResult',
            id: crypto.randomUUID(),
            toolUseId: event.toolUseId,
            output: event.content,
            isError: event.isError,
          },
        ]);
        this.streamingTextValue = '';
        return NONE;
      case 'result':
        this.flushPendingDeltas(index, messages);
        return { kind: 'sessionId', sessionId: event.sessionId };
      case 'error':
        this.flushPendingDeltas(index, messages);
        return { kind: 'error', message: event.message };
      case 'unknown':
        return NONE;
    }
  }

  /**
   * Apply the last assistant snapshot as the authoritative final state, after the stream ends:
   * when deltas built the blocks, the snapshot only fills in tool inputs (and completes running
   * tools); otherwise its blocks become the message's.
   */
  finalize(assistantMessageId: string, messages: ChatMessage[]): void {
    this.cancelFlush();

    const index = lastIndexOf(messages, assistantMessageId);
    if (index === -1) {
      this.pendingAssistantBlocks = null;
      return;
    }

    this.flushPendingDeltas(index, messages);

    const blocks = this.pendingAssistantBlocks;
    if (blocks === null) return;

    const current = messages[index]?.contentBlocks ?? [];
    if (current.length > 0) {
      this.updateBlocks(messages, index, (existing) => {
        const next = [...existing];
        for (const block of blocks) {
          if (block.type !== 'toolUse') continue;
          const i = next.findIndex((b) => b.type === 'toolUse' && b.id === block.id);
          const found = next[i];
          if (found?.type !== 'toolUse') continue;
          next[i] = {
            ...found,
            input: block.input,
            status: found.status === 'running' ? 'completed' : found.status,
          };
        }
        return next;
      });
    } else {
      const appended: ContentBlock[] = [];
      for (const block of blocks) {
        if (block.type === 'text') {
          appended.push(...splitContentBlocks(block.text, appended.length));
        } else {
          appended.push({
            type: 'toolUse',
            id: block.id,
            name: block.name,
            input: block.input,
            status: 'completed',
          });
        }
      }
      this.updateBlocks(messages, index, (existing) => [...existing, ...appended]);
    }

    this.pendingAssistantBlocks = null;
    this.streamingTextValue = '';
  }

  /** Cancel the pending flush and clear all buffering state. */
  reset(): void {
    this.cancelFlush();
    this.pendingAssistantBlocks = null;
    this.streamingTextValue = '';
  }

  /** Apply the buffered text to the message's blocks now, replacing text since the last tool. */
  flushPendingDeltas(messageIndex: number, messages: ChatMessage[]): void {
    this.cancelFlush();
    if (messageIndex >= messages.length || this.streamingTextValue === '') return;
    const text = this.streamingTextValue;

    this.updateBlocks(messages, messageIndex, (blocks) => {
      let lastToolIndex = -1;
      blocks.forEach((block, i) => {
        if (block.type === 'toolUse' || block.type === 'toolResult') lastToolIndex = i;
      });
      const textStartIndex = lastToolIndex + 1;
      const split = splitContentBlocks(text, textStartIndex);
      if (textStartIndex >= blocks.length) return [...blocks, ...split];
      const kept = blocks
        .slice(textStartIndex)
        .filter((block) => block.type !== 'text' && block.type !== 'codeBlock');
      return [...blocks.slice(0, textStartIndex), ...split, ...kept];
    });
  }

  // MARK: Private

  private scheduleFlush(assistantMessageId: string): void {
    if (this.pendingFlush !== null) return;
    this.pendingFlush = setTimeout(() => {
      this.pendingFlush = null;
      this.flushHandler?.(assistantMessageId);
    }, STREAM_FLUSH_DELAY_MS);
  }

  private cancelFlush(): void {
    if (this.pendingFlush !== null) clearTimeout(this.pendingFlush);
    this.pendingFlush = null;
  }

  private updateBlocks(
    messages: ChatMessage[],
    index: number,
    body: (blocks: readonly ContentBlock[]) => ContentBlock[],
  ): void {
    const message = messages[index];
    if (message === undefined) return;
    messages[index] = { ...message, contentBlocks: body(message.contentBlocks) };
  }
}
