// Port of apps/purepoint-macos/purepoint-macosTests/StreamAccumulatorTests.swift (WIN-8.16).
// Each title keeps the Swift test function name. The two timer cases run on fake timers rather
// than waiting on the real 50 ms throttle.
import { afterEach, describe, expect, it, vi } from 'vitest';

import { makeChatMessage, type ChatMessage, type ContentBlock } from '../model/content';
import { STREAM_FLUSH_DELAY_MS, StreamAccumulator } from './sessions';

function assistant(id: string): ChatMessage[] {
  return [makeChatMessage({ id, role: 'assistant', isStreaming: true })];
}

function blocks(messages: ChatMessage[]): ContentBlock[] {
  return messages[0]?.contentBlocks ?? [];
}

function texts(messages: ChatMessage[]): string[] {
  return blocks(messages).flatMap((block) => (block.type === 'text' ? [block.text] : []));
}

afterEach(() => {
  vi.useRealTimers();
});

describe('StreamAccumulatorTests', () => {
  // MARK: - Delta accumulation

  it('givenTextDeltasShouldAccumulateAndFlush', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'Hello ' }, 'a1', messages);
    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'world!' }, 'a1', messages);

    accumulator.finalize('a1', messages);

    expect(texts(messages).join('')).toBe('Hello world!');
  });

  // MARK: - Tool use events

  it('givenToolBlockStartShouldFlushTextAndAppendTool', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle(
      { type: 'contentBlockDelta', index: 0, delta: 'Before tool.' },
      'a1',
      messages,
    );
    accumulator.handle(
      { type: 'contentBlockStart', index: 1, id: 't1', name: 'Read' },
      'a1',
      messages,
    );

    // Text is flushed before the tool block.
    expect(blocks(messages).some((block) => block.type === 'text')).toBe(true);
    expect(blocks(messages).some((block) => block.type === 'toolUse' && block.id === 't1')).toBe(
      true,
    );
  });

  // MARK: - Tool result preservation

  it('givenToolResultShouldPreserveThroughFinalization', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle(
      { type: 'contentBlockStart', index: 0, id: 't1', name: 'Bash' },
      'a1',
      messages,
    );
    accumulator.handle(
      { type: 'toolResult', toolUseId: 't1', content: 'output here', isError: false },
      'a1',
      messages,
    );
    accumulator.handle(
      {
        type: 'assistant',
        content: [{ type: 'toolUse', id: 't1', name: 'Bash', input: '{"cmd":"ls"}' }],
      },
      'a1',
      messages,
    );

    accumulator.finalize('a1', messages);

    const toolResults = blocks(messages).filter((block) => block.type === 'toolResult');
    expect(toolResults).toHaveLength(1);
    expect(toolResults[0]).toMatchObject({ toolUseId: 't1', output: 'output here' });
  });

  // MARK: - Failed tool status preserved

  it('givenFailedToolResultShouldPreserveFailedStatus', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle(
      { type: 'contentBlockStart', index: 0, id: 't1', name: 'Bash' },
      'a1',
      messages,
    );
    accumulator.handle(
      { type: 'toolResult', toolUseId: 't1', content: 'error', isError: true },
      'a1',
      messages,
    );
    accumulator.handle(
      { type: 'assistant', content: [{ type: 'toolUse', id: 't1', name: 'Bash', input: '{}' }] },
      'a1',
      messages,
    );

    accumulator.finalize('a1', messages);

    const statuses = blocks(messages).flatMap((block) =>
      block.type === 'toolUse' ? [block.status] : [],
    );
    expect(statuses).toEqual(['failed']);
  });

  // MARK: - Finalize with no deltas uses snapshot

  it('givenNoDeltas_finalizeShouldUseAssistantSnapshot', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle(
      { type: 'assistant', content: [{ type: 'text', text: 'Full text from snapshot' }] },
      'a1',
      messages,
    );

    accumulator.finalize('a1', messages);

    expect(texts(messages).join('')).toBe('Full text from snapshot');
  });

  it('givenDeltasPlusSnapshot_finalizeShouldPreserveDeltaText', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    // Deltas drive the text content.
    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'partial ' }, 'a1', messages);
    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'text' }, 'a1', messages);
    // The snapshot is stored but does not replace delta-driven text.
    accumulator.handle(
      { type: 'assistant', content: [{ type: 'text', text: 'partial text' }] },
      'a1',
      messages,
    );

    accumulator.finalize('a1', messages);

    // Delta-driven text is authoritative when blocks are non-empty.
    expect(texts(messages).join('')).toBe('partial text');
  });

  // MARK: - Reset clears state

  it('givenResetShouldClearAllBufferingState', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'partial' }, 'a1', messages);

    accumulator.reset();

    // After reset, finalize produces nothing (no pending blocks, no text).
    const freshMessages = assistant('a2');
    accumulator.finalize('a2', freshMessages);
    expect(blocks(freshMessages)).toEqual([]);
  });

  // MARK: - Session ID returned from result event

  it('givenResultEventShouldReturnSessionId', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    const result = accumulator.handle(
      { type: 'result', sessionId: 'sess-42', durationMs: 100 },
      'a1',
      messages,
    );

    expect(result).toEqual({ kind: 'sessionId', sessionId: 'sess-42' });
  });

  // MARK: - Error returned from error event

  it('givenErrorEventShouldReturnError', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    const result = accumulator.handle(
      { type: 'error', message: 'something broke' },
      'a1',
      messages,
    );

    expect(result).toEqual({ kind: 'error', message: 'something broke' });
  });

  // MARK: - Timer-based flush callback

  it('givenDeltaWithFlushHandler_shouldCallHandlerAfterDelay', () => {
    vi.useFakeTimers();
    const accumulator = new StreamAccumulator();
    const notified: string[] = [];
    accumulator.flushHandler = (messageId) => notified.push(messageId);
    const messages = assistant('a1');

    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'Hello' }, 'a1', messages);
    expect(notified).toEqual([]);

    vi.advanceTimersByTime(STREAM_FLUSH_DELAY_MS);
    expect(notified).toEqual(['a1']);
  });

  it('givenSynchronousFlush_shouldNotCallHandler', () => {
    vi.useFakeTimers();
    const accumulator = new StreamAccumulator();
    let handlerCalled = false;
    accumulator.flushHandler = () => {
      handlerCalled = true;
    };
    const messages = assistant('a1');

    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'Hello' }, 'a1', messages);
    // Flushing synchronously through finalize cancels the timer.
    accumulator.finalize('a1', messages);

    vi.advanceTimersByTime(100);

    expect(handlerCalled).toBe(false);
  });

  // MARK: - Post-tool text accumulation

  it('givenTextAfterToolShouldAppendCorrectly', () => {
    const accumulator = new StreamAccumulator();
    const messages = assistant('a1');

    accumulator.handle({ type: 'contentBlockDelta', index: 0, delta: 'Before.' }, 'a1', messages);
    accumulator.handle(
      { type: 'contentBlockStart', index: 1, id: 't1', name: 'Read' },
      'a1',
      messages,
    );
    accumulator.handle(
      { type: 'toolResult', toolUseId: 't1', content: 'file data', isError: false },
      'a1',
      messages,
    );
    accumulator.handle({ type: 'contentBlockDelta', index: 2, delta: 'After.' }, 'a1', messages);
    accumulator.handle(
      {
        type: 'assistant',
        content: [
          { type: 'text', text: 'Before.' },
          { type: 'toolUse', id: 't1', name: 'Read', input: '{}' },
          { type: 'text', text: 'After.' },
        ],
      },
      'a1',
      messages,
    );

    accumulator.finalize('a1', messages);

    expect(texts(messages)).toContain('Before.');
    expect(texts(messages)).toContain('After.');
  });
});
