// Port of apps/purepoint-macos/purepoint-macosTests/PaneTabStackTests.swift (WIN-6.2).
//
// The tab operations on a workspace: every one is pure, keeps each agent in exactly one tab, and
// never leaves a pane with no tabs. Each title keeps the Swift test function name.

import { describe, expect, it } from 'vitest';
import { leaf, split } from '../model/paneSplit';
import {
  PROJECT_ROOT,
  adoptingWorkspace,
  agentContent,
  breakTab,
  closePane,
  closeTab,
  contentAgentId,
  cycleTab,
  fileContent,
  focusedAgentId,
  isGhost,
  locationOfAgent,
  makeWorkspace,
  moveTab,
  moveTabBefore,
  newTab,
  normalizeWorkspace,
  paneActiveTab,
  paneCount,
  removeSurfaces,
  selectLastTab,
  selectTab,
  selectTabAt,
  setSurfaceContent,
  splitPane,
  workspaceAgentIds,
  workspaceSurfaces,
  type Workspace,
} from '../model/workspace';

/** One pane (leaf 0) holding tabs A, B, C, with B active. */
function stacked(): Workspace {
  let workspace = adoptingWorkspace('A', PROJECT_ROOT);
  workspace = newTab(workspace, 0, agentContent('B')).workspace;
  workspace = newTab(workspace, 0, agentContent('C')).workspace;
  return selectTabAt(workspace, 0, 1);
}

function labels(workspace: Workspace, leafId: number): string[] {
  return (workspace.panes[leafId]?.tabs ?? []).map((surface) => {
    switch (surface.content.kind) {
      case 'agent':
        return surface.content.agentId;
      case 'file':
        return surface.content.path ?? 'files';
      case 'empty':
        return '+';
      case 'pointGuard':
        return 'Point Guard';
    }
  });
}

function activeLabel(workspace: Workspace, leafId: number): string | null {
  const pane = workspace.panes[leafId];
  const tab = pane === undefined ? null : paneActiveTab(pane);
  return tab === null ? null : contentAgentId(tab.content);
}

function tabOf(workspace: Workspace, agentId: string): number {
  const location = locationOfAgent(workspace, agentId);
  expect(location).not.toBeNull();
  return location?.surfaceId ?? -1;
}

function splitWith(workspace: Workspace, agentId: string): Workspace {
  return splitPane(workspace, 0, 'vertical', agentContent(agentId)).workspace;
}

describe('PaneTabStackTests', () => {
  // MARK: - New tab

  it('newTabInsertsAfterTheActiveTabAndActivatesIt', () => {
    const { workspace, surfaceId } = newTab(stacked(), 0);

    expect(labels(workspace, 0)).toEqual(['A', 'B', '+', 'C']);
    expect(workspace.panes[0]?.activeTabId).toBe(surfaceId);
    expect(workspace.focusedLeafId).toBe(0);
  });

  it('newTabIdsAreUniqueAcrossPanes', () => {
    let workspace = splitPane(stacked(), 0, 'vertical').workspace;
    workspace = newTab(workspace, 1).workspace;

    const ids = workspaceSurfaces(workspace).map(({ surface }) => surface.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('newTabInMissingPaneDoesNothing', () => {
    const { workspace, surfaceId } = newTab(stacked(), 9);
    expect(surfaceId).toBeNull();
    expect(workspace).toEqual(stacked());
  });

  // MARK: - Select

  it('selectByPositionIgnoresOutOfRange', () => {
    let workspace = selectTabAt(stacked(), 0, 7);
    expect(activeLabel(workspace, 0)).toBe('B');
    workspace = selectTabAt(workspace, 0, 0);
    expect(activeLabel(workspace, 0)).toBe('A');
  });

  it('selectLastTab', () => {
    expect(activeLabel(selectLastTab(stacked(), 0), 0)).toBe('C');
  });

  it('cycleWrapsAtBothEnds', () => {
    let workspace = cycleTab(stacked(), 0, 1);
    expect(activeLabel(workspace, 0)).toBe('C');
    workspace = cycleTab(workspace, 0, 1);
    expect(activeLabel(workspace, 0)).toBe('A');
    workspace = cycleTab(workspace, 0, -1);
    expect(activeLabel(workspace, 0)).toBe('C');
  });

  it('selectingATabFocusesItsPane', () => {
    let workspace = splitPane(stacked(), 0, 'vertical').workspace;
    expect(workspace.focusedLeafId).toBe(1);

    workspace = selectTab(workspace, tabOf(workspace, 'C'));

    expect(workspace.focusedLeafId).toBe(0);
    expect(focusedAgentId(workspace)).toBe('C');
  });

  // MARK: - Close

  it('closingTheActiveTabActivatesItsLeftNeighbour', () => {
    const start = stacked();
    const { workspace, agentIds } = closeTab(start, tabOf(start, 'B'));

    expect(agentIds).toEqual(['B']);
    expect(labels(workspace, 0)).toEqual(['A', 'C']);
    expect(activeLabel(workspace, 0)).toBe('A');
  });

  it('closingTheLeftmostActiveTabActivatesTheNewFirstTab', () => {
    const start = selectTabAt(stacked(), 0, 0);
    const { workspace } = closeTab(start, tabOf(start, 'A'));
    expect(activeLabel(workspace, 0)).toBe('B');
  });

  it('closingABackgroundTabKeepsTheActiveOne', () => {
    const start = stacked();
    const { workspace } = closeTab(start, tabOf(start, 'C'));
    expect(activeLabel(workspace, 0)).toBe('B');
  });

  it('closingAPanesLastTabCollapsesThePane', () => {
    const start = splitWith(stacked(), 'D');

    const { workspace, agentIds } = closeTab(start, tabOf(start, 'D'));

    expect(agentIds).toEqual(['D']);
    expect(paneCount(workspace)).toBe(1);
    expect(Object.keys(workspace.panes).map(Number)).toEqual([0]);
    expect(workspace.focusedLeafId).toBe(0);
  });

  it('closingAPaneReturnsEveryAgentInIt', () => {
    let start = splitWith(stacked(), 'D');
    start = newTab(start, 1, fileContent(null)).workspace;

    const { workspace, agentIds } = closePane(start, 0);

    expect(agentIds).toEqual(['A', 'B', 'C']);
    expect(workspace.root).toEqual(leaf(1));
    expect(workspaceAgentIds(workspace)).toEqual(['D']);
  });

  // MARK: - Move

  it('moveTabToAnotherPaneKeepsItsIdAndActivatesIt', () => {
    let workspace = splitWith(stacked(), 'D');
    const c = tabOf(workspace, 'C');

    workspace = moveTab(workspace, c, 1, 0);

    expect(labels(workspace, 0)).toEqual(['A', 'B']);
    expect(labels(workspace, 1)).toEqual(['C', 'D']);
    expect(workspace.panes[1]?.activeTabId).toBe(c);
    expect(workspace.focusedLeafId).toBe(1);
  });

  it('moveTabWithoutIndexAppends', () => {
    let workspace = splitWith(stacked(), 'D');
    workspace = moveTab(workspace, tabOf(workspace, 'A'), 1);
    expect(labels(workspace, 1)).toEqual(['D', 'A']);
  });

  it('moveTabReordersWithinAPane', () => {
    const start = stacked();
    expect(labels(moveTab(start, tabOf(start, 'C'), 0, 0), 0)).toEqual(['C', 'A', 'B']);
  });

  it('movingAPanesOnlyTabCollapsesTheSourcePane', () => {
    let workspace = splitWith(stacked(), 'D');

    workspace = moveTab(workspace, tabOf(workspace, 'D'), 0);

    expect(paneCount(workspace)).toBe(1);
    expect(labels(workspace, 0)).toEqual(['A', 'B', 'C', 'D']);
  });

  it('moveBeforeLaterTabInSamePaneLandsJustBeforeIt', () => {
    const start = stacked();
    const workspace = moveTabBefore(start, tabOf(start, 'A'), tabOf(start, 'C'));
    expect(labels(workspace, 0)).toEqual(['B', 'A', 'C']);
  });

  it('moveBeforeEarlierTabInSamePane', () => {
    const start = stacked();
    const workspace = moveTabBefore(start, tabOf(start, 'C'), tabOf(start, 'A'));
    expect(labels(workspace, 0)).toEqual(['C', 'A', 'B']);
  });

  it('moveBeforeTabInAnotherPane', () => {
    const start = splitWith(stacked(), 'D');
    const workspace = moveTabBefore(start, tabOf(start, 'B'), tabOf(start, 'D'));
    expect(labels(workspace, 0)).toEqual(['A', 'C']);
    expect(labels(workspace, 1)).toEqual(['B', 'D']);
  });

  it('moveBeforeItselfDoesNothing', () => {
    const start = stacked();
    const b = tabOf(start, 'B');
    expect(moveTabBefore(start, b, b)).toEqual(stacked());
  });

  it('moveTabToMissingPaneDoesNothing', () => {
    expect(moveTab(stacked(), 0, 9)).toEqual(stacked());
  });

  // MARK: - Break out / join

  it('breakTabMovesItIntoANewPaneBesideItsOwn', () => {
    const start = stacked();

    const { workspace, leafId } = breakTab(start, tabOf(start, 'B'), 'vertical');

    expect(leafId).toBe(1);
    expect(workspace.root).toEqual(split('vertical', 0.5, leaf(0), leaf(1)));
    expect(labels(workspace, 0)).toEqual(['A', 'C']);
    expect(labels(workspace, 1)).toEqual(['B']);
    expect(focusedAgentId(workspace)).toBe('B');
  });

  it('breakingAPanesOnlyTabDoesNothing', () => {
    const { workspace, leafId } = breakTab(adoptingWorkspace('A', PROJECT_ROOT), 0, 'vertical');
    expect(leafId).toBeNull();
    expect(paneCount(workspace)).toBe(1);
  });

  it('breakTabRespectsTheSplitLimit', () => {
    let workspace = stacked();
    for (let i = 0; i < 5; i++) workspace = splitPane(workspace, 0, 'vertical').workspace;
    expect(paneCount(workspace)).toBe(6);
    const result = breakTab(workspace, tabOf(workspace, 'B'), 'vertical');
    expect(result.leafId).toBeNull();
    expect(labels(result.workspace, 0)).toEqual(['A', 'B', 'C']);
  });

  // MARK: - Ghosts

  it('aWorkspaceWithOnlyEmptyTabsIsAGhost', () => {
    const start = stacked();
    expect(isGhost(start)).toBe(false);
    const { workspace } = removeSurfaces(start, (s) => contentAgentId(s.content) !== null);
    expect(isGhost(normalizeWorkspace(workspace))).toBe(true);
  });

  it('aFileTabKeepsAWorkspaceAlive', () => {
    let workspace = adoptingWorkspace('A', PROJECT_ROOT);
    workspace = newTab(workspace, 0, fileContent('/tmp/x.md')).workspace;
    workspace = closeTab(workspace, 0).workspace;
    expect(workspaceAgentIds(workspace)).toEqual([]);
    expect(isGhost(workspace)).toBe(false);
  });

  // MARK: - Content and pruning

  it('setContentReplacesOnlyThatTab', () => {
    const { workspace, surfaceId } = newTab(stacked(), 0);
    const updated = setSurfaceContent(workspace, agentContent('E'), surfaceId ?? -1);
    expect(labels(updated, 0)).toEqual(['A', 'B', 'E', 'C']);
  });

  it('removeSurfacesReportsWhenNothingIsLeft', () => {
    const first = removeSurfaces(stacked(), (s) => contentAgentId(s.content) === 'B');
    expect(first.survived).toBe(true);
    expect(labels(first.workspace, 0)).toEqual(['A', 'C']);
    expect(removeSurfaces(first.workspace, () => true).survived).toBe(false);
  });

  // MARK: - Normalize

  it('normalizeGivesMissingPanesAnEmptyTabAndFixesDuplicateIds', () => {
    const workspace = makeWorkspace({
      id: 'ws',
      container: PROJECT_ROOT,
      root: split('vertical', 0.5, leaf(0), leaf(1)),
      panes: {
        0: {
          tabs: [
            { id: 3, content: agentContent('A') },
            { id: 3, content: agentContent('B') },
          ],
          activeTabId: 3,
        },
        7: { tabs: [{ id: 9, content: agentContent('Z') }], activeTabId: 9 },
      },
      focusedLeafId: 5,
      nextLeafId: 0,
    });
    const normalized = normalizeWorkspace(workspace);

    const ids = workspaceSurfaces(normalized).map(({ surface }) => surface.id);
    expect(new Set(ids).size).toBe(ids.length);
    expect(Object.keys(normalized.panes).map(Number)).toEqual([0, 1]);
    expect(normalized.panes[1]?.tabs.map((tab) => tab.content)).toEqual([{ kind: 'empty' }]);
    expect(normalized.focusedLeafId).toBe(0);
    expect(normalized.nextLeafId).toBe(2);
    expect(normalized.nextSurfaceId).toBeGreaterThan(Math.max(0, ...ids));
  });
});
