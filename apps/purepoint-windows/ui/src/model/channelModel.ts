// Port of apps/purepoint-macos/purepoint-macos/Models/ChannelModel.swift (WIN-8.10).
//
// The channel wire types are generated from pu-core (`ChannelAuthor`, `ChannelReference`,
// `ChannelReaction`, `ChannelMessage`); `ChannelHistory` and `ChannelMutation` are the two
// channel responses. The display helpers below are the private formatting functions of
// ProjectChannelView.swift, kept pure so they can be tested.
import type {
  ChannelAuthor,
  ChannelMessage,
  ChannelReaction,
  ChannelReference,
  Response,
} from '../protocol';

export type { ChannelAuthor, ChannelMessage, ChannelReaction, ChannelReference };

/** `channel_history`: one read window (oldest to newest) and the store's cursors. */
export type ChannelHistory = Omit<Extract<Response, { type: 'channel_history' }>, 'type'>;

/** `channel_message`: the message a send, edit or reaction wrote. */
export type ChannelMutation = Omit<Extract<Response, { type: 'channel_message' }>, 'type'>;

/** The reaction the hover row and the reaction chips toggle (Swift hard-codes it). */
export const THUMBS_UP = '👍';

/** Swift `ISO8601DateFormatter`: internet date-time, with or without fractional seconds. */
export function channelDate(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:?\d{2})$/.test(value)) return null;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? null : date;
}

/** `Date.formatted(date: .omitted, time: .shortened)`, e.g. "5:00 PM". */
export function channelTime(value: string, locale?: string): string {
  const date = channelDate(value);
  return date === null
    ? value
    : new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' }).format(date);
}

/** `Date.formatted(date: .abbreviated, time: .omitted)`, e.g. "Oct 9, 2026". */
export function channelDay(value: string, locale?: string): string {
  const date = channelDate(value);
  return date === null
    ? value.slice(0, 10)
    : new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric', year: 'numeric' }).format(
        date,
      );
}

/** Whether `message` continues `previous`'s group: same author, same day, under five minutes. */
export function isGrouped(
  message: ChannelMessage,
  previous: ChannelMessage | null | undefined,
  locale?: string,
): boolean {
  if (previous === null || previous === undefined) return false;
  if (previous.author.id !== message.author.id) return false;
  const a = channelDate(message.created_at);
  const b = channelDate(previous.created_at);
  if (a === null || b === null) return false;
  return (
    (a.getTime() - b.getTime()) / 1000 < 300 &&
    channelDay(message.created_at, locale) === channelDay(previous.created_at, locale)
  );
}

/** The avatar's initials: the first letter of the first two words of the name. */
export function authorInitials(name: string): string {
  return name
    .split(' ')
    .filter((word) => word !== '')
    .slice(0, 2)
    .map((word) => Array.from(word)[0] ?? '')
    .join('');
}

/** A reference chip's title: its label, else `PR #n` or the commit's first eight characters. */
export function referenceTitle(reference: ChannelReference): string {
  if (reference.label !== null && reference.label !== undefined) return reference.label;
  return reference.kind === 'pr' ? `PR #${reference.value}` : reference.value.slice(0, 8);
}

/** `"1 reply"` / `"n replies"`. */
export function replyCountTitle(count: number): string {
  return `${String(count)} ${count === 1 ? 'reply' : 'replies'}`;
}

/** The distinct human authors of loaded messages, by name (the members list and mentions). */
export function humanAuthors(messages: readonly ChannelMessage[]): ChannelAuthor[] {
  const byId = new Map<string, ChannelAuthor>();
  for (const message of messages) {
    if (message.author.kind === 'human') byId.set(message.author.id, message.author);
  }
  return [...byId.values()].sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
}
