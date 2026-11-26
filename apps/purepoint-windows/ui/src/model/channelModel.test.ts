// WIN-8.10: the channel wire types against the shared Swift fixture, and the display helpers of
// ProjectChannelView.swift.
import { describe, expect, it } from 'vitest';
import fixture from '../../../../../docs/reference/fixtures/channel-history.json?raw';
import type { Response } from '../protocol';
import { inTimeZone } from '../test/timeZone';
import {
  authorInitials,
  channelDate,
  channelDay,
  channelTime,
  humanAuthors,
  isGrouped,
  referenceTitle,
  replyCountTitle,
  type ChannelHistory,
  type ChannelMessage,
} from './channelModel';

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error('missing test value');
  return value;
}

const history = JSON.parse(fixture) as Extract<Response, { type: 'channel_history' }>;

function message(overrides: Partial<ChannelMessage> = {}): ChannelMessage {
  return { ...must(history.messages[0]), ...overrides };
}

describe('channel wire fixture', () => {
  it('decodes every field of channel-history.json', () => {
    const decoded: ChannelHistory = history;
    expect(decoded.revision).toBe(11);
    expect(decoded.latest_sequence).toBe(9);
    expect(decoded.has_more).toBe(true);
    expect(decoded.oldest_sequence).toBe(7);
    expect(decoded.unchanged).toBe(false);
    expect(decoded.self_author_id).toBe('human:501');
    expect(decoded.reply_counts).toEqual({ 'msg-parent': 3 });
    const [parent, reply] = decoded.messages;
    expect(parent).toMatchObject({
      id: 'msg-parent',
      sequence: 1,
      parent_id: null,
      text: 'Ready for `review`?',
      edited_at: '2026-10-09T17:01:00+00:00',
      references: [{ kind: 'pr', value: '42', label: 'Review PR' }],
      reactions: [{ emoji: '👍', author_ids: ['ag-fixture', 'human:501'] }],
    });
    expect(reply?.author).toEqual({
      id: 'ag-fixture',
      name: 'Channel builder',
      kind: 'agent',
      agent_type: 'codex',
      worktree_id: 'wt-fixture',
      branch: 'pu/channel',
    });
    expect(reply?.text).toBe('Yes — checks passed.\nRésumé included.');
  });
});

describe('channel display helpers', () => {
  it('parses ISO 8601 with and without fractional seconds', () => {
    expect(channelDate('2026-10-09T17:00:00+00:00')?.toISOString()).toBe(
      '2026-10-09T17:00:00.000Z',
    );
    expect(channelDate('2026-10-09T17:00:00.250Z')?.toISOString()).toBe('2026-10-09T17:00:00.250Z');
    expect(channelDate('yesterday')).toBeNull();
    expect(channelDate('')).toBeNull();
  });

  inTimeZone('UTC', () => {
    it('formats times and days like Date.formatted', () => {
      expect(channelTime('2026-10-09T17:00:00+00:00', 'en-US')).toBe('5:00 PM');
      expect(channelDay('2026-10-09T17:00:00+00:00', 'en-US')).toBe('Oct 9, 2026');
      expect(channelTime('not a date', 'en-US')).toBe('not a date');
      expect(channelDay('2026-10-09 garbage', 'en-US')).toBe('2026-10-09');
    });

    it('groups the same author within five minutes on the same day', () => {
      const first = message({ created_at: '2026-10-09T17:00:00+00:00' });
      expect(isGrouped(message({ created_at: '2026-10-09T17:04:59+00:00' }), first)).toBe(true);
      expect(isGrouped(message({ created_at: '2026-10-09T17:05:00+00:00' }), first)).toBe(false);
      expect(isGrouped(message(), null)).toBe(false);
      const other = message({ author: { ...first.author, id: 'someone-else' } });
      expect(isGrouped(other, first)).toBe(false);
      const late = message({ created_at: '2026-10-09T23:59:00+00:00' });
      expect(isGrouped(message({ created_at: '2026-10-10T00:01:00+00:00' }), late)).toBe(false);
      expect(isGrouped(message({ created_at: '' }), first)).toBe(false);
    });
  });

  it('builds initials, reference titles and reply counts', () => {
    expect(authorInitials('Channel builder')).toBe('Cb');
    expect(authorInitials('jono')).toBe('j');
    expect(authorInitials('a b c')).toBe('ab');
    expect(referenceTitle({ kind: 'pr', value: '42', label: null })).toBe('PR #42');
    expect(referenceTitle({ kind: 'pr', value: '42', label: 'Review PR' })).toBe('Review PR');
    expect(referenceTitle({ kind: 'commit', value: 'abcdef0123456789', label: null })).toBe(
      'abcdef01',
    );
    expect(replyCountTitle(1)).toBe('1 reply');
    expect(replyCountTitle(3)).toBe('3 replies');
  });

  it('lists distinct human authors by name', () => {
    const a = message({ author: { ...must(history.messages[0]).author, id: 'h1', name: 'zed' } });
    const b = message({ author: { ...must(history.messages[0]).author, id: 'h2', name: 'amy' } });
    expect(humanAuthors([a, b, a, must(history.messages[1])]).map((author) => author.name)).toEqual(
      ['amy', 'zed'],
    );
  });
});
