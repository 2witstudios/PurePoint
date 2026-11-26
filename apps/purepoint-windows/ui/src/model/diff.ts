// Port of apps/purepoint-macos/purepoint-macos/Models/DiffModel.swift (WIN-6.0).
//
// The review data types themselves (`FileDiff`, `Hunk`, `DiffLine`, `ReviewPage`, …) are what the
// git backend returns (`services/git.ts`, WIN-8.1), so they are re-exported here rather than
// declared twice. Swift's `PatchSnapshot` (a temp file deleted on `deinit`) is a backend-owned
// snapshot id in `PatchSlice.snapshot`. What stays in the front end is below: `DiffTab` (declared
// in DiffState.swift), the review list's grouping, the page history and the bounded hunk pager.

import type { DiffLine, FileDiff, Hunk, ReviewPage, ReviewPageCursor } from '../services/git';
import { continuation } from '../services/git';

export type {
  DiffData,
  DiffLine,
  FileDiff,
  GitBranchReview,
  GitCommitInfo,
  GitLocalReview,
  GitReviewSummary,
  Hunk,
  LineType,
  PatchSlice,
  ReviewPage,
  ReviewPageCursor,
} from '../services/git';
export { continuation, shouldCollapse, uniquePathCount } from '../services/git';

/** Swift `DiffData.empty`. */
export const EMPTY_DIFF_DATA = { files: [] } as const;

/** Swift `FileDiff.id`. */
export function fileDiffId(file: FileDiff): string {
  return file.filename;
}

/** Swift `DiffTab`, by case name; `DIFF_TAB_TITLES` holds the raw values the tab bar shows. */
export const DIFF_TABS = ['branch', 'unstaged', 'commits', 'prDiffs'] as const;
export type DiffTab = (typeof DIFF_TABS)[number];

export const DIFF_TAB_TITLES: Readonly<Record<DiffTab, string>> = {
  branch: 'Branch Changes',
  unstaged: 'Uncommitted',
  commits: 'Commits',
  prDiffs: 'PR Diffs',
};

/** Swift `GitReviewError`: the message a review call failed with. */
export class GitReviewError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'GitReviewError';
  }
}

/**
 * Swift `ReviewPageHistory`: keep one page body in memory and reread previously visited pages
 * from their disk checkpoints. Mutating, like the Swift struct.
 */
export class ReviewPageHistory {
  private checkpoints: (ReviewPageCursor | null)[] = [null];
  private current: ReviewPage | null = null;
  private currentIndex = 0;

  get retainedLineCount(): number {
    return this.current?.hunks.reduce((sum, hunk) => sum + hunk.lines.length, 0) ?? 0;
  }

  page(index: number): ReviewPage | null {
    return this.currentIndex === index ? this.current : null;
  }

  /** The cursor that loads page `index`; throws when the preceding page was never loaded. */
  cursor(index: number): ReviewPageCursor | null {
    if (index < 0 || index >= this.checkpoints.length) {
      throw new GitReviewError('Load the preceding page first.');
    }
    return this.checkpoints[index] ?? null;
  }

  store(page: ReviewPage, index: number): void {
    this.current = page;
    this.currentIndex = index;
    if (index === 0) {
      this.checkpoints[0] = {
        slice: page.slice,
        nextOffset: page.slice.start,
        oldLine: 0,
        newLine: 0,
        header: '',
        isBinary: page.isBinary,
      };
    }
    if (this.checkpoints.length === index + 1) this.checkpoints.push(continuation(page));
    else if (index + 1 < this.checkpoints.length) this.checkpoints[index + 1] = continuation(page);
  }

  releaseBody(): void {
    this.current = null;
  }
}

/** Swift `DiffFileGroup`. */
export interface DiffFileGroup {
  readonly title: string;
  readonly files: readonly FileDiff[];
}

/** Swift `ReviewFileEntry`. */
export interface ReviewFileEntry {
  readonly file: FileDiff;
  readonly group: string;
  readonly startsGroup: boolean;
}

/** Swift `ReviewFileEntry.id`: the group, NUL, the file name. */
export function reviewFileEntryId(entry: Pick<ReviewFileEntry, 'file' | 'group'>): string {
  return `${entry.group}\0${entry.file.filename}`;
}

const LONG_LINE = 2048;

const GRAPHEMES = new Intl.Segmenter(undefined, { granularity: 'grapheme' });

/** Swift counts Characters (grapheme clusters); a line is never longer in those than in UTF-16. */
function truncatedLine(content: string): string {
  if (content.length <= LONG_LINE) return content;
  let count = 0;
  for (const { index } of GRAPHEMES.segment(content)) {
    if (count === LONG_LINE) return content.slice(0, index) + ' … [long line]';
    count += 1;
  }
  return content;
}

/**
 * Swift `FileDiff.pageHunks(offset:limit:)`: lines `offset ..< offset + limit` of a full patch,
 * regrouped under their hunk headers, with lines over 2,048 characters shortened.
 */
export function pageHunks(file: Pick<FileDiff, 'hunks'>, offset: number, limit = 200): Hunk[] {
  let skipped = 0;
  let remaining = limit;
  const result: Hunk[] = [];
  for (const hunk of file.hunks) {
    if (skipped + hunk.lines.length <= offset) {
      skipped += hunk.lines.length;
      continue;
    }
    const start = Math.max(0, offset - skipped);
    const lines: DiffLine[] = hunk.lines
      .slice(start, start + remaining)
      .map((line) => ({ ...line, content: truncatedLine(line.content) }));
    if (lines.length > 0) {
      result.push({ header: hunk.header, lines });
      remaining -= lines.length;
    }
    skipped += hunk.lines.length;
    if (remaining === 0) break;
  }
  return result;
}
