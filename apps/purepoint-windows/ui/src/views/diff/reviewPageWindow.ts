// The pages a diff card shows (WIN-8.3): a run of consecutive review pages, appended as the user
// scrolls, up to a bound. Swift's `ReviewPageHistory` keeps one page body and pages with
// Previous/More; here the card reads on continuously (a 5,000-line diff scrolls top to bottom)
// and falls back to the Mac's paging only past `MAX_WINDOW_LINES`, so memory stays bounded.
// Like the history, every visited page leaves a cursor checkpoint, so earlier pages reload from
// the backend's snapshot instead of staying in memory.
import { continuation, GitReviewError } from '../../model/diff.ts';
import type { ReviewPage, ReviewPageCursor } from '../../services/git.ts';

/** Lines one card keeps in memory before "More lines" replaces the window (50 pages of 200). */
export const MAX_WINDOW_LINES = 10_000;

const lineCount = (page: ReviewPage) =>
  page.hunks.reduce((sum, hunk) => sum + hunk.lines.length, 0);

export class ReviewPageWindow {
  private checkpoints: (ReviewPageCursor | null)[] = [null];
  private loaded: ReviewPage[] = [];
  private firstIndex = 0;

  /** Index of the first page in the window. */
  get first(): number {
    return this.firstIndex;
  }

  /** Index of the last page in the window (first − 1 when empty). */
  get last(): number {
    return this.firstIndex + this.loaded.length - 1;
  }

  get pages(): readonly ReviewPage[] {
    return this.loaded;
  }

  get isEmpty(): boolean {
    return this.loaded.length === 0;
  }

  get retainedLineCount(): number {
    return this.loaded.reduce((sum, page) => sum + lineCount(page), 0);
  }

  /** The last page says there is more patch after it. */
  get hasMore(): boolean {
    return this.loaded.at(-1)?.hasMore ?? false;
  }

  /** Another page can join the window rather than replace it. */
  get canExtend(): boolean {
    return this.retainedLineCount < MAX_WINDOW_LINES;
  }

  /** The cursor that loads page `index`; throws when the preceding page was never loaded. */
  cursor(index: number): ReviewPageCursor | null {
    if (index < 0 || index >= this.checkpoints.length) {
      throw new GitReviewError('Load the preceding page first.');
    }
    return this.checkpoints[index] ?? null;
  }

  /**
   * Store page `index`: appended when it follows the window and the window has room, otherwise
   * the window becomes that page alone.
   */
  store(page: ReviewPage, index: number): void {
    if (index === this.last + 1 && !this.isEmpty && this.canExtend) {
      this.loaded = [...this.loaded, page];
    } else {
      this.loaded = [page];
      this.firstIndex = index;
    }
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

  /** Drop the bodies (a collapsed or unmounted card); the checkpoints stay. */
  releaseBodies(): void {
    this.loaded = [];
  }
}
