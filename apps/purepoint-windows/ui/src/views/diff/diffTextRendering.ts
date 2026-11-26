// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/DiffTextRendering.swift (WIN-8.3).
//
// The platform-neutral half of the diff and inline code bodies: which rows they draw, in which
// colours, and the gutter metrics. The Mac builds one attributed string per body and lays it out
// with TextKit; here each source line is a row (so the list can virtualize them), and the colours
// are the theme's CSS tokens (`--pp-addition-*`, `--pp-deletion-*`, `--pp-hunk-separator`).
import type { DiffLine, Hunk, LineType } from '../../services/git.ts';

// MARK: - Diff body

/** `DiffContentNSView`'s text container inset (px). */
export const DIFF_INSET = { width: 4, height: 6 } as const;
/** The Mac's tab stops: old line number right-aligned at 36, new at 72, code from 82. */
export const DIFF_OLD_NUMBER_RIGHT = 36;
export const DIFF_NEW_NUMBER_RIGHT = 72;
export const DIFF_CODE_LEFT = 82;

/** One row of a diff body: a source line, or the centred `···` between hunks. */
export type DiffBodyRow =
  { readonly kind: 'line'; readonly line: DiffLine } | { readonly kind: 'separator' };

/** Swift `diffAttributedString(hunks:)`, as rows: each hunk's lines, `···` between hunks. */
export function diffBodyRows(hunks: readonly Hunk[]): DiffBodyRow[] {
  const rows: DiffBodyRow[] = [];
  hunks.forEach((hunk, index) => {
    if (index > 0) rows.push({ kind: 'separator' });
    for (const line of hunk.lines) rows.push({ kind: 'line', line });
  });
  return rows;
}

/** Swift `diffBackground(for:)`: the row's background token. */
export function diffBackground(type: LineType): string {
  switch (type) {
    case 'addition':
      return 'var(--pp-addition-background)';
    case 'deletion':
      return 'var(--pp-deletion-background)';
    case 'context':
      return 'var(--pp-card-background)';
  }
}

/** The code colour of a diff line (Swift's `fgColor`). */
export function diffForeground(type: LineType): string {
  switch (type) {
    case 'addition':
      return 'var(--pp-addition-text)';
    case 'deletion':
      return 'var(--pp-deletion-text)';
    case 'context':
      return 'var(--pp-primary-text)';
  }
}

// MARK: - Inline code body

/** One source line of the inline code body (`line === null` is the `⋯` row between hunks). */
export interface InlineRow {
  readonly text: string;
  readonly line: DiffLine | null;
}

/** Width of the line-number and +/− gutter left of the code. */
export const INLINE_GUTTER_WIDTH = 42;
/** `InlineCodeNSView`'s text container inset. */
export const INLINE_INSET = { width: 4, height: 4 } as const;
/** Line numbers are right-aligned at x = 27, the +/− marker starts at x = 32. */
export const INLINE_NUMBER_RIGHT = 27;
export const INLINE_MARKER_LEFT = 32;

/**
 * Swift `inlineCode(hunks:)`: source only, one row per line, with `⋯` between hunks. Line
 * numbers and markers are drawn in the gutter so they cannot confuse the syntax highlighter.
 */
export function inlineCodeRows(hunks: readonly Hunk[]): InlineRow[] {
  const rows: InlineRow[] = [];
  hunks.forEach((hunk, index) => {
    if (index > 0) rows.push({ text: '⋯', line: null });
    for (const line of hunk.lines) rows.push({ text: line.content, line });
  });
  return rows;
}

/** Swift `inlineBackground(for:)`; the `⋯` row uses the card header colour. */
export function inlineBackground(type: LineType | null): string {
  return type === null ? 'var(--pp-card-header-background)' : diffBackground(type);
}

/** The gutter of an inline row: its number (new, else old) and +/− marker. */
export function inlineGutter(line: DiffLine | null): { number: string; marker: string } {
  if (line === null) return { number: '', marker: '' };
  const number = line.newLineNo ?? line.oldLineNo;
  return {
    number: number === null ? '' : String(number),
    marker: line.type === 'addition' ? '+' : line.type === 'deletion' ? '−' : '',
  };
}
