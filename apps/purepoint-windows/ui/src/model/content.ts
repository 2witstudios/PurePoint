// Port of apps/purepoint-macos/purepoint-macos/Models/{ChatMessage,ContentBlockSplitter}.swift
// (WIN-8.16): the chat message model shared by transcripts (`services/conversations.ts`) and the
// stream accumulator (`state/sessions.ts`), and the Markdown code-fence splitter. Also the
// `StreamEvent` shape the backend's `conversations/stream_event.rs` serializes. Dates are Unix ms.

/** `ChatRole`. */
export type ChatRole = 'user' | 'assistant';

/** `ToolUseStatus`. */
export type ToolUseStatus = 'running' | 'completed' | 'failed';

/** `PulseEvent`. */
export interface PulseEvent {
  readonly agent: string;
  readonly event: string;
}

/** `PulseSummary`. */
export interface PulseSummary {
  readonly activeAgents: number;
  readonly recentEvents: readonly PulseEvent[];
}

/** `ContentBlock`; every case carries its `id`. */
export type ContentBlock =
  | { type: 'text'; id: string; text: string }
  | { type: 'codeBlock'; id: string; language: string | null; code: string }
  | { type: 'toolUse'; id: string; name: string; input: string; status: ToolUseStatus }
  | { type: 'toolResult'; id: string; toolUseId: string; output: string; isError: boolean }
  | { type: 'pulse'; id: string; summary: PulseSummary };

/** `ContentBlock` as a transcript yields it: never a Point Guard `pulse`. */
export type TranscriptContentBlock = Exclude<ContentBlock, { type: 'pulse' }>;

/** `ChatMessage`. */
export interface ChatMessage {
  id: string;
  role: ChatRole;
  timestamp: number;
  contentBlocks: ContentBlock[];
  isStreaming: boolean;
}

/** `ChatMessage.init` defaults: a fresh UUID, now, no blocks, not streaming. */
export function makeChatMessage(
  init: Pick<ChatMessage, 'role'> & Partial<Omit<ChatMessage, 'role'>>,
): ChatMessage {
  return {
    id: init.id ?? crypto.randomUUID(),
    role: init.role,
    timestamp: init.timestamp ?? Date.now(),
    contentBlocks: init.contentBlocks ?? [],
    isStreaming: init.isStreaming ?? false,
  };
}

// MARK: - Stream events

/** `StreamContentBlock`: a block of an `assistant` snapshot. */
export type StreamContentBlock =
  { type: 'text'; text: string } | { type: 'toolUse'; id: string; name: string; input: string };

/** `StreamEvent`: one line of `claude --output-format stream-json`, as the backend parses it. */
export type StreamEvent =
  | { type: 'assistant'; content: StreamContentBlock[] }
  | { type: 'contentBlockStart'; index: number; id: string; name: string }
  | { type: 'contentBlockDelta'; index: number; delta: string }
  | { type: 'toolResult'; toolUseId: string; content: string; isError: boolean }
  | { type: 'result'; sessionId: string; durationMs: number | null }
  | { type: 'error'; message: string }
  | { type: 'unknown' };

// MARK: - ContentBlockSplitter

/** Swift's `.whitespaces`: horizontal whitespace, not line breaks. */
function trimWhitespace(value: string): string {
  return value.replace(/^[^\S\r\n]+|[^\S\r\n]+$/gu, '');
}

function isClosingFence(line: string): boolean {
  const trimmed = trimWhitespace(line);
  if (!trimmed.startsWith('```')) return false;
  return /^\s*$/u.test(trimmed.slice(3));
}

/**
 * `ContentBlockSplitter.split`: Markdown text as text and fenced code blocks. IDs are stable and
 * index-based (`cb-<n>`) so repeated calls diff cleanly; pass `startIndex` to keep IDs unique
 * when appending several results. A fence left open at the end is plain text.
 */
export function splitContentBlocks(text: string, startIndex = 0): ContentBlock[] {
  if (text === '') return [];

  const lines = text.split('\n');
  const blocks: ContentBlock[] = [];
  let currentText: string[] = [];
  let codeLines: string[] = [];
  let codeLanguage: string | null = null;
  let openingFenceLine: string | null = null;
  let savedText: string[] = [];
  let inCodeBlock = false;
  let blockIndex = startIndex;

  const flushText = () => {
    const joined = currentText.join('\n').trim();
    if (joined !== '') {
      blocks.push({ type: 'text', id: `cb-${String(blockIndex)}`, text: joined });
      blockIndex += 1;
    }
    currentText = [];
  };

  for (const line of lines) {
    if (!inCodeBlock) {
      if (line.startsWith('```')) {
        // Optimistically enter a code block, keeping the text before it in case it never closes.
        savedText = currentText;
        currentText = [];
        const langPart = trimWhitespace(line.slice(3));
        codeLanguage = langPart === '' ? null : langPart;
        openingFenceLine = line;
        inCodeBlock = true;
        codeLines = [];
      } else {
        currentText.push(line);
      }
    } else if (isClosingFence(line)) {
      // The code block is confirmed: flush the text before it.
      currentText = savedText;
      flushText();
      savedText = [];
      blocks.push({
        type: 'codeBlock',
        id: `cb-${String(blockIndex)}`,
        language: codeLanguage,
        code: codeLines.join('\n'),
      });
      blockIndex += 1;
      inCodeBlock = false;
      codeLines = [];
      codeLanguage = null;
      openingFenceLine = null;
    } else {
      codeLines.push(line);
    }
  }

  // End of text inside a code block: the fence never closed, so it is plain text.
  if (inCodeBlock) {
    currentText = savedText;
    if (openingFenceLine !== null) currentText.push(openingFenceLine);
    currentText.push(...codeLines);
  }

  flushText();
  return blocks;
}
