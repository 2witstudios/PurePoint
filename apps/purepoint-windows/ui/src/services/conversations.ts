// Front end of the agent conversation indexes (WIN-8.12): Services/{Claude,Codex,OpenCode}
// ConversationIndex.swift and Services/TranscriptParser.swift run in the Rust backend
// (`src-tauri/src/conversations/`); these wrap its commands. Dates are Unix milliseconds.
import { invoke } from '@tauri-apps/api/core';

/** `AgentSource`. */
export type AgentSource = 'claude' | 'codex' | 'opencode';

/** Foundation's `Date.distantPast` in Unix ms: a session whose modification date is unknown. */
export const DISTANT_PAST_MS = -62_135_596_800_000;

/** `Conversation`, with its computed `id`, `projectName` and `workspaceName`. */
export interface Conversation {
  /** `"<agentSource>:<sessionId>"`. */
  id: string;
  sessionId: string;
  agentSource: AgentSource;
  title: string;
  previewSnippets: string[];
  projectPath: string | null;
  purePointProjectRoot: string | null;
  gitBranch: string | null;
  transcriptPath: string | null;
  createdAt: number | null;
  modifiedAt: number;
  messageCount: number | null;
  projectName: string;
  workspaceName: string;
}

/**
 * Which pass of an index to run. `indexed` is fast (Claude `sessions-index.json`, Codex
 * `session_index.jsonl`, OpenCode session files). `loose` is slow: Claude transcripts not in
 * `excluding`, Codex sessions enriched with cwd and branch (replace phase-1 rows by `id`),
 * nothing for OpenCode. `all` is both.
 */
export type ListPhase = 'indexed' | 'loose' | 'all';

export interface ListConversationsOptions {
  phase?: ListPhase;
  /** Only sessions in this project (its directory or PurePoint root is it or inside it). */
  projectPath?: string;
  /** Claude session ids to skip in the `loose` phase (those `indexed` returned). */
  excluding?: readonly string[];
}

export type ChatRole = 'user' | 'assistant';
export type ToolUseStatus = 'running' | 'completed' | 'failed';

/** `ContentBlock` as a transcript yields it (no Point Guard `pulse`). */
export type ContentBlock =
  | { type: 'text'; id: string; text: string }
  | { type: 'codeBlock'; id: string; language: string | null; code: string }
  | { type: 'toolUse'; id: string; name: string; input: string; status: ToolUseStatus }
  | { type: 'toolResult'; id: string; toolUseId: string; output: string; isError: boolean };

/** `ChatMessage`. */
export interface ChatMessage {
  id: string;
  role: ChatRole;
  timestamp: number;
  contentBlocks: ContentBlock[];
  isStreaming: boolean;
}

/** One source's sessions, newest first. */
export function listConversations(
  source: AgentSource,
  options: ListConversationsOptions = {},
): Promise<Conversation[]> {
  return invoke<Conversation[]>('conversations_list', {
    source,
    phase: options.phase ?? 'all',
    projectPath: options.projectPath ?? null,
    excluding: options.excluding ? [...options.excluding] : null,
  });
}

/** Up to `limit` recent message snippets from a Claude transcript, oldest first. */
export function conversationSnippets(transcriptPath: string, limit = 3): Promise<string[]> {
  return invoke<string[]>('conversation_snippets', { transcriptPath, limit });
}

/** A Claude transcript as chat messages. */
export function loadTranscript(transcriptPath: string): Promise<ChatMessage[]> {
  return invoke<ChatMessage[]>('conversation_transcript', { transcriptPath });
}

/** Rows from a `loose` pass merged into an earlier list: same `id` replaces, new ids append. */
export function mergeConversations(
  existing: readonly Conversation[],
  incoming: readonly Conversation[],
): Conversation[] {
  const byId = new Map(existing.map((conversation) => [conversation.id, conversation]));
  for (const conversation of incoming) byId.set(conversation.id, conversation);
  return [...byId.values()].sort((a, b) => b.modifiedAt - a.modifiedAt);
}
