// Port of apps/purepoint-macos/purepoint-macos/Views/Channel/ChannelComposer.swift (WIN-8.10).
//
// The message editor: a plain-text textarea (IME composition and native editing shortcuts come
// with it, as NSTextView's do). Enter sends, Shift+Enter inserts a newline, and Enter during IME
// composition finishes the composition. Typing an `@` token offers the matching mentions in a
// list, as NSTextView's `complete(_:)`; Enter or Tab inserts the selected one, the arrows move
// the selection, and Escape closes the list (or, closed, asks for completions). The height follows
// the content, clamped to 60–180 px.
import { useLayoutEffect, useRef, useState, type KeyboardEvent } from 'react';
import {
  composerHeight,
  composerKeyAction,
  insertCompletion,
  mentionCompletions,
  mentionRange,
} from '../../model/channelComposer.ts';

export interface ChannelComposerProps {
  text: string;
  onTextChange: (text: string) => void;
  /** The editor's accessible label (Swift `setAccessibilityLabel(placeholder)`). */
  placeholder: string;
  /** The grey prompt shown while empty ("Message #project", "Reply to thread…"). */
  prompt: string;
  mentions: readonly string[];
  onSubmit: () => void;
}

interface Completion {
  readonly words: string[];
  readonly index: number;
}

export function ChannelComposer({
  text,
  onTextChange,
  placeholder,
  prompt,
  mentions,
  onSubmit,
}: ChannelComposerProps) {
  const editor = useRef<HTMLTextAreaElement>(null);
  const [height, setHeight] = useState(60);
  const [completion, setCompletion] = useState<Completion | null>(null);
  const pendingCaret = useRef<number | null>(null);

  useLayoutEffect(() => {
    const view = editor.current;
    if (view === null) return;
    if (pendingCaret.current !== null) {
      view.setSelectionRange(pendingCaret.current, pendingCaret.current);
      pendingCaret.current = null;
    }
    // Measure the content: collapse to the minimum, read scrollHeight (which includes padding).
    const previous = view.style.height;
    view.style.height = '0px';
    const content = view.scrollHeight - 16;
    view.style.height = previous;
    setHeight(composerHeight(Math.max(0, content)));
  }, [text]);

  /** Swift `complete(nil)`: offer the mentions for the token before the caret. */
  const complete = (value: string, caret: number) => {
    const range = mentionRange(value, caret);
    const words =
      range === null ? [] : mentionCompletions(mentions, value.slice(range.start, range.end));
    setCompletion(words.length === 0 ? null : { words, index: 0 });
  };

  const accept = (word: string) => {
    const view = editor.current;
    const caret = view?.selectionStart ?? text.length;
    const range = mentionRange(text, caret);
    setCompletion(null);
    if (range === null) return;
    const result = insertCompletion(text, range, word);
    pendingCaret.current = result.caret;
    onTextChange(result.text);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    const action = composerKeyAction({
      key: event.key,
      shiftKey: event.shiftKey,
      // 'Process': Chromium's key events during composition.
      composing: event.nativeEvent.isComposing || event.key === 'Process',
      completionOpen: completion !== null,
    });
    switch (action) {
      case 'submit':
        event.preventDefault();
        onSubmit();
        return;
      case 'accept':
        event.preventDefault();
        if (completion !== null) accept(completion.words[completion.index] ?? '');
        return;
      case 'previous':
      case 'next':
        event.preventDefault();
        if (completion !== null) {
          const step = action === 'next' ? 1 : -1;
          const count = completion.words.length;
          setCompletion({ ...completion, index: (completion.index + step + count) % count });
        }
        return;
      case 'close':
        event.preventDefault();
        event.stopPropagation();
        setCompletion(null);
        return;
      case 'complete':
        complete(text, event.currentTarget.selectionStart);
        return;
      case 'default':
        return;
    }
  };

  return (
    <div className="channel-composer-editor">
      <textarea
        ref={editor}
        className="channel-composer-text"
        aria-label={placeholder}
        placeholder={prompt}
        aria-autocomplete="list"
        aria-expanded={completion !== null}
        value={text}
        style={{ height }}
        onChange={(event) => {
          const value = event.target.value;
          onTextChange(value);
          const native = event.nativeEvent as InputEvent;
          if (native.isComposing) return;
          complete(value, event.target.selectionStart);
        }}
        onKeyDown={onKeyDown}
        onBlur={() => {
          setCompletion(null);
        }}
      />
      {completion !== null && (
        <ul className="channel-completions" role="listbox" aria-label="Mentions">
          {completion.words.map((word, index) => (
            <li
              key={word}
              role="option"
              aria-selected={index === completion.index}
              className={index === completion.index ? 'selected' : undefined}
              onMouseDown={(event) => {
                // Keep focus in the editor.
                event.preventDefault();
                accept(word);
              }}
            >
              {word}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
