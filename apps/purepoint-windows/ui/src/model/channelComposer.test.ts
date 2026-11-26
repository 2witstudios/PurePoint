// WIN-8.10: the shared composer logic. These are the platform-neutral cases of
// purepoint-macosTests/ChannelComposerIOSTests.swift (Return, mention range, completions,
// completion acceptance and the height clamp); the UIKit plumbing they also exercise is iOS-only.
import { describe, expect, it } from 'vitest';
import {
  composerHeight,
  composerKeyAction,
  insertCompletion,
  mentionCompletions,
  mentionRange,
} from './channelComposer';

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error('missing test value');
  return value;
}

const key = (k: string, options: { shift?: boolean; composing?: boolean; open?: boolean } = {}) =>
  composerKeyAction({
    key: k,
    shiftKey: options.shift ?? false,
    composing: options.composing ?? false,
    completionOpen: options.open ?? false,
  });

describe('channel composer', () => {
  it('returnSendsAndInsertsNothing', () => {
    expect(key('Enter')).toBe('submit');
  });

  it('shiftReturnInsertsANewlineWithoutSending', () => {
    expect(key('Enter', { shift: true })).toBe('default');
  });

  it('otherTextIsInsertedNormally', () => {
    expect(key('a')).toBe('default');
    expect(key('!')).toBe('default');
  });

  it('given IME composition, Return finishes the composition instead of sending', () => {
    expect(key('Enter', { composing: true })).toBe('default');
    expect(key('Enter', { composing: true, open: true })).toBe('default');
  });

  it('mentionRangeEndsAtTheCaretAndStopsAtWhitespace', () => {
    expect(mentionRange('hey @Al', 7)).toEqual({ start: 4, end: 7 });
    expect(mentionRange('hey @Al ice', 11)).toBeNull();
    expect(mentionRange('no mention', 10)).toBeNull();
    // The caret, not the end of the text, ends the token.
    expect(mentionRange('hey @Al ice', 6)).toEqual({ start: 4, end: 6 });
  });

  it('completionsMatchMentionsCaseInsensitively', () => {
    const mentions = ['Alice', 'alfred', 'Bob'];
    expect(mentionCompletions(mentions, '@al')).toEqual(['@Alice', '@alfred']);
    expect(mentionCompletions(mentions, 'al')).toEqual([]);
    expect(mentionCompletions(mentions, '@')).toEqual(['@Alice', '@alfred', '@Bob']);
  });

  it('openCompletionTakesReturnAndInsertsTheMention', () => {
    expect(key('Enter', { open: true })).toBe('accept');
    expect(key('Tab', { open: true })).toBe('accept');
    const text = 'hey @bo';
    const range = mentionRange(text, text.length);
    expect(range).not.toBeNull();
    const [first] = mentionCompletions(
      ['Bob', 'Bobby'],
      text.slice(must(range).start, must(range).end),
    );
    expect(insertCompletion(text, must(range), must(first))).toEqual({
      text: 'hey @Bob',
      caret: 8,
    });
    // With the list closed, Return sends again.
    expect(key('Enter')).toBe('submit');
  });

  it('completionClosesWhenNothingMatches', () => {
    expect(mentionCompletions(['Bob'], '@zz')).toEqual([]);
    expect(key('Escape', { open: true })).toBe('close');
    expect(key('Escape')).toBe('complete');
    expect(key('ArrowDown', { open: true })).toBe('next');
    expect(key('ArrowUp', { open: true })).toBe('previous');
  });

  it('heightFollowsTheMacClamp', () => {
    expect(composerHeight(17)).toBe(60);
    expect(composerHeight(40 * 17)).toBe(180);
    expect(composerHeight(80)).toBe(98);
  });
});
