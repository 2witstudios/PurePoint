// Port of apps/purepoint-macos/purepoint-macosTests/WorkspaceReconcilerTests.swift (WIN-6.1).
//
// Each title keeps the Swift test function name. The three restart cases that save and load
// through `WorkspacePersistence` stand in a JSON round trip for the file until WIN-6.3 ports it;
// the cases that only exercise the file format are `todo` here and land with WIN-6.3.

import type { AgentEntry } from '../protocol';
import { leaf } from './paneSplit';
import {
  PROJECT_ROOT,
  adoptingWorkspace,
  agentContent,
  fileContent,
  focusedAgentId,
  focusedSurface,
  liveAgent,
  liveAgents,
  locationOfAgent,
  makeWorkspace,
  mergeWorkspaceLayouts,
  newTab,
  paneCount,
  placementRekeyed,
  pointGuardId,
  reconcileWorkspaces,
  removeSurfaces,
  selectTab,
  splitPane,
  surfaceById,
  tabCount,
  workspaceAgentIds,
  worktreeContainer,
  type LiveAgent,
  type Workspace,
} from './workspace';
import { allLeafIds } from './paneSplit';
import {
  agentModelFromEntry,
  makeAgentModel,
  makeWorktreeModel,
  type AgentModel,
} from './workspaceModel';

function makeAgent(id: string): AgentModel {
  return makeAgentModel({
    id,
    name: id,
    agentType: 'claude',
    status: 'running',
    prompt: '',
    startedAt: '',
  });
}

function makeWorktree(id: string, agents: string[]) {
  return makeWorktreeModel({
    id,
    name: id,
    path: `/tmp/${id}`,
    branch: `pu/${id}`,
    status: 'active',
    agents: agents.map(makeAgent),
  });
}

function live(root: string[] = [], worktrees: [string, string[]][] = []): LiveAgent[] {
  return liveAgents(
    root.map(makeAgent),
    worktrees.map(([id, agents]) => makeWorktree(id, agents)),
  );
}

/** A three-pane workspace: ag-a split right into ag-b, then that split down into ag-c. */
function makeGroupedWorkspace(): Workspace {
  let workspace = adoptingWorkspace('ag-a', PROJECT_ROOT);
  workspace = splitPane(workspace, 0, 'vertical', agentContent('ag-b')).workspace;
  return splitPane(workspace, workspace.focusedLeafId, 'horizontal', agentContent('ag-c'))
    .workspace;
}

/** Stand-in for `WorkspacePersistence.save` then `load` until WIN-6.3 ports the file format. */
function restart(workspaces: Workspace[]): Workspace[] {
  return JSON.parse(JSON.stringify(workspaces)) as Workspace[];
}

/**
 * The property the whole workspace model exists to guarantee: if it holds, a pane cannot also be
 * a loose sidebar row, because rows are generated from workspaces and each agent lives in one.
 */
function expectInvariant(result: Workspace[], agents: LiveAgent[]): void {
  const placed = result.flatMap(workspaceAgentIds);
  expect(new Set(placed)).toEqual(new Set(agents.map((agent) => agent.id)));
  expect(placed.length, 'an agent appears in more than one pane').toBe(agents.length);
  expect(
    result.every((workspace) => workspaceAgentIds(workspace).length > 0),
    'a workspace holds no agents',
  ).toBe(true);
  expect(new Set(result.map((workspace) => workspace.id)).size, 'duplicate workspace ids').toBe(
    result.length,
  );
}

/** ag-a with an empty tab split beside it, as a split made on any machine leaves it. */
function makeSplitWaitingForAgent(): [Workspace, number] {
  const { workspace } = splitPane(adoptingWorkspace('ag-a', PROJECT_ROOT), 0, 'vertical');
  const tab = focusedSurface(workspace);
  if (tab === null) throw new Error('split left no focused tab');
  return [workspace, tab.id];
}

describe('WorkspaceReconcilerTests', () => {
  // MARK: - The invariant

  it('givenNoStoredLayoutEachAgentBecomesItsOwnWorkspace', () => {
    const agents = live(['ag-a', 'ag-b'], [['wt-1', ['ag-c']]]);
    const result = reconcileWorkspaces([], agents);

    expectInvariant(result, agents);
    expect(result).toHaveLength(3);
    expect(result.every((workspace) => paneCount(workspace) === 1)).toBe(true);
  });

  it('givenWorktreeAgentShouldRecordItsWorktreeContainer', () => {
    const agents = live(['ag-a'], [['wt-1', ['ag-c']]]);
    const result = reconcileWorkspaces([], agents);

    const byAgents = (ids: string[]) =>
      result.find((workspace) => workspaceAgentIds(workspace).join() === ids.join());
    expect(byAgents(['ag-c'])?.container).toEqual(worktreeContainer('wt-1'));
    expect(byAgents(['ag-a'])?.container).toEqual(PROJECT_ROOT);
  });

  // MARK: - Restart

  it('givenSavedMultiPaneWorkspaceShouldStayOneRowAcrossRestart', () => {
    const original = makeGroupedWorkspace();
    const agents = live(['ag-a', 'ag-b', 'ag-c']);
    const restored = reconcileWorkspaces(restart([original]), agents);

    expectInvariant(restored, agents);
    expect(restored).toHaveLength(1);
    expect(restored[0] && paneCount(restored[0])).toBe(3);
  });

  it('givenSavedLayoutShouldPreservePaneIdsAcrossRestart', () => {
    const original = makeGroupedWorkspace();
    const restored = restart([original]);

    expect(new Set(restored[0] ? allLeafIds(restored[0].root) : [])).toEqual(
      new Set(allLeafIds(original.root)),
    );
    expect(restored[0]?.focusedLeafId).toBe(original.focusedLeafId);
  });

  it('givenSameInputsReconcileShouldBeDeterministicAndIdempotent', () => {
    const agents = live(['ag-a', 'ag-b', 'ag-c']);
    const once = reconcileWorkspaces([makeGroupedWorkspace()], agents);
    const again = reconcileWorkspaces([makeGroupedWorkspace()], agents);

    expect(once).toEqual(again);
    expect(reconcileWorkspaces(once, agents)).toEqual(once);
  });

  // MARK: - Agents coming and going

  it('givenDeadAgentShouldCollapseItsPaneAndKeepTheWorkspace', () => {
    const agents = live(['ag-a', 'ag-c']);
    const result = reconcileWorkspaces([makeGroupedWorkspace()], agents);

    expectInvariant(result, agents);
    expect(result).toHaveLength(1);
    expect(result[0] && paneCount(result[0])).toBe(2);
  });

  it('givenAllAgentsDeadShouldDropTheWorkspaceEntirely', () => {
    expect(reconcileWorkspaces([makeGroupedWorkspace()], [])).toEqual([]);
  });

  it('givenUnknownAgentShouldAdoptItWithoutDisturbingExistingGroups', () => {
    const agents = live(['ag-a', 'ag-b', 'ag-c', 'ag-new']);
    const result = reconcileWorkspaces([makeGroupedWorkspace()], agents);

    expectInvariant(result, agents);
    expect(result).toHaveLength(2);
    expect(result[0] && paneCount(result[0])).toBe(3);
    expect(result[1] && workspaceAgentIds(result[1])).toEqual(['ag-new']);
  });

  it('givenAgentClaimedByTwoWorkspacesShouldKeepOnlyTheFirst', () => {
    const duplicate = makeWorkspace({
      id: 'ws-duplicate',
      container: PROJECT_ROOT,
      root: leaf(0),
      panes: { 0: { tabs: [{ id: 0, content: agentContent('ag-a') }], activeTabId: 0 } },
      focusedLeafId: 0,
      nextLeafId: 1,
    });
    const agents = live(['ag-a']);

    const result = reconcileWorkspaces(
      [adoptingWorkspace('ag-a', PROJECT_ROOT), duplicate],
      agents,
    );

    expectInvariant(result, agents);
    expect(result).toHaveLength(1);
  });

  it('givenDeliberatelyEmptyPaneShouldSurviveAlongsideAnAgent', () => {
    // An empty pane awaiting a spawn.
    const { workspace } = splitPane(adoptingWorkspace('ag-a', PROJECT_ROOT), 0, 'vertical');
    const agents = live(['ag-a']);

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result[0] && paneCount(result[0])).toBe(2);
  });

  it('givenFocusOnARemovedPaneShouldRefocusASurvivingOne', () => {
    const grouped = makeGroupedWorkspace();
    const location = locationOfAgent(grouped, 'ag-b');
    expect(location).not.toBeNull();
    const workspace = { ...grouped, focusedLeafId: location?.leafId ?? -1 };

    const result = reconcileWorkspaces([workspace], live(['ag-a', 'ag-c']));

    const survivor = result[0];
    expect(survivor).toBeDefined();
    expect(survivor && allLeafIds(survivor.root).includes(survivor.focusedLeafId)).toBe(true);
  });

  // MARK: - Tabs

  it('givenDeadAgentInABackgroundTabShouldDropTheTabAndKeepThePane', () => {
    let workspace = adoptingWorkspace('ag-a', PROJECT_ROOT);
    workspace = newTab(workspace, 0, agentContent('ag-b')).workspace;
    workspace = selectTab(workspace, 0);
    const agents = live(['ag-a']);

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result[0] && paneCount(result[0])).toBe(1);
    expect(result[0]?.panes[0]?.tabs.map((tab) => tab.content)).toEqual([agentContent('ag-a')]);
  });

  it('givenDeadActiveTabShouldActivateItsLeftNeighbour', () => {
    const { workspace } = newTab(adoptingWorkspace('ag-a', PROJECT_ROOT), 0, agentContent('ag-b'));
    const agents = live(['ag-a']);

    const result = reconcileWorkspaces([workspace], agents);

    expect(result[0] && focusedAgentId(result[0])).toBe('ag-a');
  });

  it('givenAgentInTwoTabsShouldKeepOnlyTheFirst', () => {
    const { workspace } = newTab(adoptingWorkspace('ag-a', PROJECT_ROOT), 0, agentContent('ag-a'));
    const agents = live(['ag-a']);

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result[0] && tabCount(result[0])).toBe(1);
  });

  it('givenSavedTabsShouldRestoreStacksAndActiveTabsAcrossRestart', () => {
    let original = makeGroupedWorkspace();
    original = newTab(original, 0, fileContent('/tmp/notes.md')).workspace;
    original = newTab(original, 0, agentContent('ag-d')).workspace;

    const agents = live(['ag-a', 'ag-b', 'ag-c', 'ag-d']);
    const restored = reconcileWorkspaces(restart([original]), agents);

    expectInvariant(restored, agents);
    expect(restored).toEqual([original]);
  });

  it('givenOnlyFileTabsLeftShouldKeepTheWorkspace', () => {
    const { workspace } = newTab(
      adoptingWorkspace('ag-a', worktreeContainer('wt-1')),
      0,
      fileContent('/tmp/notes.md'),
    );

    const result = reconcileWorkspaces([workspace], []);

    expect(result).toHaveLength(1);
    expect(result[0]?.panes[0]?.tabs.map((tab) => tab.content)).toEqual([
      fileContent('/tmp/notes.md'),
    ]);
    expect(result[0]?.container).toEqual(worktreeContainer('wt-1'));
  });

  it('givenOnlyEmptyTabsLeftShouldDropTheWorkspaceUnlessASpawnIsPending', () => {
    const { workspace } = newTab(adoptingWorkspace('ag-a', PROJECT_ROOT), 0);

    expect(reconcileWorkspaces([workspace], [])).toEqual([]);
    expect(reconcileWorkspaces([workspace], [], new Set([workspace.id]))).toHaveLength(1);
  });

  // MARK: - Migration (file format: WorkspacePersistence, WIN-6.3)

  it.todo('givenVersionTwoDocumentShouldMigrateEachLeafToOneTab (WIN-6.3: WorkspacePersistence)');
  it.todo(
    'givenLegacyGridLayoutShouldMigrateIntoOneWorkspaceAndRetireTheFile (WIN-6.3: WorkspacePersistence)',
  );
  it.todo('givenNewerDocumentOnDiskShouldNotOverwriteIt (WIN-6.3: WorkspacePersistence)');
  it.todo('givenActiveWorkspaceShouldRecordItForTheCLI (WIN-6.3: WorkspacePersistence)');
  it.todo('givenMissingLayoutFileShouldLoadEmptyRatherThanFail (WIN-6.3: WorkspacePersistence)');

  // MARK: - Spawn placement

  it('givenAgentSpawnedForAnEmptyTabShouldPlaceItThere', () => {
    const [workspace, tab] = makeSplitWaitingForAgent();
    const agents = [
      liveAgent('ag-a', PROJECT_ROOT),
      liveAgent('ag-new', PROJECT_ROOT, { workspace: workspace.id, surface: tab }),
    ];

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result.map((w) => w.id)).toEqual([workspace.id]);
    expect(result[0] && surfaceById(result[0], tab)?.content).toEqual(agentContent('ag-new'));
  });

  it('givenPlacementTabAlreadyHoldsAnAgentShouldAdoptTheNewAgentSeparately', () => {
    const [workspace] = makeSplitWaitingForAgent();
    const agents = [
      liveAgent('ag-a', PROJECT_ROOT),
      liveAgent('ag-new', PROJECT_ROOT, { workspace: workspace.id, surface: 0 }),
    ];

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result.map((w) => w.id)).toEqual([workspace.id, 'ws-ag-new']);
  });

  it('givenPlacementWorkspaceIsUnknownShouldAdoptTheAgentSeparately', () => {
    const [workspace, tab] = makeSplitWaitingForAgent();
    const agents = [
      liveAgent('ag-a', PROJECT_ROOT),
      liveAgent('ag-new', PROJECT_ROOT, { workspace: 'ws-gone', surface: tab }),
    ];

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result.map((w) => w.id)).toEqual([workspace.id, 'ws-ag-new']);
  });

  it('givenPlacementInAnotherContainerShouldAdoptTheAgentSeparately', () => {
    const [workspace, tab] = makeSplitWaitingForAgent();
    const agents = [
      liveAgent('ag-a', PROJECT_ROOT),
      liveAgent('ag-new', worktreeContainer('wt-1'), { workspace: workspace.id, surface: tab }),
    ];

    const result = reconcileWorkspaces([workspace], agents);

    expectInvariant(result, agents);
    expect(result.find((w) => workspaceAgentIds(w).join() === 'ag-new')?.container).toEqual(
      worktreeContainer('wt-1'),
    );
  });

  it('givenPlacedAgentShouldStayWhereTheLayoutHasItEvenIfItsPlacementDiffers', () => {
    const [workspace, tab] = makeSplitWaitingForAgent();
    const elsewhere = adoptingWorkspace('ag-new', PROJECT_ROOT);
    const agents = [
      liveAgent('ag-a', PROJECT_ROOT),
      liveAgent('ag-new', PROJECT_ROOT, { workspace: workspace.id, surface: tab }),
    ];

    const result = reconcileWorkspaces([workspace, elsewhere], agents);

    expectInvariant(result, agents);
    const same = result.find((w) => w.id === workspace.id);
    expect(same && surfaceById(same, tab)?.content).toEqual({ kind: 'empty' });
    const adopted = result.find((w) => w.id === 'ws-ag-new');
    expect(adopted && workspaceAgentIds(adopted)).toEqual(['ag-new']);
  });

  it('givenEmptyWorkspaceWaitingForItsAgentShouldBeKeptAndFilled', () => {
    const waiting = makeWorkspace({
      id: 'ws-new',
      container: PROJECT_ROOT,
      root: leaf(0),
      focusedLeafId: 0,
      nextLeafId: 1,
    });
    const tab = focusedSurface(waiting)?.id ?? -1;
    const agents = [liveAgent('ag-new', PROJECT_ROOT, { workspace: 'ws-new', surface: tab })];

    const result = reconcileWorkspaces([waiting], agents);

    expectInvariant(result, agents);
    expect(result.map((w) => w.id)).toEqual(['ws-new']);
  });

  it('givenAgentEntryWithPlacementShouldDecodeIt', () => {
    const json = `{"id":"ag-1","name":"a","agentType":"claude","status":"running","startedAt":"2026-01-01T00:00:00Z",
       "placement":{"workspace":"ws-ag-a","surface":2}}`;
    const entry = JSON.parse(json) as Omit<AgentEntry, 'prompt' | 'suspended'>;
    expect(agentModelFromEntry(entry).placement).toEqual({ workspace: 'ws-ag-a', surface: 2 });
  });

  // MARK: - Layout merge

  it('givenEachMacChangedADifferentWorkspaceMergeShouldKeepBothChanges', () => {
    const base = [adoptingWorkspace('ag-a', PROJECT_ROOT), adoptingWorkspace('ag-b', PROJECT_ROOT)];
    const ours = [splitPane(base[0] as Workspace, 0, 'vertical').workspace, base[1] as Workspace];
    const theirs = [
      base[0] as Workspace,
      splitPane(base[1] as Workspace, 0, 'horizontal').workspace,
    ];

    const merged = mergeWorkspaceLayouts(base, ours, theirs);

    expect(merged).toEqual([ours[0], theirs[1]]);
  });

  it('givenBothMacsChangedTheSameWorkspaceMergeShouldKeepOurs', () => {
    const base = [adoptingWorkspace('ag-a', PROJECT_ROOT)];
    const ours = [splitPane(base[0] as Workspace, 0, 'vertical').workspace];
    const theirs = [splitPane(base[0] as Workspace, 0, 'horizontal').workspace];

    // Ours keeps a tab split here for a spawn; the other app then adopts it unchanged.
    expect(mergeWorkspaceLayouts(base, ours, theirs)).toEqual(ours);
    expect(mergeWorkspaceLayouts(theirs, theirs, ours)).toEqual(ours);
  });

  it('givenWorkspacesAddedAndRemovedMergeShouldApplyEachSidesChange', () => {
    const a = adoptingWorkspace('ag-a', PROJECT_ROOT);
    const b = adoptingWorkspace('ag-b', PROJECT_ROOT);
    const mine = adoptingWorkspace('ag-mine', PROJECT_ROOT);
    const theirsNew = adoptingWorkspace('ag-theirs', PROJECT_ROOT);
    // We removed b and added mine; they removed a and added theirsNew.
    const merged = mergeWorkspaceLayouts([a, b], [a, mine], [b, theirsNew]);

    expect(merged.map((w) => w.id)).toEqual([theirsNew.id, mine.id]);
  });

  it('givenWorkspaceChangedOnOneMacAndRemovedOnTheOtherMergeShouldKeepIt', () => {
    const a = adoptingWorkspace('ag-a', PROJECT_ROOT);
    const changed = splitPane(a, 0, 'vertical').workspace;

    expect(mergeWorkspaceLayouts([a], [changed], [])).toEqual([changed]);
    expect(mergeWorkspaceLayouts([a], [], [changed])).toEqual([changed]);
  });

  it('givenWorkspaceClosedHereAndUntouchedThereMergeShouldNotBringItBack', () => {
    // Closed here: its agent is already gone from this app's live list, so the base as this app
    // reconciles it holds only the file tab. The other app's copy is the base document.
    const closed = newTab(
      adoptingWorkspace('ag-x', PROJECT_ROOT),
      0,
      fileContent('/p/README.md'),
    ).workspace;
    const reconciledHere = removeSurfaces(
      closed,
      (surface) => surface.content.kind === 'agent' && surface.content.agentId === 'ag-x',
    ).workspace;

    const merged = mergeWorkspaceLayouts([closed], [], [closed], [reconciledHere]);

    expect(merged).toEqual([]);
  });

  it('givenPointGuardPlacementFromAnotherMacShouldBeRekeyedToThisApp', () => {
    const agent = liveAgent('ag-new', PROJECT_ROOT, {
      workspace: 'ws-point-guard:machine:abc:/srv/app',
      surface: 3,
    });

    const rekeyed = placementRekeyed(agent, '/srv/app');

    expect(rekeyed.placement).toEqual({ workspace: pointGuardId('/srv/app'), surface: 3 });
    expect(placementRekeyed(liveAgent('ag-x', PROJECT_ROOT), '/p').placement).toBeNull();
  });
});
