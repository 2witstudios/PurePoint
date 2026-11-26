// Port of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/PaneSplitNode.swift (WIN-6.1, completed
// in WIN-6.2), plus the split geometry of Views/PaneGrid/DraggableSplit.swift (`sizes`, `ratio`) so
// its tests run without the view, which WIN-7.4 renders.
//
// The tree is geometry only: it says *where* panes are, never *what* they show. A leaf's content
// is its tab stack, kept in `Workspace.panes` under the leaf's ID, so moving a tab between panes
// never touches the tree and every tree operation stays content-agnostic.

/** `horizontal` stacks top/bottom; `vertical` sits left/right. */
export type PaneAxis = 'horizontal' | 'vertical';

export type PaneSplitNode =
  | { readonly type: 'leaf'; readonly id: number }
  | {
      readonly type: 'split';
      readonly axis: PaneAxis;
      readonly ratio: number;
      readonly first: PaneSplitNode;
      readonly second: PaneSplitNode;
    };

/** The most panes a workspace holds (2 rows x 3 columns). */
export const MAX_PANES = 6;

export function leaf(id: number): PaneSplitNode {
  return { type: 'leaf', id };
}

export function split(
  axis: PaneAxis,
  ratio: number,
  first: PaneSplitNode,
  second: PaneSplitNode,
): PaneSplitNode {
  return { type: 'split', axis, ratio, first, second };
}

// MARK: - Queries

export function allLeafIds(node: PaneSplitNode): number[] {
  return node.type === 'leaf' ? [node.id] : [...allLeafIds(node.first), ...allLeafIds(node.second)];
}

export function leafCount(node: PaneSplitNode): number {
  return node.type === 'leaf' ? 1 : leafCount(node.first) + leafCount(node.second);
}

/** The pane in the grid's top-left corner. */
export function firstLeafId(node: PaneSplitNode): number {
  return node.type === 'leaf' ? node.id : firstLeafId(node.first);
}

export function lastLeafId(node: PaneSplitNode): number {
  return node.type === 'leaf' ? node.id : lastLeafId(node.second);
}

/** The pane in the grid's top-right corner, whose tab strip ends at the window's trailing edge. */
export function topTrailingLeafId(node: PaneSplitNode): number {
  if (node.type === 'leaf') return node.id;
  return node.axis === 'horizontal'
    ? topTrailingLeafId(node.first)
    : topTrailingLeafId(node.second);
}

// MARK: - Structural analysis (2x3 constraint)

/** The number of rows: horizontal splits at the top level. */
export function rowCount(node: PaneSplitNode): number {
  if (node.type === 'leaf') return 1;
  return node.axis === 'horizontal' ? rowCount(node.first) + rowCount(node.second) : 1;
}

/** Whether the tree can take another split. Swift takes an axis it ignores; only the count matters. */
export function canSplit(node: PaneSplitNode): boolean {
  return leafCount(node) < MAX_PANES;
}

// MARK: - Mutations (return a new tree)

/**
 * Split a leaf into two along `axis`. The new leaf takes `nextId`; the returned `nextId` is one
 * past it when the leaf was found (Swift's `inout nextId`). The new leaf goes after the old one
 * (right or below) unless `newFirst` puts it before (left or above): a tab dropped on a pane's
 * leading or top edge (WIN-7.4).
 */
export function splittingLeaf(
  node: PaneSplitNode,
  targetId: number,
  axis: PaneAxis,
  nextId: number,
  ratio = 0.5,
  newFirst = false,
): { root: PaneSplitNode; nextId: number } {
  if (node.type === 'leaf') {
    if (node.id !== targetId) return { root: node, nextId };
    const root = newFirst
      ? split(axis, ratio, leaf(nextId), leaf(node.id))
      : split(axis, ratio, leaf(node.id), leaf(nextId));
    return { root, nextId: nextId + 1 };
  }
  const first = splittingLeaf(node.first, targetId, axis, nextId, ratio, newFirst);
  const second = splittingLeaf(node.second, targetId, axis, first.nextId, ratio, newFirst);
  return {
    root: split(node.axis, node.ratio, first.root, second.root),
    nextId: second.nextId,
  };
}

/** Remove a leaf and collapse the tree. `null` when nothing is left. */
export function removingLeaf(node: PaneSplitNode, targetId: number): PaneSplitNode | null {
  return removingLeaves(node, new Set([targetId]));
}

/** Remove several leaves at once and collapse the tree. `null` when nothing is left. */
export function removingLeaves(
  node: PaneSplitNode,
  targetIds: ReadonlySet<number>,
): PaneSplitNode | null {
  if (node.type === 'leaf') return targetIds.has(node.id) ? null : node;
  const first = removingLeaves(node.first, targetIds);
  const second = removingLeaves(node.second, targetIds);
  if (first === null) return second;
  if (second === null) return first;
  return split(node.axis, node.ratio, first, second);
}

/** Update the ratio of the split identified by its first child's first leaf ID. */
export function settingRatio(
  node: PaneSplitNode,
  ratio: number,
  targetLeafId: number,
): PaneSplitNode {
  if (node.type === 'leaf') return node;
  if (firstLeafId(node.first) === targetLeafId) {
    return split(node.axis, ratio, node.first, node.second);
  }
  return split(
    node.axis,
    node.ratio,
    settingRatio(node.first, ratio, targetLeafId),
    settingRatio(node.second, ratio, targetLeafId),
  );
}

// MARK: - Spatial navigation

interface PathStep {
  readonly node: PaneSplitNode;
  /** True when the path descended into `first`. */
  readonly wentFirst: boolean;
}

function pathTo(node: PaneSplitNode, leafId: number): PathStep[] | null {
  if (node.type === 'leaf') return node.id === leafId ? [] : null;
  const first = pathTo(node.first, leafId);
  if (first !== null) return [{ node, wentFirst: true }, ...first];
  const second = pathTo(node.second, leafId);
  if (second !== null) return [{ node, wentFirst: false }, ...second];
  return null;
}

/**
 * The nearest leaf in a direction from `leafId`: walk up to the nearest split on `axis`, then
 * into the other child's nearest leaf.
 */
export function findAdjacentLeaf(
  node: PaneSplitNode,
  leafId: number,
  axis: PaneAxis,
  forward: boolean,
): number | null {
  const path = pathTo(node, leafId);
  if (path === null) return null;
  for (let i = path.length - 1; i >= 0; i--) {
    const step = path[i];
    if (step === undefined || step.node.type !== 'split' || step.node.axis !== axis) continue;
    if (forward && step.wentFirst) return firstLeafId(step.node.second);
    if (!forward && !step.wentFirst) return lastLeafId(step.node.first);
  }
  return null;
}

/** The first leaf of a leaf's sibling subtree: the pane that fills the space when it closes. */
export function siblingLeafId(node: PaneSplitNode, leafId: number): number | null {
  const path = pathTo(node, leafId);
  const last = path?.at(-1);
  if (last === undefined || last.node.type !== 'split') return null;
  return firstLeafId(last.wentFirst ? last.node.second : last.node.first);
}

/** Structural equality of two trees (Swift's synthesized `==`). */
export function nodesEqual(a: PaneSplitNode, b: PaneSplitNode): boolean {
  if (a.type === 'leaf' || b.type === 'leaf') {
    return a.type === 'leaf' && b.type === 'leaf' && a.id === b.id;
  }
  return (
    a.axis === b.axis &&
    a.ratio === b.ratio &&
    nodesEqual(a.first, b.first) &&
    nodesEqual(a.second, b.second)
  );
}

// MARK: - Split geometry (DraggableSplit.swift)

/** Thickness of the hairline between a split's two sides. */
export const SPLIT_DIVIDER_THICKNESS = 1;
/** The hit area each side of the hairline: a finger or a pointer needs more than the hairline. */
export const SPLIT_DRAG_INSET = 8;
/** The smallest a pane can be dragged to. */
export const SPLIT_MIN_PANE_SIZE = 120;

/**
 * The two children's sizes along the split axis in a split `total` long. A ratio of 0 or 1
 * collapses one side and drops the divider.
 */
export function splitSizes(total: number, ratio: number): { first: number; second: number } {
  const length = Math.max(total, 0);
  if (ratio <= 0) return { first: 0, second: length };
  if (ratio >= 1) return { first: length, second: 0 };
  const available = Math.max(length - SPLIT_DIVIDER_THICKNESS, 0);
  // Swift's `rounded()` rounds half away from zero; the product is never negative here.
  const first = Math.round(available * ratio);
  return { first, second: available - first };
}

/**
 * The ratio after dragging `translation` from `start`, keeping each side at least
 * `SPLIT_MIN_PANE_SIZE` when the split has room for both.
 */
export function splitDragRatio(start: number, translation: number, total: number): number {
  if (!(total > 0)) return 0.5;
  const proposed = start + translation / total;
  const minRatio = SPLIT_MIN_PANE_SIZE / total;
  if (!(minRatio < 0.5)) return 0.5;
  return Math.min(Math.max(proposed, minRatio), 1 - minRatio);
}
