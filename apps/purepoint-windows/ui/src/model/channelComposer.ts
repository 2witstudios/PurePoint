// The platform-neutral parts of apps/purepoint-macos/purepoint-macos/Views/Channel/
// ChannelComposer.swift (WIN-8.10): the `@` mention range NSTextView completes
// (`rangeForUserCompletion`), the completions it offers, the Return handling of `keyDown` and the
// editor's height clamp. ChannelComposer.tsx is the textarea that drives them.

/** A half-open UTF-16 range `[start, end)` of the composer text. */
export interface TextRange {
  readonly start: number;
  readonly end: number;
}

/** Swift `updateHeight`: content height plus insets, clamped to 60–180 pt. */
export const COMPOSER_MIN_HEIGHT = 60;
export const COMPOSER_MAX_HEIGHT = 180;

export function composerHeight(contentHeight: number): number {
  return Math.min(COMPOSER_MAX_HEIGHT, Math.max(COMPOSER_MIN_HEIGHT, contentHeight + 18));
}

/**
 * The partial mention before the caret: from the last `@` to the caret, when that token holds no
 * whitespace. Null when there is none (NSNotFound).
 */
export function mentionRange(text: string, caret: number): TextRange | null {
  if (caret < 0 || caret > text.length) return null;
  const prefix = text.slice(0, caret);
  const at = prefix.lastIndexOf('@');
  if (at < 0) return null;
  if (/\s/u.test(prefix.slice(at))) return null;
  return { start: at, end: caret };
}

/** `@name` for every mention that starts with the partial word, case-insensitively. */
export function mentionCompletions(mentions: readonly string[], partial: string): string[] {
  if (!partial.startsWith('@')) return [];
  const lowered = partial.toLowerCase();
  return mentions
    .map((name) => `@${name}`)
    .filter((word) => word.toLowerCase().startsWith(lowered));
}

/** Replace the partial range with the chosen completion; the caret lands after it. */
export function insertCompletion(
  text: string,
  range: TextRange,
  word: string,
): { text: string; caret: number } {
  return {
    text: text.slice(0, range.start) + word + text.slice(range.end),
    caret: range.start + word.length,
  };
}

/** What a key does in the composer. */
export type ComposerKeyAction =
  /** Enter without Shift, no IME composition, no completion list: send. */
  | 'submit'
  /** Enter or Tab with the completion list open: insert the selected mention. */
  | 'accept'
  /** Arrow keys with the list open: move the selection. */
  | 'previous'
  | 'next'
  /** Escape with the list open closes it; without it, asks for completions (NSTextView). */
  | 'close'
  | 'complete'
  /** Everything else edits the text normally (Shift+Enter inserts a newline). */
  | 'default';

export interface ComposerKey {
  readonly key: string;
  readonly shiftKey: boolean;
  /** IME composition in progress (`KeyboardEvent.isComposing`, Swift `hasMarkedText`). */
  readonly composing: boolean;
  readonly completionOpen: boolean;
}

export function composerKeyAction({
  key,
  shiftKey,
  composing,
  completionOpen,
}: ComposerKey): ComposerKeyAction {
  if (composing) return 'default';
  if (completionOpen) {
    switch (key) {
      case 'Enter':
      case 'Tab':
        return 'accept';
      case 'ArrowUp':
        return 'previous';
      case 'ArrowDown':
        return 'next';
      case 'Escape':
        return 'close';
    }
    return 'default';
  }
  if (key === 'Enter') return shiftKey ? 'default' : 'submit';
  if (key === 'Escape') return 'complete';
  return 'default';
}
