// Port of apps/purepoint-macos/purepoint-macos/Models/ProjectOrder.swift (WIN-6.5).
//
// The sidebar's project order. Each machine's daemon saves the order of its projects so every
// device lists them the same way; these helpers apply it to the rows on screen.

/** `items` with the one rooted at `root` moved to `index`, or null when nothing moves. */
export function movingProject<T>(
  root: string,
  index: number,
  items: readonly T[],
  rootOf: (item: T) => string,
): T[] | null {
  const from = items.findIndex((item) => rootOf(item) === root);
  if (from === -1) return null;
  const next = [...items];
  const [item] = next.splice(from, 1) as [T];
  const to = Math.min(Math.max(index, 0), next.length);
  if (to === from) return null;
  next.splice(to, 0, item);
  return next;
}

/**
 * `items` sorted into the order of `roots`. Items `roots` doesn't list keep their relative order
 * after the ones it does.
 */
export function sortedByProjectOrder<T>(
  items: readonly T[],
  roots: readonly string[],
  rootOf: (item: T) => string,
): T[] {
  const rank = new Map<string, number>();
  roots.forEach((root, index) => {
    if (!rank.has(root)) rank.set(root, index);
  });
  return items
    .map((element, offset) => ({ element, offset }))
    .sort((left, right) => {
      const l = rank.get(rootOf(left.element)) ?? Number.MAX_SAFE_INTEGER;
      const r = rank.get(rootOf(right.element)) ?? Number.MAX_SAFE_INTEGER;
      return l !== r ? l - r : left.offset - right.offset;
    })
    .map(({ element }) => element);
}

/**
 * Sends one daemon's project order (`reorder_projects`) and answers whether the daemon replied
 * with `project_order`. WIN-7.8 supplies it from the daemon client.
 */
export type ProjectOrderSend = (projectRoots: readonly string[]) => Promise<boolean>;

/**
 * Saves one daemon's project order. Moves are sent one at a time, in the order they were made, so
 * the daemon always ends up with the last one; each send carries the full order.
 */
export class ProjectOrderSaver {
  private last: Promise<void> = Promise.resolve();
  private pendingCount = 0;

  constructor(private readonly send: ProjectOrderSend) {}

  /**
   * Moves sent but not yet answered. While any are, a project list pushed or fetched from the
   * daemon may predate them, so its order must not replace the one on screen.
   */
  get pending(): number {
    return this.pendingCount;
  }

  save(roots: readonly string[], failed: () => void): Promise<void> {
    const previous = this.last;
    this.pendingCount += 1;
    const next = previous.then(async () => {
      let ok: boolean;
      try {
        ok = await this.send(roots);
      } catch {
        ok = false;
      }
      this.pendingCount -= 1;
      if (!ok) failed();
    });
    this.last = next;
    return next;
  }
}
