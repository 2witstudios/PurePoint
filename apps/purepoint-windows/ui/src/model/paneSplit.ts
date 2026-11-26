// Port of the parts of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/PaneSplitNode.swift
// that the workspace model needs (WIN-6.1). WIN-6.2 owns the rest of the file (topTrailingLeafId,
// rowCount) and its PaneSplitNodeTests.
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

/** Whether the tree can take another split. Swift takes an axis it ignores; only the count matters. */
export function canSplit(node: PaneSplitNode): boolean {
  return leafCount(node) < MAX_PANES;
}

// MARK: - Mutations (return a new tree)

/**
 * Split a leaf into two along `axis`. The new leaf takes `nextId`; the returned `nextId` is one
 * past it when the leaf was found (Swift's `inout nextId`).
 */
export function splittingLeaf(
  node: PaneSplitNode,
  targetId: number,
  axis: PaneAxis,
  nextId: number,
  ratio = 0.5,
): { root: PaneSplitNode; nextId: number } {
  if (node.type === 'leaf') {
    if (node.id !== targetId) return { root: node, nextId };
    return { root: split(axis, ratio, leaf(node.id), leaf(nextId)), nextId: nextId + 1 };
  }
  const first = splittingLeaf(node.first, targetId, axis, nextId, ratio);
  const second = splittingLeaf(node.second, targetId, axis, first.nextId, ratio);
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
