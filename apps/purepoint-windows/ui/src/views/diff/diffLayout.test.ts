// Port of apps/purepoint-macos/purepoint-macosTests/DiffLayoutTests.swift (WIN-8.3): the
// platform-neutral `boundedPageResizes` case, against the diff body layout the virtualized list
// uses. Titles are the Swift failure messages.
import { describe, expect, it } from 'vitest';
import type { DiffLine, Hunk } from '../../services/git.ts';
import {
  DIFF_METRICS,
  DiffContentLayout,
  columnsPerRow,
  diffBodyHeight,
  lineColumns,
  wrappedRows,
} from './diffLayout.ts';

const lines: DiffLine[] = Array.from({ length: 200 }, (_, i) => ({
  type: 'addition',
  content: 'source text '.repeat(8),
  oldLineNo: null,
  newLineNo: i + 1,
}));
const page: Hunk[] = [{ header: '@@ -0,0 +1,200 @@', lines }];

describe('DiffLayoutTests', () => {
  it('diff height must reflect wrapped lines at the actual available width', () => {
    const view = new DiffContentLayout(page);
    const started = performance.now();
    const wide = view.height(1000);
    const narrow = view.height(320);
    expect(wide).toBeGreaterThan(200);
    expect(narrow).toBeGreaterThan(wide);
    // Bounded: a 200-line page measures well inside one frame.
    expect(performance.now() - started).toBeLessThan(50);
  });

  it('changed diff content must replace the previous measured height', () => {
    const view = new DiffContentLayout(page);
    const narrow = view.height(320);
    const first = lines[0];
    if (first === undefined) throw new Error('no line');
    view.update([{ header: '@@ -0,0 +1 @@', lines: [first] }]);
    expect(view.height(320)).toBeLessThan(narrow);
  });

  it('never measures at a zero width', () => {
    expect(new DiffContentLayout(page).height(0)).toBe(20);
  });
});

describe('diff layout', () => {
  it('wraps a line at the columns that fit beside the gutter', () => {
    const columns = columnsPerRow(
      DIFF_METRICS.codeLeft + DIFF_METRICS.rightInset + 10 * DIFF_METRICS.charWidth,
    );
    expect(columns).toBe(10);
    expect(wrappedRows('', columns)).toBe(1);
    expect(wrappedRows('x'.repeat(10), columns)).toBe(1);
    expect(wrappedRows('x'.repeat(11), columns)).toBe(2);
  });

  it('counts tabs to the next stop and wide characters as two columns', () => {
    expect(lineColumns('\tab')).toBe(6);
    expect(lineColumns('a\tb')).toBe(5);
    expect(lineColumns('日本')).toBe(4);
  });

  it('adds a separator between hunks and the insets', () => {
    const one: Hunk = { header: '@@', lines: lines.slice(0, 1) };
    const single = diffBodyHeight([one], 1000);
    expect(single).toBe(12 + DIFF_METRICS.lineHeight);
    expect(diffBodyHeight([one, one], 1000)).toBe(
      12 + 2 * DIFF_METRICS.lineHeight + DIFF_METRICS.separatorHeight,
    );
  });
});
