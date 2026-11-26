// Command wrappers for the conversation indexes (WIN-8.12); the indexes are tested in Rust.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

import {
  conversationSnippets,
  listConversations,
  loadTranscript,
  mergeConversations,
  type Conversation,
} from './conversations';

const row = (id: string, modifiedAt: number, title = id): Conversation => ({
  id: `claude:${id}`,
  sessionId: id,
  agentSource: 'claude',
  title,
  previewSnippets: [],
  projectPath: null,
  purePointProjectRoot: null,
  gitBranch: null,
  transcriptPath: null,
  createdAt: null,
  modifiedAt,
  messageCount: null,
  projectName: 'p',
  workspaceName: '~',
});

describe('conversation commands', () => {
  beforeEach(() => invoke.mockReset().mockResolvedValue([]));

  it('lists with defaults', async () => {
    await listConversations('codex');
    expect(invoke).toHaveBeenCalledWith('conversations_list', {
      source: 'codex',
      phase: 'all',
      projectPath: null,
      excluding: null,
    });
  });

  it('passes phase, project filter and exclusions', async () => {
    await listConversations('claude', {
      phase: 'loose',
      projectPath: 'C:\\repo',
      excluding: ['a'],
    });
    expect(invoke).toHaveBeenCalledWith('conversations_list', {
      source: 'claude',
      phase: 'loose',
      projectPath: 'C:\\repo',
      excluding: ['a'],
    });
  });

  it('loads snippets and transcripts', async () => {
    await conversationSnippets('C:\\t.jsonl');
    expect(invoke).toHaveBeenCalledWith('conversation_snippets', {
      transcriptPath: 'C:\\t.jsonl',
      limit: 3,
    });
    await loadTranscript('C:\\t.jsonl');
    expect(invoke).toHaveBeenCalledWith('conversation_transcript', {
      transcriptPath: 'C:\\t.jsonl',
    });
  });
});

describe('mergeConversations', () => {
  it('replaces by id, appends new rows, and sorts newest first', () => {
    const merged = mergeConversations(
      [row('a', 1), row('b', 3)],
      [row('a', 5, 'enriched'), row('c', 2)],
    );
    expect(merged.map((c) => [c.sessionId, c.title])).toEqual([
      ['a', 'enriched'],
      ['b', 'b'],
      ['c', 'c'],
    ]);
  });
});
