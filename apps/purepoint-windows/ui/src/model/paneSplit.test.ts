// Ports of `PaneSplitNodeTests` (apps/purepoint-macos/purepoint-macosTests/purepoint_macosTests.swift)
// and DraggableSplitTests.swift (WIN-6.2). Each title keeps the Swift test function name.

import { describe, expect, it } from 'vitest';
import {
  SPLIT_DIVIDER_THICKNESS,
  SPLIT_DRAG_INSET,
  SPLIT_MIN_PANE_SIZE,
  allLeafIds,
  canSplit,
  findAdjacentLeaf,
  firstLeafId,
  leaf,
  leafCount,
  nodesEqual,
  removingLeaf,
  rowCount,
  settingRatio,
  siblingLeafId,
  split,
  splitDragRatio,
  splitSizes,
  splittingLeaf,
  topTrailingLeafId,
  type PaneSplitNode,
} from './paneSplit';

/** Split the last leaf `times` times, as the Swift tests build 5- and 6-leaf trees. */
function chain(times: number): PaneSplitNode {
  let nextId = 1;
  let tree = leaf(0);
  for (let i = 0; i < times; i++) {
    const target = allLeafIds(tree).at(-1) ?? 0;
    const result = splittingLeaf(tree, target, 'vertical', nextId);
    tree = result.root;
    nextId = result.nextId;
  }
  return tree;
}

describe('PaneSplitNodeTests', () => {
  // MARK: Leaf creation

  it('leafNodeStoresId', () => {
    const node = leaf(1);
    expect(allLeafIds(node)).toEqual([1]);
    expect(leafCount(node)).toBe(1);
  });

  it('leafRowCountIsOne', () => {
    expect(rowCount(leaf(5))).toBe(1);
  });

  // MARK: Splitting

  it('splitHorizontallyCreatesTwo', () => {
    const { root, nextId } = splittingLeaf(leaf(0), 0, 'horizontal', 1);
    expect(leafCount(root)).toBe(2);
    expect(allLeafIds(root)).toEqual([0, 1]);
    expect(nextId).toBe(2);
  });

  it('splitVerticallyCreatesTwo', () => {
    const { root } = splittingLeaf(leaf(0), 0, 'vertical', 1);
    expect(leafCount(root)).toBe(2);
    expect(allLeafIds(root)).toEqual([0, 1]);
  });

  it('splitKeepsOriginalLeafFirst', () => {
    const { root } = splittingLeaf(leaf(0), 0, 'horizontal', 1);
    expect(root).toEqual(split('horizontal', 0.5, leaf(0), leaf(1)));
  });

  it('splitNonexistentIdIsNoOp', () => {
    const { root, nextId } = splittingLeaf(leaf(0), 99, 'horizontal', 1);
    expect(root).toEqual(leaf(0));
    expect(nextId).toBe(1);
  });

  it('splitNestedLeaf', () => {
    const tree = split('horizontal', 0.5, leaf(0), leaf(1));
    const { root } = splittingLeaf(tree, 1, 'vertical', 2);
    expect(leafCount(root)).toBe(3);
    expect(allLeafIds(root)).toEqual([0, 1, 2]);
  });

  it('splitNestedLeafPreservesCustomRatio', () => {
    const tree = split('horizontal', 0.5, leaf(0), leaf(1));
    const { root } = splittingLeaf(tree, 1, 'vertical', 2, 0.7);
    // The inner split created for leaf 1 has ratio 0.7.
    expect(root).toEqual(
      split('horizontal', 0.5, leaf(0), split('vertical', 0.7, leaf(1), leaf(2))),
    );
  });

  // MARK: Top-trailing leaf

  it('singleLeafIsTopTrailing', () => {
    expect(topTrailingLeafId(leaf(3))).toBe(3);
  });

  it('rightPaneOfAVerticalSplitIsTopTrailing', () => {
    expect(topTrailingLeafId(split('vertical', 0.5, leaf(0), leaf(1)))).toBe(1);
  });

  it('topPaneOfAHorizontalSplitIsTopTrailing', () => {
    expect(topTrailingLeafId(split('horizontal', 0.5, leaf(0), leaf(1)))).toBe(0);
  });

  it('nestedGridResolvesTheTopRightPane', () => {
    // Two columns; the right column is split into a top (2) and bottom (1) pane.
    const tree = split('vertical', 0.5, leaf(0), split('horizontal', 0.5, leaf(2), leaf(1)));
    expect(topTrailingLeafId(tree)).toBe(2);
  });

  // MARK: Top-leading leaf

  it('leftPaneOfAVerticalSplitIsTopLeading', () => {
    expect(firstLeafId(split('vertical', 0.5, leaf(0), leaf(1)))).toBe(0);
  });

  it('topPaneOfAHorizontalSplitIsTopLeading', () => {
    expect(firstLeafId(split('horizontal', 0.5, leaf(1), leaf(0)))).toBe(1);
  });

  it('nestedGridResolvesTheTopLeftPane', () => {
    // Two rows; the top row is split into a left (3) and right (2) pane.
    const tree = split('horizontal', 0.5, split('vertical', 0.5, leaf(3), leaf(2)), leaf(0));
    expect(firstLeafId(tree)).toBe(3);
  });

  // MARK: Row count / canSplit

  it('horizontalSplitCountsRows', () => {
    expect(rowCount(split('horizontal', 0.5, leaf(0), leaf(1)))).toBe(2);
  });

  it('verticalSplitRowCountIsOne', () => {
    expect(rowCount(split('vertical', 0.5, leaf(0), leaf(1)))).toBe(1);
  });

  it('canSplitUnderSixLeaves', () => {
    expect(canSplit(leaf(0))).toBe(true);
  });

  it('canSplitAtFiveLeaves', () => {
    const tree = chain(4);
    expect(leafCount(tree)).toBe(5);
    expect(canSplit(tree)).toBe(true);
  });

  it('cannotSplitAtSixLeaves', () => {
    const tree = chain(5);
    expect(leafCount(tree)).toBe(6);
    expect(canSplit(tree)).toBe(false);
  });

  // MARK: Removing / closing

  it('removeSingleLeafReturnsNil', () => {
    expect(removingLeaf(leaf(0), 0)).toBeNull();
  });

  it('removeNonexistentLeafIsNoOp', () => {
    expect(removingLeaf(leaf(0), 99)).toEqual(leaf(0));
  });

  it('removeLeafCollapsesParent', () => {
    expect(removingLeaf(split('horizontal', 0.5, leaf(0), leaf(1)), 0)).toEqual(leaf(1));
  });

  it('removeDeeplyNestedLeaf', () => {
    const tree = split('vertical', 0.5, leaf(0), split('horizontal', 0.5, leaf(1), leaf(2)));
    const result = removingLeaf(tree, 1);
    expect(result).not.toBeNull();
    if (result === null) return;
    expect(leafCount(result)).toBe(2);
    expect(allLeafIds(result)).toEqual([0, 2]);
  });

  // MARK: Setting ratio

  it('settingRatioOnSplit', () => {
    const tree = split('vertical', 0.5, leaf(0), leaf(1));
    expect(settingRatio(tree, 0.7, 0)).toEqual(split('vertical', 0.7, leaf(0), leaf(1)));
  });

  // MARK: Spatial navigation

  it('findAdjacentLeafForward', () => {
    expect(findAdjacentLeaf(split('vertical', 0.5, leaf(0), leaf(1)), 0, 'vertical', true)).toBe(1);
  });

  it('findAdjacentLeafBackward', () => {
    expect(findAdjacentLeaf(split('vertical', 0.5, leaf(0), leaf(1)), 1, 'vertical', false)).toBe(
      0,
    );
  });

  it('findAdjacentLeafNoNeighbor', () => {
    // No horizontal neighbour exists.
    expect(
      findAdjacentLeaf(split('vertical', 0.5, leaf(0), leaf(1)), 0, 'horizontal', true),
    ).toBeNull();
  });

  it('siblingLeafId', () => {
    const tree = split('vertical', 0.5, leaf(0), leaf(1));
    expect(siblingLeafId(tree, 0)).toBe(1);
    expect(siblingLeafId(tree, 1)).toBe(0);
  });

  // MARK: Equatable

  it('equalityForLeaves', () => {
    expect(nodesEqual(leaf(0), leaf(0))).toBe(true);
    expect(nodesEqual(leaf(0), leaf(1))).toBe(false);
    expect(nodesEqual(split('vertical', 0.5, leaf(0), leaf(1)), leaf(0))).toBe(false);
  });
});

describe('DraggableSplitTests', () => {
  it('bothSidesAndTheDividerFillTheSplit', () => {
    for (const total of [801, 1000, 1194]) {
      for (const ratio of [0.1, 0.33, 0.5, 0.9]) {
        const sizes = splitSizes(total, ratio);
        expect(sizes.first + SPLIT_DIVIDER_THICKNESS + sizes.second).toBe(total);
      }
    }
    expect(splitSizes(1001, 0.5)).toEqual({ first: 500, second: 500 });
  });

  it('aCollapsedSideDropsTheDivider', () => {
    expect(splitSizes(800, 0)).toEqual({ first: 0, second: 800 });
    expect(splitSizes(800, 1)).toEqual({ first: 800, second: 0 });
  });

  it('aTinySplitNeverGivesANegativeSize', () => {
    const sizes = splitSizes(0.5, 0.5);
    expect(sizes.first).toBeGreaterThanOrEqual(0);
    expect(sizes.second).toBeGreaterThanOrEqual(0);
    expect(splitSizes(-10, 0.5)).toEqual({ first: 0, second: 0 });
  });

  it('dragMovesTheRatioByTheTranslation', () => {
    expect(Math.abs(splitDragRatio(0.5, 100, 1000) - 0.6)).toBeLessThan(1e-9);
    expect(Math.abs(splitDragRatio(0.5, -200, 1000) - 0.3)).toBeLessThan(1e-9);
  });

  it('dragKeepsEachPaneAtItsMinimum', () => {
    const total = 1000;
    const min = SPLIT_MIN_PANE_SIZE / total;
    expect(splitDragRatio(0.5, -900, total)).toBe(min);
    expect(splitDragRatio(0.5, 900, total)).toBe(1 - min);
  });

  it('aSplitTooSmallForTwoMinimumsStaysCentred', () => {
    expect(splitDragRatio(0.3, 50, SPLIT_MIN_PANE_SIZE * 1.5)).toBe(0.5);
    expect(splitDragRatio(0.3, 50, 0)).toBe(0.5);
  });

  it('theDividerHitAreaIsWideEnoughForAFingerOrPointer', () => {
    expect(SPLIT_DRAG_INSET * 2).toBeGreaterThanOrEqual(16);
  });
});
