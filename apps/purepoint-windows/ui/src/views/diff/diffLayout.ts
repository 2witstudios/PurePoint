// Row heights for the virtualized diff body (WIN-8.3), the platform-neutral half of
// DiffContentNSView's `height(for:)`: lines wrap at the width the body actually gets, so a
// narrower body is taller. The virtualizer uses these as estimates and then measures the rows it
// draws; diffLayout.test.ts ports DiffLayoutTests.swift against them.
//
// The code cell is monospaced with `white-space: pre-wrap; overflow-wrap: anywhere; tab-size: 4`,
// so a line of n columns at c columns per row takes ceil(n / c) rows. East Asian wide characters
// count as two columns and a tab advances to the next multiple of four.
import type { DiffLine, Hunk } from '../../services/git.ts';
import { DIFF_CODE_LEFT, DIFF_INSET } from './diffTextRendering.ts';

export interface DiffMetrics {
  /** Advance of one monospaced column (12px Cascadia Mono). */
  readonly charWidth: number;
  /** One wrapped row of code. */
  readonly lineHeight: number;
  /** Code starts here; the line numbers sit left of it. */
  readonly codeLeft: number;
  /** Right inset of the code cell. */
  readonly rightInset: number;
  /** The `···` row between hunks (11px text, 4px above and below). */
  readonly separatorHeight: number;
}

export const DIFF_METRICS: DiffMetrics = {
  charWidth: 7.23,
  lineHeight: 16,
  codeLeft: DIFF_CODE_LEFT,
  rightInset: DIFF_INSET.width * 2,
  separatorHeight: 22,
};

const TAB_SIZE = 4;

function isWide(code: number): boolean {
  return (
    (code >= 0x1100 && code <= 0x115f) ||
    (code >= 0x2e80 && code <= 0xa4cf) ||
    (code >= 0xac00 && code <= 0xd7a3) ||
    (code >= 0xf900 && code <= 0xfaff) ||
    (code >= 0xfe30 && code <= 0xfe4f) ||
    (code >= 0xff00 && code <= 0xff60) ||
    (code >= 0xffe0 && code <= 0xffe6) ||
    (code >= 0x1f300 && code <= 0x1faff) ||
    (code >= 0x20000 && code <= 0x3fffd)
  );
}

/** Columns a line occupies unwrapped. */
export function lineColumns(content: string): number {
  let columns = 0;
  for (const char of content) {
    if (char === '\t') columns += TAB_SIZE - (columns % TAB_SIZE);
    else columns += isWide(char.codePointAt(0) ?? 0) ? 2 : 1;
  }
  return columns;
}

/** Columns that fit in a body `width` px wide (at least one). */
export function columnsPerRow(width: number, metrics: DiffMetrics = DIFF_METRICS): number {
  const available = width - metrics.codeLeft - metrics.rightInset;
  return Math.max(1, Math.floor(available / metrics.charWidth));
}

/** Wrapped rows of one line (an empty line still takes one). */
export function wrappedRows(content: string, columns: number): number {
  return Math.max(1, Math.ceil(lineColumns(content) / Math.max(1, columns)));
}

export function diffLineHeight(
  line: Pick<DiffLine, 'content'>,
  width: number,
  metrics: DiffMetrics = DIFF_METRICS,
): number {
  return wrappedRows(line.content, columnsPerRow(width, metrics)) * metrics.lineHeight;
}

/** The whole body at `width`: its rows plus the top and bottom inset. */
export function diffBodyHeight(
  hunks: readonly Hunk[],
  width: number,
  metrics: DiffMetrics = DIFF_METRICS,
): number {
  if (width <= 0) return 20;
  const columns = columnsPerRow(width, metrics);
  let height = DIFF_INSET.height * 2;
  hunks.forEach((hunk, index) => {
    if (index > 0) height += metrics.separatorHeight;
    for (const line of hunk.lines)
      height += wrappedRows(line.content, columns) * metrics.lineHeight;
  });
  return Math.ceil(height);
}

/**
 * DiffContentNSView's measuring half: the body for some hunks, measured at the proposed width
 * and cached per width until the hunks change.
 */
export class DiffContentLayout {
  private hunks: readonly Hunk[];
  private cachedWidth: number | null = null;
  private cachedHeight = 20;
  private readonly metrics: DiffMetrics;

  constructor(hunks: readonly Hunk[], metrics: DiffMetrics = DIFF_METRICS) {
    this.hunks = hunks;
    this.metrics = metrics;
  }

  update(hunks: readonly Hunk[]): void {
    if (hunks === this.hunks) return;
    this.hunks = hunks;
    this.cachedWidth = null;
  }

  /** Never measures at a zero width (the Mac's initial frame). */
  height(width: number): number {
    if (width <= 0) return 20;
    if (this.cachedWidth === width) return this.cachedHeight;
    this.cachedHeight = diffBodyHeight(this.hunks, width, this.metrics);
    this.cachedWidth = width;
    return this.cachedHeight;
  }
}
