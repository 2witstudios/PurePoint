// The small model ports of WIN-6.0: AgentStatus, WorktreeNameNormalizer, DiffModel's page history
// and pager, PRModel ids, PlatformTypes and ShellUtilities. Swift has no dedicated suites for these; the
// ReviewPageHistory checks follow GitReviewTests.reviewRegressionChecks.
import { describe, expect, it } from 'vitest';
import {
  AGENT_STATUS_RAW_VALUES,
  agentStatusSystemColor,
  isAgentStatusAlive,
  normalizeAgentStatus,
} from './agentStatus';
import {
  DIFF_TABS,
  DIFF_TAB_TITLES,
  GitReviewError,
  ReviewPageHistory,
  fileDiffId,
  pageHunks,
  reviewFileEntryId,
} from './diff';
import type { ReviewPage } from '../services/git';
import { elementHasKeyboardFocus } from './platformTypes';
import { prLabelId, pullRequestId } from './pr';
import { normalizeWorktreeName } from './worktreeName';
import { shellEscape } from './shellEscape';

describe('AgentStatus', () => {
  it('normalizes legacy statuses to the two observable states', () => {
    const alive = ['running', 'streaming', 'waiting', 'spawning', 'idle', 'suspended'];
    for (const raw of AGENT_STATUS_RAW_VALUES) {
      expect(normalizeAgentStatus(raw)).toBe(alive.includes(raw) ? 'running' : 'broken');
      expect(isAgentStatusAlive(raw)).toBe(alive.includes(raw));
    }
    // An unknown raw value decodes as `lost`.
    expect(normalizeAgentStatus('exploded')).toBe('broken');
    expect(agentStatusSystemColor('idle')).toBe('systemGreen');
    expect(agentStatusSystemColor('killed')).toBe('systemRed');
  });
});

describe('WorktreeNameNormalizer', () => {
  it('lowercases, keeps ASCII alphanumerics and hyphenates whitespace and underscores', () => {
    expect(normalizeWorktreeName('Fix Login Bug')).toBe('fix-login-bug');
    expect(normalizeWorktreeName('snake_case_name')).toBe('snake-case-name');
    expect(normalizeWorktreeName('  --Lead  and trail--  ')).toBe('lead-and-trail');
    expect(normalizeWorktreeName('a\t\nb\r\nc')).toBe('a-b-c');
    expect(normalizeWorktreeName('feat/auth: v2!')).toBe('featauth-v2');
    expect(normalizeWorktreeName('café über')).toBe('caf-ber');
    // A combining accent makes one non-ASCII Character, dropped whole as in Swift.
    expect(normalizeWorktreeName('café')).toBe('caf');
    expect(normalizeWorktreeName('---')).toBe('');
  });
});

function page(index: number, lines: number, hasMore: boolean): ReviewPage {
  return {
    slice: { snapshot: 'snap', start: 10, end: 99 },
    hunks: [
      {
        header: '@@',
        lines: Array.from({ length: lines }, (_, i) => ({
          type: 'addition' as const,
          content: `line ${String(index * 200 + i)}`,
          oldLineNo: null,
          newLineNo: index * 200 + i + 1,
        })),
      },
    ],
    nextOffset: 10 + (index + 1) * 100,
    oldLine: 0,
    newLine: (index + 1) * 200,
    header: '@@',
    hasMore,
    isBinary: false,
    hasLongLines: false,
  };
}

describe('ReviewPageHistory', () => {
  it('visited pages retain only one bounded body and stay reachable', () => {
    const history = new ReviewPageHistory();
    expect(history.cursor(0)).toBeNull();
    expect(() => history.cursor(2)).toThrow(GitReviewError);
    history.store(page(0, 200, true), 0);
    expect(history.cursor(0)).toMatchObject({ nextOffset: 10, oldLine: 0, newLine: 0, header: '' });
    expect(history.cursor(1)).toMatchObject({ nextOffset: 110, newLine: 200 });
    history.store(page(1, 50, false), 1);
    expect(history.retainedLineCount).toBe(50);
    expect(history.page(0)).toBeNull();
    expect(history.page(1)?.hunks[0]?.lines.at(-1)?.newLineNo).toBe(250);
    // collapsing/offscreen cards release page bodies
    history.releaseBody();
    expect(history.retainedLineCount).toBe(0);
    // re-storing an earlier page replaces the next checkpoint
    history.store(page(0, 200, true), 0);
    expect(history.cursor(1)).toMatchObject({ nextOffset: 110 });
  });
});

describe('DiffModel', () => {
  const longLine = 'x'.repeat(3000);
  const file = {
    hunks: [
      {
        header: 'A',
        lines: [1, 2, 3].map((n) => ({
          type: 'context' as const,
          content: n === 2 ? longLine : `a${String(n)}`,
          oldLineNo: n,
          newLineNo: n,
        })),
      },
      {
        header: 'B',
        lines: [4, 5].map((n) => ({
          type: 'deletion' as const,
          content: `b${String(n)}`,
          oldLineNo: n,
          newLineNo: null,
        })),
      },
    ],
  };

  it('pages hunk lines across headers and shortens long lines', () => {
    const first = pageHunks(file, 1, 3);
    expect(first.map((h) => h.header)).toEqual(['A', 'B']);
    expect(first[0]?.lines[0]?.content).toBe('x'.repeat(2048) + ' … [long line]');
    expect(first[1]?.lines.map((l) => l.content)).toEqual(['b4']);
    expect(pageHunks(file, 3).map((h) => h.lines.length)).toEqual([2]);
    expect(pageHunks(file, 5)).toEqual([]);
  });

  it('names tabs and ids as Swift does', () => {
    expect(DIFF_TABS.map((tab) => DIFF_TAB_TITLES[tab])).toEqual([
      'Branch Changes',
      'Uncommitted',
      'Commits',
      'PR Diffs',
    ]);
    const f = { filename: 'a.txt' } as Parameters<typeof fileDiffId>[0];
    expect(fileDiffId(f)).toBe('a.txt');
    expect(reviewFileEntryId({ file: f, group: 'Staged' })).toBe('Staged\0a.txt');
  });
});

describe('PRModel', () => {
  it('derives Identifiable ids', () => {
    expect(pullRequestId({ number: 7 } as Parameters<typeof pullRequestId>[0])).toBe(7);
    expect(prLabelId({ name: 'bug', color: 'ff0000' })).toBe('bug');
  });
});

describe('PlatformTypes', () => {
  it('reports keyboard focus inside the terminal element', () => {
    const host = document.createElement('div');
    const input = document.createElement('textarea');
    host.append(input);
    document.body.append(host);
    expect(elementHasKeyboardFocus(host)).toBe(false);
    input.focus();
    expect(elementHasKeyboardFocus(host)).toBe(true);
    host.remove();
  });
});

describe('ShellUtilities', () => {
  it('shellEscape wraps in single quotes and escapes embedded ones', () => {
    expect(shellEscape('plain')).toBe("'plain'");
    expect(shellEscape("it's here")).toBe("'it'\\''s here'");
    expect(shellEscape('C:\\a b\\c')).toBe("'C:\\a b\\c'");
    expect(shellEscape('')).toBe("''");
  });
});
