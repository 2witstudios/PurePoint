// A tab dropped on a pane's edge (WIN-7.4): `dropTabOnEdge` and `splittingLeaf(newFirst)`.
import { describe, expect, it } from 'vitest';
import { leaf, split, splittingLeaf } from './paneSplit';
import {
  agentContent,
  dropTabOnEdge,
  makeWorkspace,
  paneCount,
  splitPane,
  workspaceAgentIds,
  type Workspace,
} from './workspace';
import { documentText, workspacesFromText } from './workspacePersistence';

/** Pane 0 holds A and B (A active); pane 1 holds C. */
function twoPanes(): Workspace {
  return makeWorkspace({
    id: 'ws',
    container: { kind: 'projectRoot' },
    root: split('vertical', 0.5, leaf(0), leaf(1)),
    panes: {
      0: {
        tabs: [
          { id: 0, content: agentContent('A') },
          { id: 1, content: agentContent('B') },
        ],
        activeTabId: 0,
      },
      1: { tabs: [{ id: 2, content: agentContent('C') }], activeTabId: 2 },
    },
    focusedLeafId: 0,
    nextLeafId: 2,
    nextSurfaceId: 3,
  });
}

describe('splittingLeaf', () => {
  it('puts the new leaf before the old one when asked', () => {
    expect(splittingLeaf(leaf(0), 0, 'horizontal', 1, 0.5, true).root).toEqual(
      split('horizontal', 0.5, leaf(1), leaf(0)),
    );
    expect(splittingLeaf(leaf(0), 0, 'horizontal', 1).root).toEqual(
      split('horizontal', 0.5, leaf(0), leaf(1)),
    );
  });
});

describe('dropTabOnEdge', () => {
  it('splits the target pane on the edge and moves the tab into the new pane', () => {
    const result = dropTabOnEdge(twoPanes(), 1, 1, 'left');
    expect(result.leafId).toBe(2);
    const w = result.workspace;
    expect(w.root).toEqual(
      split('vertical', 0.5, leaf(0), split('vertical', 0.5, leaf(2), leaf(1))),
    );
    expect(w.panes[2]).toEqual({ tabs: [{ id: 1, content: agentContent('B') }], activeTabId: 1 });
    expect(w.panes[0]?.tabs.map((t) => t.id)).toEqual([0]);
    expect(w.focusedLeafId).toBe(2);
    expect(workspaceAgentIds(w).sort()).toEqual(['A', 'B', 'C']);
  });

  it('splits below and right after the target pane', () => {
    const w = dropTabOnEdge(twoPanes(), 0, 1, 'bottom').workspace;
    expect(w.root).toEqual(
      split('vertical', 0.5, leaf(0), split('horizontal', 0.5, leaf(1), leaf(2))),
    );
  });

  it('collapses a source pane the move empties', () => {
    const w = dropTabOnEdge(twoPanes(), 2, 0, 'top').workspace;
    expect(paneCount(w)).toBe(2);
    expect(w.root).toEqual(split('horizontal', 0.5, leaf(2), leaf(0)));
    expect(w.panes[1]).toBeUndefined();
  });

  it('splits a pane with its own tab when it has others left', () => {
    const w = dropTabOnEdge(twoPanes(), 1, 0, 'right').workspace;
    expect(paneCount(w)).toBe(3);
    expect(w.panes[2]?.tabs.map((t) => t.id)).toEqual([1]);
  });

  it('does nothing for a lone tab on its own pane, an unknown tab, or a full grid', () => {
    const start = twoPanes();
    expect(dropTabOnEdge(start, 2, 1, 'left')).toEqual({ workspace: start, leafId: null });
    expect(dropTabOnEdge(start, 99, 1, 'left')).toEqual({ workspace: start, leafId: null });
    let full = start;
    for (let i = 0; i < 4; i++) full = splitPane(full, 0, 'vertical').workspace;
    expect(paneCount(full)).toBe(6);
    expect(dropTabOnEdge(full, 1, 1, 'right').leafId).toBeNull();
  });

  it('round-trips a leading-edge split through the layout document', () => {
    const w = dropTabOnEdge(twoPanes(), 1, 1, 'top').workspace;
    const text = documentText([w], 'ws');
    expect(text).not.toBeNull();
    const [restored] = workspacesFromText(text ?? '', '/tmp/project');
    expect(restored?.root).toEqual(w.root);
    expect(restored?.panes).toEqual(w.panes);
  });
});
