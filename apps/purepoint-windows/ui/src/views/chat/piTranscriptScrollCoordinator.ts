// Port of apps/purepoint-macos/purepoint-macos/Views/Chat/PiTranscriptScrollCoordinator.swift
// (WIN-9.5): keeps the transcript's scroll-to-bottom out of the layout pass, and bounded.
//
// A request is deferred one task (the Swift main-queue hop) and dropped when another is pending,
// when the view no longer wants it (`canScroll`), when the view is already at the bottom, or when
// this exact content/viewport size was already attempted since the output last changed: our own
// scrolling must never feed an unbounded loop of resize → scroll → resize.

export interface TranscriptScrollGeometry {
  readonly contentHeight: number;
  readonly viewportHeight: number;
  readonly visibleBottom: number;
}

export type Defer = (run: () => void) => void;

const defaultDefer: Defer = (run) => {
  setTimeout(run, 0);
};

export class PiTranscriptScrollCoordinator {
  private size: { content: number; viewport: number } | null = null;
  private atBottom = false;
  private readonly attempted = new Set<string>();
  private pending: symbol | null = null;
  private readonly defer: Defer;

  constructor(defer: Defer = defaultDefer) {
    this.defer = defer;
  }

  update({ contentHeight, viewportHeight, visibleBottom }: TranscriptScrollGeometry): void {
    this.size = { content: contentHeight, viewport: viewportHeight };
    this.atBottom = viewportHeight > 0 && visibleBottom >= contentHeight - 1;
  }

  outputChanged(): void {
    this.attempted.clear();
  }

  /** A disclosure toggle starts a new layout intent even without a bridge revision. */
  userLayoutChanged(): void {
    this.cancel();
    this.attempted.clear();
  }

  cancel(): void {
    this.pending = null;
  }

  reset(): void {
    this.cancel();
    this.size = null;
    this.atBottom = false;
    this.outputChanged();
  }

  /** Whether a request is waiting to run. */
  get isPending(): boolean {
    return this.pending !== null;
  }

  request(canScroll: () => boolean, scroll: () => void): void {
    if (this.pending !== null) return;
    const token = Symbol('scroll');
    this.pending = token;
    this.defer(() => {
      if (this.pending !== token) return;
      this.pending = null;
      if (!canScroll() || this.atBottom) return;
      const size = this.size;
      if (size !== null) {
        const key = `${String(size.content)}x${String(size.viewport)}`;
        if (size.viewport <= 0 || this.attempted.has(key)) return;
        // Remember attempted sizes until output or user layout intent changes.
        this.attempted.add(key);
      }
      scroll();
    });
  }
}
