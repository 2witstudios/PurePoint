// Port of apps/purepoint-macos/purepoint-macos/Views/Detail/DiffListView.swift (WIN-8.3).
//
// A review's changed files: the count and the navigator toggle, the cards in the middle, and the
// navigator (find a changed file, jump to it) on the right. Metadata navigation stays complete
// while only visible, expanded bodies load.
//
// The middle is one virtualized list (@tanstack/react-virtual) of every card's rows, so a diff
// of thousands of lines keeps only the rows on screen in the DOM. Rows are estimated from
// diffLayout.ts at the list's width and measured once drawn. When the user scrolls near the end
// of a card whose patch has more pages, the next page loads and joins it (DiffCardController).
import { useVirtualizer } from '@tanstack/react-virtual';
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from 'react';
import { reviewFileEntryId, type DiffFileGroup, type ReviewFileEntry } from '../../model/diff.ts';
import type { DiffData, FileDiff } from '../../services/git.ts';
import { Icon } from '../../theme/Icon.tsx';
import {
  type CardRow,
  DiffCardController,
  DiffCardRowView,
  type LoadPage,
} from './DiffCardView.tsx';
import { DIFF_METRICS, diffLineHeight } from './diffLayout.ts';
import './diff.css';

const SIDEBAR_KEY = 'showReviewFilesSidebar';
/** Load the next page when the card's end is this many rows below the last row on screen. */
const READ_AHEAD_ROWS = 80;

function readSidebarShown(): boolean {
  try {
    return localStorage.getItem(SIDEBAR_KEY) !== 'false';
  } catch {
    return true;
  }
}

function writeSidebarShown(shown: boolean): void {
  try {
    localStorage.setItem(SIDEBAR_KEY, String(shown));
  } catch {
    // Storage unavailable: the choice lasts for this view only.
  }
}

function lastPathComponent(path: string): string {
  return path.slice(path.lastIndexOf('/') + 1);
}

function parentPath(path: string): string {
  const slash = path.lastIndexOf('/');
  return slash === -1 ? '' : path.slice(0, slash);
}

export interface DiffListViewProps {
  diff: DiffData | null;
  isLoading: boolean;
  emptyMessage: string;
  error: string | null;
  onRetry?: () => void;
  groups?: readonly DiffFileGroup[];
  loadPage?: LoadPage;
  onLoadPreview?: (file: FileDiff) => void;
}

/** One row of the virtualized list. */
interface ListRow {
  readonly key: string;
  readonly entryId: string;
  readonly card: DiffCardController | null;
  readonly row:
    CardRow | { readonly kind: 'group'; readonly title: string } | { readonly kind: 'error' };
  /** First row of a card that is not the list's first card: carries the 12px gap. */
  readonly gapAbove: boolean;
  readonly lastOfCard: boolean;
}

function estimate(row: ListRow, width: number): number {
  const gap = row.gapAbove ? 12 : 0;
  switch (row.row.kind) {
    case 'group':
      return 26;
    case 'error':
      return 60;
    case 'header':
      return gap + 34;
    case 'line':
      return diffLineHeight(row.row.line, width);
    case 'separator':
      return DIFF_METRICS.separatorHeight;
    case 'pager':
      return 44;
    default:
      return 36;
  }
}

export function DiffListView({
  diff,
  isLoading,
  emptyMessage,
  error,
  onRetry,
  groups,
  loadPage,
  onLoadPreview,
}: DiffListViewProps) {
  const [showSidebar, setShowSidebar] = useState(readSidebarShown);
  const [search, setSearch] = useState('');
  const [selectedFile, setSelectedFile] = useState<string | null>(null);
  const [version, rerender] = useReducer((count: number) => count + 1, 0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(800);

  const entries = useMemo<ReviewFileEntry[]>(
    () =>
      (groups ?? [{ title: '', files: diff?.files ?? [] }]).flatMap((group) =>
        group.files.map((file, index) => ({ file, group: group.title, startsGroup: index === 0 })),
      ),
    [groups, diff],
  );

  // One controller per entry, kept while the entry stays (cards outlive their scrolled-out rows).
  const controllers = useRef(new Map<string, DiffCardController>());
  const cards = useMemo(() => {
    const map = controllers.current;
    const ids = new Set<string>();
    const result = entries.map((entry) => {
      const id = reviewFileEntryId(entry);
      ids.add(id);
      let card = map.get(id);
      if (card === undefined) {
        card = new DiffCardController(entry.file, {
          loadPage: loadPage ?? null,
          onLoadPreview:
            onLoadPreview === undefined
              ? null
              : () => {
                  onLoadPreview(entry.file);
                },
          onChange: rerender,
        });
        map.set(id, card);
      } else {
        card.setFile(entry.file);
      }
      return { entry, id, card };
    });
    for (const [id, card] of map) {
      if (!ids.has(id)) {
        card.release();
        map.delete(id);
      }
    }
    return result;
  }, [entries, loadPage, onLoadPreview]);

  // Cards that are expanded on arrival load their first page.
  useEffect(() => {
    for (const { card } of cards) card.ensureLoaded();
  }, [cards]);

  useEffect(
    () => () => {
      for (const card of controllers.current.values()) card.release();
    },
    [],
  );

  // Every card's rows; rebuilt when a card changes (`version`) or the entries do.
  const rows = useMemo(() => {
    const rows: ListRow[] = [];
    if (error !== null)
      rows.push({
        key: 'error',
        entryId: '',
        card: null,
        row: { kind: 'error' },
        gapAbove: false,
        lastOfCard: false,
      });
    cards.forEach(({ entry, id, card }, index) => {
      if (entry.group !== '' && entry.startsGroup) {
        rows.push({
          key: `g:${entry.group}`,
          entryId: id,
          card: null,
          row: { kind: 'group', title: entry.group },
          gapAbove: false,
          lastOfCard: false,
        });
      }
      const cardRows = card.rows();
      cardRows.forEach((row, rowIndex) => {
        rows.push({
          key: `${id}\u0001${row.kind === 'line' || row.kind === 'separator' || row.kind === 'notice' ? row.id : row.kind}`,
          entryId: id,
          card,
          row,
          gapAbove: rowIndex === 0 && index > 0 && !(entry.group !== '' && entry.startsGroup),
          lastOfCard: rowIndex === cardRows.length - 1,
        });
      });
    });
    return rows;
    // `version` counts card changes: the controllers mutate, so it is what invalidates their rows.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [cards, error, version]);

  const hasEntries = entries.length > 0;
  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (element === null) return;
    const observer = new ResizeObserver(() => {
      // The cards' content width: the list's padding (16) and the card borders (1) aside.
      setWidth(Math.max(0, element.clientWidth - 34));
    });
    observer.observe(element);
    setWidth(Math.max(0, element.clientWidth - 34));
    return () => {
      observer.disconnect();
    };
  }, [hasEntries]);

  const estimateSize = useCallback(
    (index: number) => {
      const row = rows[index];
      return row === undefined ? 20 : estimate(row, width);
    },
    [rows, width],
  );
  const getItemKey = useCallback((index: number) => rows[index]?.key ?? index, [rows]);
  const getScrollElement = useCallback(() => scrollRef.current, []);
  // The virtualizer hands back fresh functions each render; this app does not use the React
  // Compiler, so nothing memoizes across them.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement,
    estimateSize,
    getItemKey,
    overscan: 16,
    // Until the scroll view reports its size (and in tests, which have no layout).
    initialRect: { width: 800, height: 600 },
    paddingStart: 16,
    paddingEnd: 16,
  });

  // Re-estimate unmeasured rows when the width changes (wrapping changes).
  useEffect(() => {
    virtualizer.measure();
  }, [virtualizer, width]);

  const items = virtualizer.getVirtualItems();
  const lastVisible = items.at(-1)?.index ?? -1;

  // Read on: the next page of any card whose end is close below the screen.
  useEffect(() => {
    if (lastVisible < 0) return;
    const end = Math.min(rows.length - 1, lastVisible + READ_AHEAD_ROWS);
    for (let index = items[0]?.index ?? 0; index <= end; index += 1) {
      const row = rows[index];
      // A card's pager is its last row: its end is near.
      if (row?.row.kind === 'pager' && row.card?.canReadOn === true) row.card.loadMore();
    }
  });

  const reveal = useCallback(
    (id: string) => {
      setSelectedFile(id);
      const card = controllers.current.get(id);
      card?.reveal();
      requestAnimationFrame(() => {
        const index = rows.findIndex((row) => row.entryId === id && row.row.kind === 'header');
        if (index !== -1) virtualizer.scrollToIndex(index, { align: 'start' });
      });
    },
    [rows, virtualizer],
  );

  const sidebarLabel = showSidebar ? 'Hide changed files' : 'Show changed files';
  const filtered = entries.filter(
    (entry) =>
      search === '' || entry.file.filename.toLocaleLowerCase().includes(search.toLocaleLowerCase()),
  );

  let middle;
  if (isLoading && diff === null && entries.length === 0) {
    middle = (
      <div className="diff-list-state">
        <span className="diff-spinner" role="status" aria-label="Loading" />
      </div>
    );
  } else if (entries.length === 0) {
    middle = (
      <div className="diff-list-state">
        <Icon name={error === null ? 'checkmark.circle' : 'exclamationmark.triangle'} size={28} />
        <span className="diff-list-message">{error ?? emptyMessage}</span>
        {error !== null && onRetry !== undefined && (
          <button type="button" className="detail-button detail-bordered-button" onClick={onRetry}>
            Retry
          </button>
        )}
      </div>
    );
  } else {
    middle = (
      <div className="diff-list-scroll" ref={scrollRef} data-testid="diff-list-scroll">
        <div className="diff-list-inner" style={{ height: virtualizer.getTotalSize() }}>
          {items.map((item) => {
            const row = rows[item.index];
            if (row === undefined) return null;
            const kind = row.row.kind;
            const className = [
              'diff-list-row',
              row.card === null ? '' : 'diff-card-row',
              kind === 'header' ? 'diff-card-top' : '',
              row.lastOfCard ? 'diff-card-bottom' : '',
              row.gapAbove ? 'gap-above' : '',
            ].join(' ');
            return (
              <div
                key={item.key}
                data-index={item.index}
                ref={virtualizer.measureElement}
                className={className}
                style={{ transform: `translateY(${String(item.start)}px)` }}
              >
                {row.row.kind === 'group' ? (
                  <div className="diff-group-title">{row.row.title}</div>
                ) : row.row.kind === 'error' ? (
                  <div className="diff-list-error">
                    <span>{error}</span>
                    {onRetry !== undefined && (
                      <button
                        type="button"
                        className="detail-button detail-bordered-button"
                        onClick={onRetry}
                      >
                        Retry
                      </button>
                    )}
                  </div>
                ) : row.card === null ? null : (
                  <DiffCardRowView card={row.card} row={row.row} />
                )}
              </div>
            );
          })}
        </div>
      </div>
    );
  }

  return (
    <div className="diff-list" data-view="DiffListView">
      <div className="diff-list-bar">
        <span className="diff-list-count">{`${String(entries.length)} changed files`}</span>
        <span className="detail-spacer" />
        <button
          type="button"
          className="detail-icon-button"
          aria-label={sidebarLabel}
          title={sidebarLabel}
          onClick={() => {
            setShowSidebar((shown) => {
              writeSidebarShown(!shown);
              return !shown;
            });
          }}
        >
          <Icon name="sidebar.right" size={14} />
        </button>
      </div>
      <div className="diff-list-split">
        <div className="diff-list-middle">{middle}</div>
        {showSidebar && (
          <nav className="diff-navigator" aria-label="Changed files">
            <div className="diff-navigator-bar">
              <span className="diff-navigator-title">Files</span>
              <span className="diff-navigator-count">{entries.length}</span>
            </div>
            <input
              className="diff-navigator-search"
              type="search"
              placeholder="Find changed file"
              aria-label="Find changed file"
              value={search}
              onChange={(event) => {
                setSearch(event.target.value);
              }}
            />
            <div className="diff-navigator-list">
              {filtered.map((entry) => {
                const id = reviewFileEntryId(entry);
                const folder = parentPath(entry.file.filename);
                return (
                  <button
                    key={id}
                    type="button"
                    className="diff-navigator-row"
                    aria-current={selectedFile === id ? 'true' : undefined}
                    aria-label={`Show ${entry.group} diff for ${entry.file.filename}`}
                    title={entry.file.filename}
                    onClick={() => {
                      reveal(id);
                    }}
                  >
                    <Icon name="doc.text" size={12} className="diff-navigator-icon" />
                    <span className="diff-navigator-names">
                      <span className="diff-navigator-name">
                        {lastPathComponent(entry.file.filename)}
                      </span>
                      <span className="diff-navigator-folder">
                        {entry.group === '' ? folder : `${entry.group} · ${folder}`}
                      </span>
                    </span>
                    <span className="diff-card-added">{`+${String(entry.file.added)}`}</span>
                    <span className="diff-card-removed">{`−${String(entry.file.removed)}`}</span>
                  </button>
                );
              })}
            </div>
          </nav>
        )}
      </div>
    </div>
  );
}
