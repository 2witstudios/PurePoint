// Port of apps/purepoint-macos/purepoint-macosTests/SessionListStateTests.swift (WIN-8.16).
// Each Swift case keeps its function name as the title; the cases after them cover details and
// `refreshSessions`, which the Swift suite leaves untested (it reads the real indexes).
import { describe, expect, it } from 'vitest';

import type {
  AgentSource,
  Conversation,
  ListConversationsOptions,
} from '../services/conversations';
import { SessionListState, groupConversations, type SessionListService } from './sessions';

function makeSession(
  id: string,
  title: string,
  modifiedAt: number = Date.now(),
  overrides: Partial<Conversation> = {},
): Conversation {
  const agentSource = overrides.agentSource ?? 'claude';
  return {
    id: `${agentSource}:${id}`,
    sessionId: id,
    agentSource,
    title,
    previewSnippets: [],
    projectPath: '/tmp',
    purePointProjectRoot: null,
    gitBranch: null,
    transcriptPath: `/tmp/${id}.jsonl`,
    createdAt: null,
    modifiedAt,
    messageCount: null,
    projectName: 'tmp',
    workspaceName: 'tmp',
    ...overrides,
  };
}

/** `calendar.date(byAdding: .day, value:, to:)`. */
function addingDays(ms: number, days: number): number {
  const date = new Date(ms);
  date.setDate(date.getDate() + days);
  return date.getTime();
}

describe('SessionListStateTests', () => {
  // MARK: - Filtering

  it('givenSearchQueryShouldFilterSessions', () => {
    const state = new SessionListState();
    state.sessions = [makeSession('s1', 'Dashboard work'), makeSession('s2', 'Terminal bugs')];
    state.searchQuery = 'Dashboard';

    expect(state.filteredSessions).toHaveLength(1);
    expect(state.filteredSessions[0]?.sessionId).toBe('s1');
  });

  it('givenEmptySearchQueryShouldReturnAllSessions', () => {
    const state = new SessionListState();
    state.sessions = [makeSession('s1', 'A'), makeSession('s2', 'B')];
    state.searchQuery = '';

    expect(state.filteredSessions).toHaveLength(2);
  });

  it('givenSearchQueryMatchingSnippetShouldIncludeSession', () => {
    const state = new SessionListState();
    state.sessions = [
      makeSession('s1', 'Some task', Date.now(), { previewSnippets: ['fix the login bug'] }),
    ];
    state.searchQuery = 'login';

    expect(state.filteredSessions).toHaveLength(1);
  });

  // MARK: - Time-period grouping

  it('givenGroupedSessionsShouldPartitionByTimePeriod', () => {
    const now = Date.now();
    const state = new SessionListState({ now: () => now });
    state.sessions = [
      makeSession('s-today', 'Today work', now),
      makeSession('s-yesterday', 'Yesterday work', addingDays(now, -1)),
      makeSession('s-week', 'This week work', addingDays(now, -4)),
      makeSession('s-old', 'Old work', addingDays(now, -60)),
    ];

    const sections = state.groupedSessions;
    expect(sections).toHaveLength(4);
    expect(sections.map((section) => section.id)).toEqual([
      'today',
      'yesterday',
      'this-week',
      'older',
    ]);
    for (const section of sections) expect(section.sessions).toHaveLength(1);
  });

  // MARK: - ConversationSection.grouped (pure function)

  it('givenConversationSectionGroupedShouldDeduplicateById', () => {
    const session = makeSession('s1', 'Dup', Date.now());
    const sections = groupConversations([session, session]);
    const totalCount = sections.reduce((sum, section) => sum + section.sessions.length, 0);
    expect(totalCount).toBe(1);
  });

  it('givenEmptyInputGroupedShouldReturnEmptySections', () => {
    expect(groupConversations([])).toEqual([]);
  });
});

describe('SessionListState grouping and search details', () => {
  it('titles the periods as the Mac does, putting 7-29 days ago in This Month', () => {
    const now = new Date(2026, 10, 25, 12).getTime();
    const sections = groupConversations(
      [
        makeSession('a', 'a', addingDays(now, -10)),
        makeSession('b', 'b', addingDays(now, -6)),
        makeSession('c', 'c', addingDays(now, -30)),
      ],
      now,
    );
    expect(sections.map((section) => [section.id, section.title])).toEqual([
      ['this-week', 'This Week'],
      ['this-month', 'This Month'],
      ['older', 'Older'],
    ]);
  });

  it('matches the trimmed query case-insensitively on branch and agent source', () => {
    const state = new SessionListState();
    state.sessions = [
      makeSession('s1', 'x', 0, { gitBranch: 'Feature/Login' }),
      makeSession('s2', 'y', 0, { agentSource: 'codex' }),
    ];
    state.searchQuery = '  feature/login ';
    expect(state.filteredSessions.map((s) => s.sessionId)).toEqual(['s1']);
    state.searchQuery = 'CODEX';
    expect(state.filteredSessions.map((s) => s.sessionId)).toEqual(['s2']);
  });
});

type ListKey = `${AgentSource}:${'indexed' | 'loose'}`;

function fakeService(
  lists: Partial<Record<ListKey, Conversation[] | Error>>,
  snippets: Record<string, string[]> = {},
) {
  const calls: [AgentSource, ListConversationsOptions][] = [];
  const service: SessionListService = {
    list: (source, options) => {
      calls.push([source, options]);
      const result = lists[`${source}:${options.phase === 'loose' ? 'loose' : 'indexed'}`];
      if (result instanceof Error) return Promise.reject(result);
      return Promise.resolve(result ?? []);
    },
    snippets: (path) => Promise.resolve(snippets[path] ?? []),
  };
  return { service, calls };
}

describe('SessionListState.refreshSessions', () => {
  it('merges every source newest first, adds loose Claude sessions and enriches Codex rows', async () => {
    const { service, calls } = fakeService({
      'claude:indexed': [makeSession('c1', 'claude', 300, { previewSnippets: ['p'] })],
      'codex:indexed': [makeSession('x1', 'codex', 200, { agentSource: 'codex' })],
      'opencode:indexed': new Error('unreadable'),
      'claude:loose': [makeSession('c2', 'loose', 250, { previewSnippets: ['p'] })],
      'codex:loose': [
        makeSession('x1', 'codex', 200, { agentSource: 'codex', gitBranch: 'main' }),
        makeSession('x9', 'not listed', 100, { agentSource: 'codex' }),
      ],
    });
    const state = new SessionListState({ service });

    await state.refreshSessions();

    expect(state.sessions.map((s) => s.id)).toEqual(['claude:c1', 'claude:c2', 'codex:x1']);
    expect(state.sessions[2]?.gitBranch).toBe('main');
    expect(state.isLoadingSessions).toBe(false);
    expect(calls).toContainEqual(['claude', { phase: 'loose', excluding: ['c1'] }]);
  });

  it('fills in snippets for Claude sessions with a transcript and none yet', async () => {
    const { service } = fakeService(
      {
        'claude:indexed': [
          makeSession('c1', 'a', 2),
          makeSession('c2', 'b', 1, { transcriptPath: null }),
        ],
      },
      { '/tmp/c1.jsonl': ['hello'] },
    );
    const state = new SessionListState({ service });

    await state.refreshSessions();

    expect(state.sessions.map((s) => s.previewSnippets)).toEqual([['hello'], []]);
  });

  it('does nothing while a refresh is already running', async () => {
    const { service, calls } = fakeService({});
    const state = new SessionListState({ service });

    await Promise.all([state.refreshSessions(), state.refreshSessions()]);

    expect(calls.filter(([, options]) => options.phase === 'indexed')).toHaveLength(3);
  });
});
