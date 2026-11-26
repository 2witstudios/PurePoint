// An in-memory channel store answering `channel_*` requests with the engine's cursor rules
// (docs/product/project-channel.md, "Pagination"), for ChannelState and ProjectChannelView tests.
import type { ChannelMessage } from '../model/channelModel';
import type { Request, Response } from '../protocol';

export interface FakeChannelOptions {
  selfAuthorId?: string;
  selfName?: string;
}

export class FakeChannelDaemon {
  messages: ChannelMessage[] = [];
  revision = 0;
  latestSequence = 0;
  selfAuthorId: string;
  selfName: string;
  readonly requests: Request[] = [];
  /** Make the next request of this type fail with an `error` response. */
  failNext: Partial<Record<Request['type'], string>> = {};
  private nextId = 1;

  constructor(options: FakeChannelOptions = {}) {
    this.selfAuthorId = options.selfAuthorId ?? 'human:me';
    this.selfName = options.selfName ?? 'me';
  }

  /** Append a message as `author` (a human unless `agent` is given). */
  post(
    text: string,
    options: {
      authorId?: string;
      name?: string;
      agentType?: string;
      parentId?: string;
      createdAt?: string;
      branch?: string;
    } = {},
  ): ChannelMessage {
    this.latestSequence += 1;
    this.revision += 1;
    const authorId = options.authorId ?? 'human:other';
    const message: ChannelMessage = {
      id: `msg-${String(this.nextId++)}`,
      sequence: this.latestSequence,
      parent_id: options.parentId ?? null,
      author: {
        id: authorId,
        name: options.name ?? (authorId === this.selfAuthorId ? this.selfName : authorId),
        kind: options.agentType === undefined ? 'human' : 'agent',
        agent_type: options.agentType ?? null,
        worktree_id: null,
        branch: options.branch ?? null,
      },
      text,
      created_at: options.createdAt ?? '2026-10-09T17:00:00+00:00',
      edited_at: null,
      references: [],
      reactions: [],
    };
    this.messages.push(message);
    return message;
  }

  readonly send = (request: Request): Promise<Response> => {
    this.requests.push(request);
    const failure = this.failNext[request.type];
    if (failure !== undefined) {
      this.failNext = Object.fromEntries(
        Object.entries(this.failNext).filter(([type]) => type !== request.type),
      );
      return Promise.resolve({ type: 'error', code: 'ERR', message: failure });
    }
    return Promise.resolve(this.answer(request));
  };

  reads(): Extract<Request, { type: 'channel_read' }>[] {
    return this.requests.filter(
      (request): request is Extract<Request, { type: 'channel_read' }> =>
        request.type === 'channel_read',
    );
  }

  private answer(request: Request): Response {
    switch (request.type) {
      case 'channel_read':
        return this.read(request);
      case 'channel_send': {
        const message = this.post(request.text, {
          authorId: this.selfAuthorId,
          ...(request.parent_id == null ? {} : { parentId: request.parent_id }),
        });
        return { type: 'channel_message', message, revision: this.revision };
      }
      case 'channel_edit': {
        const index = this.messages.findIndex((message) => message.id === request.message_id);
        const current = this.messages[index];
        if (current === undefined) return { type: 'error', code: 'ERR', message: 'not found' };
        if (current.author.id !== this.selfAuthorId) {
          return { type: 'error', code: 'ERR', message: 'only the sender can edit a message' };
        }
        const message = { ...current, text: request.text, edited_at: '2026-10-09T18:00:00+00:00' };
        this.messages[index] = message;
        this.revision += 1;
        return { type: 'channel_message', message, revision: this.revision };
      }
      case 'channel_react': {
        const index = this.messages.findIndex((message) => message.id === request.message_id);
        const current = this.messages[index];
        if (current === undefined) return { type: 'error', code: 'ERR', message: 'not found' };
        const emoji = request.emoji ?? '👍';
        const others = current.reactions.filter((reaction) => reaction.emoji !== emoji);
        const existing = current.reactions.find((reaction) => reaction.emoji === emoji);
        const ids = new Set(existing?.author_ids ?? []);
        if (request.active) ids.add(this.selfAuthorId);
        else ids.delete(this.selfAuthorId);
        const reactions =
          ids.size === 0 ? others : [...others, { emoji, author_ids: [...ids].sort() }];
        const message = { ...current, reactions };
        this.messages[index] = message;
        this.revision += 1;
        return { type: 'channel_message', message, revision: this.revision };
      }
      default:
        return { type: 'error', code: 'ERR', message: `unexpected ${request.type}` };
    }
  }

  private read(request: Extract<Request, { type: 'channel_read' }>): Response {
    const limit = Math.min(request.limit ?? 100, 200);
    const after = request.after ?? null;
    const before = request.before ?? null;
    const parent = request.parent_id ?? null;
    const query = request.query ?? null;
    const filtered = after === null && before === null && parent === null && query === null;
    const base = {
      type: 'channel_history' as const,
      revision: this.revision,
      latest_sequence: this.latestSequence,
      self_author_id: this.selfAuthorId,
    };
    if (filtered && request.known_revision === this.revision) {
      return {
        ...base,
        messages: [],
        has_more: false,
        oldest_sequence: null,
        unchanged: true,
        reply_counts: {},
      };
    }
    let pool = this.messages;
    if (parent !== null) pool = pool.filter((message) => message.parent_id === parent);
    if (query !== null) {
      const lowered = query.toLowerCase();
      pool = pool.filter(
        (message) =>
          message.text.toLowerCase().includes(lowered) ||
          message.author.name.toLowerCase().includes(lowered),
      );
    }
    let window: ChannelMessage[];
    let hasMore: boolean;
    if (after !== null) {
      const newer = pool.filter((message) => message.sequence > after);
      window = newer.slice(0, limit);
      hasMore = newer.length > limit;
    } else {
      const older = before === null ? pool : pool.filter((message) => message.sequence < before);
      window = older.slice(Math.max(0, older.length - limit));
      hasMore = older.length > limit;
    }
    // Parent context for loaded replies: deduplicated, never changing cursors or has_more.
    const oldest = window[0]?.sequence ?? null;
    if (parent === null) {
      const ids = new Set(window.map((message) => message.id));
      for (const message of [...window]) {
        const parentMessage = this.messages.find((candidate) => candidate.id === message.parent_id);
        if (parentMessage !== undefined && !ids.has(parentMessage.id)) {
          ids.add(parentMessage.id);
          window.push(parentMessage);
        }
      }
      window.sort((a, b) => a.sequence - b.sequence);
    }
    const replyCounts: Record<string, number> = {};
    for (const message of window) {
      if (message.parent_id !== null) continue;
      const count = this.messages.filter((reply) => reply.parent_id === message.id).length;
      if (count > 0) replyCounts[message.id] = count;
    }
    return {
      ...base,
      messages: window,
      has_more: hasMore,
      oldest_sequence: oldest,
      unchanged: false,
      reply_counts: replyCounts,
    };
  }
}
