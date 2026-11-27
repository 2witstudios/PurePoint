// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/DiffCardView.swift (WIN-8.3).
//
// One file's diff card: a header (status icon, path, status badge, +/−) that expands or collapses
// it, its notices (preview error, loading, deferred preview, conflict, binary, rename), the body
// and the pager. Large and untracked files start collapsed; a navigator click reveals one.
//
// The card is not one element here: DiffListView virtualizes every row of every card in one
// scroll view, so the card is a controller (state and page loading, which outlive scrolled-out
// rows) and `rows()` (what the list draws). Differences from Swift:
// - Pages: the body reads on continuously, appending review pages as the list scrolls near the
//   card's end (ReviewPageWindow), up to 10,000 lines; past that "More lines" replaces the
//   window and "Previous lines" goes back, as the Mac pages. A card whose hunks came with the
//   file (no paging) shows them all, long lines shortened as `pageHunks` does.
// - "Open full patch" is not offered: the patch snapshot lives in the backend's temp folder and
//   the app has no opener for files yet.
// - The body is syntax highlighted (services/codeHighlight.ts); the Mac colours only +/−.
import type { ReactNode } from 'react';
import { detectEditorLanguage } from '../../model/editorLanguage.ts';
import { pageHunks, shouldCollapse } from '../../model/diff.ts';
import {
  gitErrorMessage,
  type DiffLine,
  type FileDiff,
  type Hunk,
  type ReviewPage,
  type ReviewPageCursor,
} from '../../services/git.ts';
import {
  highlightLines,
  loadedLanguage,
  loadLanguage,
  type HighlightedLine,
} from '../../services/codeHighlight.ts';
import { ReviewCancelledError } from '../../state/diffState.ts';
import { Icon } from '../../theme/Icon.tsx';
import type { SfSymbolName } from '../../theme/icons.ts';
import { DiffLineRow, DiffSeparatorRow } from './DiffContentView.tsx';
import { ReviewPageWindow } from './reviewPageWindow.ts';

export type LoadPage = (file: FileDiff, after: ReviewPageCursor | null) => Promise<ReviewPage>;

/** One row of a card, as the list draws it. */
export type CardRow =
  | { readonly kind: 'header' }
  | {
      readonly kind: 'notice';
      readonly id: string;
      readonly text: string;
      readonly tone: 'warning' | 'secondary';
      readonly mono?: boolean;
      readonly action?: { readonly label: string; readonly run: () => void };
    }
  | { readonly kind: 'loading' }
  | { readonly kind: 'loadPreview' }
  | {
      readonly kind: 'line';
      readonly id: string;
      readonly line: DiffLine;
      readonly tokens: HighlightedLine | null;
    }
  | { readonly kind: 'separator'; readonly id: string }
  | { readonly kind: 'pager' };

const NO_LINES: readonly Hunk[] = [];

export interface DiffCardOptions {
  readonly loadPage: LoadPage | null;
  readonly onLoadPreview: (() => void) | null;
  /** Called whenever the card's rows change. */
  readonly onChange: () => void;
}

/** Tokens per line of a set of hunks, highlighted per side (old: context + deletions). */
function highlightHunks(
  hunks: readonly Hunk[],
  filename: string,
): Map<DiffLine, HighlightedLine> | null {
  const language = loadedLanguage(detectEditorLanguage(filename));
  if (language === undefined) return null;
  const result = new Map<DiffLine, HighlightedLine>();
  for (const side of ['new', 'old'] as const) {
    const lines = hunks.flatMap((hunk) =>
      hunk.lines.filter((line) =>
        side === 'new' ? line.type !== 'deletion' : line.type !== 'addition',
      ),
    );
    const tokens = highlightLines(
      lines.map((line) => line.content),
      language,
    );
    lines.forEach((line, i) => {
      if (side === 'new' || line.type === 'deletion') result.set(line, tokens[i] ?? []);
    });
  }
  return result;
}

export class DiffCardController {
  file: FileDiff;
  expanded: boolean;
  loading = false;
  loadError: string | null = null;
  private window = new ReviewPageWindow();
  private readonly options: DiffCardOptions;
  private token = 0;
  private lastRequested = 0;
  private cachedRows: readonly CardRow[] | null = null;
  private readonly highlights = new WeakMap<object, Map<DiffLine, HighlightedLine>>();

  constructor(file: FileDiff, options: DiffCardOptions, revealed = false) {
    this.file = file;
    this.options = options;
    this.expanded = revealed || !shouldCollapse(file);
    void loadLanguage(detectEditorLanguage(file.filename)).then(() => {
      this.changed();
    });
  }

  get usesPages(): boolean {
    return (
      this.options.loadPage !== null &&
      (this.file.patchArguments !== null || this.file.patchSlice !== null)
    );
  }

  /** More patch follows the lines on screen. */
  get hasMore(): boolean {
    return this.usesPages && this.window.hasMore;
  }

  /** The list may append the next page as the user scrolls (the window has room). */
  get canReadOn(): boolean {
    return (
      this.expanded &&
      this.hasMore &&
      !this.loading &&
      this.loadError === null &&
      this.window.canExtend
    );
  }

  private changed(): void {
    this.cachedRows = null;
    this.options.onChange();
  }

  /** A refreshed review: a new preview version starts the pages over (reloaded by the list). */
  setFile(file: FileDiff): void {
    if (file === this.file) return;
    const versionChanged = file.previewVersion !== this.file.previewVersion;
    this.file = file;
    this.cachedRows = null;
    if (versionChanged) {
      this.token += 1;
      this.window = new ReviewPageWindow();
      this.loading = false;
      this.loadError = null;
      // The list reloads it after rendering (DiffListView calls `ensureLoaded`); this runs
      // while the list renders, so it must not notify.
    }
  }

  toggle(): void {
    this.expanded = !this.expanded;
    if (this.expanded) {
      this.ensureLoaded();
    } else {
      // Swift releases the body when a card collapses.
      this.token += 1;
      this.loading = false;
      this.window.releaseBodies();
    }
    this.changed();
  }

  reveal(): void {
    if (!this.expanded) this.toggle();
  }

  /** Drop the page bodies (the list is going away); the checkpoints stay. */
  release(): void {
    this.token += 1;
    this.loading = false;
    this.window.releaseBodies();
  }

  /** Load the window's first page if an expanded, paged card has none. */
  ensureLoaded(): void {
    if (this.expanded && this.usesPages && this.window.isEmpty && !this.loading) {
      void this.load(this.window.first);
    }
  }

  loadMore(): void {
    if (this.loading || !this.hasMore) return;
    void this.load(this.window.last + 1);
  }

  loadPrevious(): void {
    if (this.loading || this.window.first === 0) return;
    void this.load(this.window.first - 1);
  }

  retry(): void {
    this.loadError = null;
    void this.load(this.lastRequested);
  }

  private async load(index: number): Promise<void> {
    const loadPage = this.options.loadPage;
    if (loadPage === null) return;
    const token = ++this.token;
    this.lastRequested = index;
    this.loading = true;
    this.loadError = null;
    this.changed();
    try {
      const page = await loadPage(this.file, this.window.cursor(index));
      if (token !== this.token) return;
      this.window.store(page, index);
    } catch (error) {
      if (token !== this.token || error instanceof ReviewCancelledError) return;
      this.loadError = gitErrorMessage(error);
    }
    if (token !== this.token) return;
    this.loading = false;
    this.changed();
  }

  private tokensFor(source: object, hunks: readonly Hunk[]): Map<DiffLine, HighlightedLine> | null {
    let tokens = this.highlights.get(source);
    if (tokens === undefined) {
      const highlighted = highlightHunks(hunks, this.file.filename);
      if (highlighted === null) return null;
      tokens = highlighted;
      this.highlights.set(source, tokens);
    }
    return tokens;
  }

  /** What the body shows: the window's pages, or the file's own hunks. */
  private bodyRows(rows: CardRow[]): void {
    const sources: { source: object; hunks: readonly Hunk[] }[] = this.usesPages
      ? this.window.pages.map((page) => ({ source: page, hunks: page.hunks }))
      : this.file.hunks.length === 0
        ? []
        : [{ source: this.file.hunks, hunks: pageHunks(this.file, 0, Number.MAX_SAFE_INTEGER) }];
    let previousHeader: string | null = null;
    let lineNumber = 0;
    for (const { source, hunks } of sources) {
      const tokens = this.tokensFor(source, hunks.length === 0 ? NO_LINES : hunks);
      let firstOfSource = true;
      for (const hunk of hunks) {
        // `···` between hunks; a page that continues the previous page's hunk carries its
        // header on, and joins it without one.
        const continues = firstOfSource && hunk.header === previousHeader;
        if (previousHeader !== null && !continues) {
          rows.push({ kind: 'separator', id: `s${String(lineNumber)}` });
        }
        firstOfSource = false;
        previousHeader = hunk.header;
        for (const line of hunk.lines) {
          rows.push({
            kind: 'line',
            id: `l${String(lineNumber++)}`,
            line,
            tokens: tokens?.get(line) ?? null,
          });
        }
      }
    }
  }

  /** The rows the list draws for this card. */
  rows(): readonly CardRow[] {
    if (this.cachedRows !== null) return this.cachedRows;
    const rows: CardRow[] = [{ kind: 'header' }];
    if (this.expanded) {
      const file = this.file;
      const error = file.previewError ?? this.loadError;
      if (error !== null) {
        rows.push({
          kind: 'notice',
          id: 'error',
          text: error,
          tone: 'warning',
          ...(this.loadError === null
            ? {}
            : {
                action: {
                  label: 'Retry',
                  run: () => {
                    this.retry();
                  },
                },
              }),
        });
      }
      if (this.loading) rows.push({ kind: 'loading' });
      if (file.previewDeferred) rows.push({ kind: 'loadPreview' });
      if (file.statusCode === 'U') {
        rows.push({
          kind: 'notice',
          id: 'conflict',
          text: `Unresolved merge conflict · ${file.conflictBaseline ?? 'resolve this file before committing'}`,
          tone: 'warning',
        });
      }
      if (file.isBinary || this.window.pages.some((page) => page.isBinary)) {
        rows.push({ kind: 'notice', id: 'binary', text: 'Binary file changed', tone: 'secondary' });
      }
      if (file.oldFilename !== null) {
        rows.push({
          kind: 'notice',
          id: 'renamed',
          text: `Renamed from ${file.oldFilename}`,
          tone: 'secondary',
          mono: true,
        });
      }
      this.bodyRows(rows);
      if (this.window.pages.some((page) => page.hasLongLines)) {
        rows.push({
          kind: 'notice',
          id: 'long',
          text: 'Long lines are shortened in this preview. Open the full patch to read them.',
          tone: 'secondary',
        });
      }
      if (this.usesPages && (!this.window.isEmpty || this.window.first > 0)) {
        rows.push({ kind: 'pager' });
      }
    }
    this.cachedRows = rows;
    return rows;
  }

  get firstPage(): number {
    return this.window.first;
  }

  get lastPage(): number {
    return this.window.last;
  }

  get onLoadPreview(): (() => void) | null {
    return this.options.onLoadPreview;
  }
}

// MARK: - Status helpers

export function statusIcon(statusCode: string): SfSymbolName {
  switch (statusCode) {
    case 'A':
      return 'doc.badge.plus';
    case 'D':
      return 'doc.badge.minus';
    default:
      return 'doc.text';
  }
}

export function statusTone(statusCode: string): 'added' | 'deleted' | 'modified' | 'other' {
  switch (statusCode) {
    case 'A':
      return 'added';
    case 'D':
      return 'deleted';
    case 'M':
      return 'modified';
    default:
      return 'other';
  }
}

export function statusLabel(statusCode: string): string {
  switch (statusCode) {
    case 'U':
      return 'Conflict';
    case 'M':
      return 'Modified';
    case 'A':
      return 'Added';
    case 'D':
      return 'Deleted';
    case '??':
      return 'Untracked';
    default:
      return statusCode;
  }
}

// MARK: - Rows

function FileName({ filename }: { filename: string }) {
  const slash = filename.lastIndexOf('/');
  if (slash === -1) return <span className="diff-card-file">{filename}</span>;
  return (
    <span className="diff-card-path">
      <span className="diff-card-dir">{filename.slice(0, slash + 1)}</span>
      <span className="diff-card-file">{filename.slice(slash + 1)}</span>
    </span>
  );
}

export function DiffCardHeader({ card }: { card: DiffCardController }) {
  const file = card.file;
  const tone = statusTone(file.statusCode);
  return (
    <button
      type="button"
      className="diff-card-header"
      aria-expanded={card.expanded}
      aria-label={`${card.expanded ? 'Collapse' : 'Expand'} diff for ${file.filename}`}
      title={file.filename}
      onClick={() => {
        card.toggle();
      }}
    >
      <Icon
        name={card.expanded ? 'chevron.down' : 'chevron.right'}
        size={10}
        className="diff-card-chevron"
      />
      <Icon name={statusIcon(file.statusCode)} size={12} className={`diff-status-icon ${tone}`} />
      <FileName filename={file.filename} />
      <span className={`diff-status-badge ${tone}`}>{statusLabel(file.statusCode)}</span>
      <span className="detail-spacer" />
      {file.added > 0 && <span className="diff-card-added">{`+${String(file.added)}`}</span>}
      {file.removed > 0 && <span className="diff-card-removed">{`-${String(file.removed)}`}</span>}
    </button>
  );
}

function DiffPager({ card }: { card: DiffCardController }) {
  const first = card.firstPage;
  const last = card.lastPage;
  const label =
    last > first ? `Pages ${String(first + 1)}–${String(last + 1)}` : `Page ${String(first + 1)}`;
  return (
    <div className="diff-card-pager">
      {first > 0 && (
        <button
          type="button"
          className="detail-button"
          disabled={card.loading}
          onClick={() => {
            card.loadPrevious();
          }}
        >
          Previous lines
        </button>
      )}
      <span className="diff-card-page">{label}</span>
      {card.hasMore && (
        <button
          type="button"
          className="detail-button"
          disabled={card.loading}
          onClick={() => {
            card.loadMore();
          }}
        >
          More lines
        </button>
      )}
    </div>
  );
}

/** Draws one card row. */
export function DiffCardRowView({
  card,
  row,
}: {
  card: DiffCardController;
  row: CardRow;
}): ReactNode {
  switch (row.kind) {
    case 'header':
      return <DiffCardHeader card={card} />;
    case 'notice':
      return (
        <div className={`diff-card-notice ${row.tone}${row.mono === true ? ' mono' : ''}`}>
          <span className="diff-card-notice-text">{row.text}</span>
          {row.action !== undefined && (
            <button
              type="button"
              className="detail-button detail-bordered-button"
              onClick={row.action.run}
            >
              {row.action.label}
            </button>
          )}
        </div>
      );
    case 'loading':
      return (
        <div className="diff-card-notice secondary" role="status">
          <span className="diff-spinner" aria-label="Loading" />
        </div>
      );
    case 'loadPreview':
      return (
        <div className="diff-card-notice">
          <button
            type="button"
            className="detail-button"
            onClick={() => {
              card.onLoadPreview?.();
            }}
          >
            Load preview
          </button>
        </div>
      );
    case 'line':
      return <DiffLineRow line={row.line} tokens={row.tokens} />;
    case 'separator':
      return <DiffSeparatorRow />;
    case 'pager':
      return <DiffPager card={card} />;
  }
}
