// Port of apps/purepoint-macos/purepoint-macosTests/WorkspaceRegistryTests.swift (WIN-6.2).
//
// Each title keeps the Swift test function name. The layout file is an in-memory
// `WorkspacePersistence` (WIN-6.3 ports the real one); "another Mac" writing the file is a write to
// it with a new modification time. The two AppState cases test `resolvePendingWorkspaceSelection`,
// the registry half of `AppState.resolvePendingWorkspaceSelection`.

import { describe, expect, it } from 'vitest';
import {
  agentContent,
  fileContent,
  focusedAgentId,
  focusedSurface,
  locationOfAgent,
  paneCount,
  splitPane,
  surfaceById,
  tabCount,
  workspaceAgentIds,
  type Workspace,
} from '../model/workspace';
import { makeAgentModel, type AgentModel } from '../model/workspaceModel';
import {
  ManualScheduler,
  MemoryWorkspacePersistence,
  type MemoryDocument,
} from './workspaceRegistry.fixtures';
import { WorkspaceRegistry, resolvePendingWorkspaceSelection } from './workspaceRegistry';

const root = '/tmp/pp-registry-tests';

function agent(id: string): AgentModel {
  return makeAgentModel({
    id,
    name: id,
    agentType: 'claude',
    status: 'running',
    prompt: '',
    startedAt: '',
  });
}

function newRegistry(persistence = new MemoryWorkspacePersistence()): WorkspaceRegistry {
  return new WorkspaceRegistry({ persistence, scheduler: new ManualScheduler() });
}

function makeRegistry(
  projectRoot: string,
  agents: string[],
  persistence = new MemoryWorkspacePersistence(),
): WorkspaceRegistry {
  const registry = newRegistry(persistence);
  registry.reconcile(projectRoot, agents.map(agent), []);
  return registry;
}

function required<T>(value: T | null | undefined): T {
  expect(value).not.toBeNull();
  expect(value).not.toBeUndefined();
  return value as T;
}

function ws(registry: WorkspaceRegistry, id: string): Workspace {
  return required(registry.workspace(id));
}

function ids(registry: WorkspaceRegistry, projectRoot = root): string[] {
  return registry.workspaces(projectRoot).map((workspace) => workspace.id);
}

function killedAgents(registry: WorkspaceRegistry): string[] {
  const killed: string[] = [];
  registry.onCloseAgent = (_, agentId) => killed.push(agentId);
  return killed;
}

/** Write `workspaces` as another Mac reaching this project over SSH would. */
function writeLayoutFromAnotherMac(
  persistence: MemoryWorkspacePersistence,
  workspaces: Workspace[],
  projectRoot: string,
): void {
  const data = required(persistence.documentData(workspaces, null));
  persistence.writeFile(persistence.filePath(projectRoot), data);
}

describe('WorkspaceRegistryTests', () => {
  // MARK: - Reservations

  it('pendingReservationDoesNotAdoptUnrelatedAgent', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.reserveSurface(root, 'ws-ag-a', 0);

    registry.reconcile(root, [agent('ag-a'), agent('ag-x')], []);

    expect(ids(registry)).toEqual(['ws-ag-a']);
    expect(registry.workspaceIdForAgent('ag-x')).toBeNull();
  });

  it('fulfillingByAgentIdPlacesTheBoundAgentNotTheFirstLiveOne', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.split('ws-ag-a', 0, 'vertical');
    const newTab = required(focusedSurface(ws(registry, 'ws-ag-a'))).id;
    registry.reserveSurface(root, 'ws-ag-a', newTab);

    registry.reconcile(root, [agent('ag-a'), agent('ag-x'), agent('ag-new')], []);
    registry.fulfillReservation(root, 'ws-ag-a', newTab, 'ag-new');

    expect(surfaceById(ws(registry, 'ws-ag-a'), newTab)?.content).toEqual(agentContent('ag-new'));
    expect(registry.workspaceIdForAgent('ag-x')).toBe('ws-ag-x');
    expect(ids(registry)).toEqual(['ws-ag-a', 'ws-ag-x']);
  });

  it('releasedReservationLetDeferredAgentsSurface', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.reserveSurface(root, 'ws-ag-a', 0);
    registry.reconcile(root, [agent('ag-a'), agent('ag-x')], []);

    registry.releaseReservation(root, 'ws-ag-a', 0);

    expect(registry.workspaceIdForAgent('ag-x')).toBe('ws-ag-x');
  });

  // MARK: - setAgent

  it('bindingAnAgentRemovesItsOtherTabClaims', () => {
    const registry = makeRegistry(root, ['ag-a', 'ag-b']);
    registry.split('ws-ag-b', 0, 'vertical');
    const newTab = required(focusedSurface(ws(registry, 'ws-ag-b'))).id;

    registry.setAgent('ag-a', 'ws-ag-b', newTab);

    expect(registry.workspaceIdForAgent('ag-a')).toBe('ws-ag-b');
    expect(registry.workspace('ws-ag-a')).toBeNull();
    expect(workspaceAgentIds(ws(registry, 'ws-ag-b'))).toEqual(['ag-b', 'ag-a']);
  });

  it('bindingAnAgentAlreadyInAnotherTabOfTheSameWorkspaceMovesIt', () => {
    const registry = makeRegistry(root, ['ag-a']);
    const newTab = required(registry.newTab('ws-ag-a', 0));

    registry.setAgent('ag-a', 'ws-ag-a', newTab);

    const workspace = ws(registry, 'ws-ag-a');
    expect(workspaceAgentIds(workspace)).toEqual(['ag-a']);
    expect(surfaceById(workspace, newTab)?.content).toEqual(agentContent('ag-a'));
    expect(tabCount(workspace)).toBe(1);
  });

  // MARK: - Tabs

  it('closingATabKillsOnlyItsAgent', () => {
    const registry = makeRegistry(root, ['ag-a', 'ag-b']);
    registry.setAgent('ag-b', 'ws-ag-a', required(registry.newTab('ws-ag-a', 0)));
    const killed = killedAgents(registry);

    const tab = required(locationOfAgent(ws(registry, 'ws-ag-a'), 'ag-b')).surfaceId;
    registry.closeTab('ws-ag-a', tab);

    expect(killed).toEqual(['ag-b']);
    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-a']);
  });

  it('closingTheLastTabDropsTheWorkspace', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.activeWorkspaceId = 'ws-ag-a';
    const killed = killedAgents(registry);

    registry.closeTab('ws-ag-a', 0);

    expect(killed).toEqual(['ag-a']);
    expect(registry.workspace('ws-ag-a')).toBeNull();
    expect(registry.activeWorkspaceId).toBeNull();
  });

  it('closingTheLastAgentTabKeepsAWorkspaceThatStillHasAFileTab', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.newTab('ws-ag-a', 0, fileContent('/tmp/x.md'));

    registry.closeTab('ws-ag-a', 0);
    registry.reconcile(root, [], []);

    expect(ws(registry, 'ws-ag-a').panes[0]?.tabs.map((tab) => tab.content)).toEqual([
      fileContent('/tmp/x.md'),
    ]);
  });

  it('closingTheLastAgentTabBesideAnEmptyTabDropsTheWorkspaceAtOnce', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.newTab('ws-ag-a', 0);

    registry.closeTab('ws-ag-a', 0);

    expect(registry.workspace('ws-ag-a')).toBeNull();
  });

  it('anEmptyTabWaitingForASpawnKeepsItsWorkspace', () => {
    const registry = makeRegistry(root, ['ag-a']);
    const pending = required(registry.newTab('ws-ag-a', 0));
    registry.reserveSurface(root, 'ws-ag-a', pending);

    registry.closeTab('ws-ag-a', 0);
    registry.reconcile(root, [], []);
    registry.fulfillReservation(root, 'ws-ag-a', pending, 'ag-new');

    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-new']);
  });

  it('closingAWorkspaceKillsItsAgentsAndRemovesFileOnlyWorkspacesToo', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.newTab('ws-ag-a', 0, fileContent(null));
    const killed = killedAgents(registry);

    registry.closeWorkspace('ws-ag-a');

    expect(killed).toEqual(['ag-a']);
    expect(registry.workspace('ws-ag-a')).toBeNull();
  });

  it('fileTabSessionsLiveUntilTheirTabCloses', () => {
    const registry = makeRegistry(root, ['ag-a']);
    const fileTab = required(registry.newTab('ws-ag-a', 0, fileContent(null)));
    const session = registry.fileTabs.session('ws-ag-a', fileTab, null);

    registry.selectTab('ws-ag-a', 0);
    expect(registry.fileTabs.existingSession('ws-ag-a', fileTab)).toBe(session);

    registry.closeTab('ws-ag-a', fileTab);
    expect(registry.fileTabs.existingSession('ws-ag-a', fileTab)).toBeNull();
  });

  it('activatingAWorkspaceRecordsItInTheSavedLayout', () => {
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(root, ['ag-a', 'ag-b'], persistence);
    registry.activeWorkspaceId = 'ws-ag-b';
    registry.saveAll();

    const data = required(persistence.readFile(persistence.filePath(root)));
    const document = JSON.parse(data) as MemoryDocument;
    expect(document.activeWorkspaceId).toBe('ws-ag-b');
  });

  it('closingAPaneKillsEveryAgentInItsTabs', () => {
    const registry = makeRegistry(root, ['ag-a', 'ag-b', 'ag-c']);
    registry.split('ws-ag-a', 0, 'vertical');
    const split = ws(registry, 'ws-ag-a');
    const newLeaf = split.focusedLeafId;
    registry.setAgent('ag-b', 'ws-ag-a', required(focusedSurface(split)).id);
    registry.setAgent('ag-c', 'ws-ag-a', required(registry.newTab('ws-ag-a', newLeaf)));
    const killed = killedAgents(registry);

    registry.closePane('ws-ag-a', newLeaf);

    expect(new Set(killed)).toEqual(new Set(['ag-b', 'ag-c']));
    expect(paneCount(ws(registry, 'ws-ag-a'))).toBe(1);
  });

  // MARK: - Layout written by another Mac

  it('givenAnotherMacSplitAPaneShouldPlaceItsNewAgentInThatPane', () => {
    const project = root + '-split';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a'], persistence);
    const theirs = splitPane(ws(registry, 'ws-ag-a'), 0, 'vertical').workspace;
    const tab = required(focusedSurface(theirs)).id;
    writeLayoutFromAnotherMac(persistence, [theirs], project);

    const spawned = { ...agent('ag-new'), placement: { workspace: 'ws-ag-a', surface: tab } };
    registry.reconcile(project, [agent('ag-a'), spawned], []);

    expect(ids(registry, project)).toEqual(['ws-ag-a']);
    expect(surfaceById(ws(registry, 'ws-ag-a'), tab)?.content).toEqual(agentContent('ag-new'));
  });

  it('givenAnotherMacWroteTheLayoutASaveShouldAdoptItNotOverwriteIt', () => {
    const project = root + '-save';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a'], persistence);
    const theirs = splitPane(ws(registry, 'ws-ag-a'), 0, 'vertical').workspace;
    writeLayoutFromAnotherMac(persistence, [theirs], project);
    const written = persistence.readFile(persistence.filePath(project));

    registry.saveAll();

    expect(paneCount(ws(registry, 'ws-ag-a'))).toBe(2);
    expect(persistence.readFile(persistence.filePath(project))).toBe(written);
  });

  it('givenLayoutFileDeletedASaveShouldWriteItAgain', () => {
    const project = root + '-deleted';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a'], persistence);
    registry.saveAll();
    persistence.removeFile(persistence.filePath(project));
    registry.split('ws-ag-a', 0, 'vertical');

    registry.saveAll();

    const restored = persistence.loadForRegistry(project);
    const restoredA = required(restored.find((workspace) => workspace.id === 'ws-ag-a'));
    expect(paneCount(restoredA)).toBe(2);
  });

  it('givenAnotherMacChangedADifferentWorkspaceASaveShouldKeepBothChanges', () => {
    const project = root + '-merge';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a', 'ag-b'], persistence);
    registry.saveAll();
    const theirs = registry
      .workspaces(project)
      .map((workspace) =>
        workspace.id === 'ws-ag-b' ? splitPane(workspace, 0, 'vertical').workspace : workspace,
      );
    writeLayoutFromAnotherMac(persistence, theirs, project);
    registry.split('ws-ag-a', 0, 'vertical');

    registry.saveAll(); // conflicts, merges
    registry.saveAll(); // writes the merged layout

    expect(paneCount(ws(registry, 'ws-ag-a'))).toBe(2);
    expect(paneCount(ws(registry, 'ws-ag-b'))).toBe(2);
    expect(persistence.loadForRegistry(project).map(paneCount)).toEqual([2, 2]);
  });

  it('givenBothMacsChangedTheSameWorkspaceOneSaveShouldKeepThisMacsNewTabOnDisk', () => {
    const project = root + '-same';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a'], persistence);
    registry.saveAll();
    const theirs = registry
      .workspaces(project)
      .map((workspace, index) =>
        index === 0 ? splitPane(workspace, 0, 'horizontal').workspace : workspace,
      );
    writeLayoutFromAnotherMac(persistence, theirs, project);
    registry.split('ws-ag-a', 0, 'vertical');
    const tab = required(focusedSurface(ws(registry, 'ws-ag-a'))).id;

    registry.saveNow(project); // as a spawn does: conflicts, merges, writes

    expect(surfaceById(ws(registry, 'ws-ag-a'), tab)?.content).toEqual({ kind: 'empty' });
    const onDisk = required(persistence.loadForRegistry(project)[0]);
    expect(surfaceById(onDisk, tab)?.content).toEqual({ kind: 'empty' });
    expect(registry.hasPendingSave(project)).toBe(false);
  });

  it('givenAnotherMacsLayoutNamesAnAgentThisMacHasNotSeenYetItsTabShouldWaitForTheManifest', () => {
    const project = root + '-unseen';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a'], persistence);
    registry.saveAll();
    const theirs = splitPane(
      ws(registry, 'ws-ag-a'),
      0,
      'vertical',
      agentContent('ag-new'),
    ).workspace;
    writeLayoutFromAnotherMac(persistence, [theirs], project);

    registry.adoptLocalLayoutIfChanged(project); // the file watcher, before the manifest

    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-a', 'ag-new']);
    registry.reconcile(project, [agent('ag-a'), agent('ag-new')], []);
    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-a', 'ag-new']);
    // A manifest without it means the agent exited: its tab goes.
    registry.reconcile(project, [agent('ag-a')], []);
    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-a']);
  });

  it('givenAnotherMacSplitsAgainBeforeTheManifestCatchesUpItsNewTabShouldSurvive', () => {
    const project = root + '-unseen-twice';
    const persistence = new MemoryWorkspacePersistence();
    const registry = makeRegistry(project, ['ag-a'], persistence);
    registry.saveAll();
    let theirs = splitPane(ws(registry, 'ws-ag-a'), 0, 'vertical', agentContent('ag-z')).workspace;
    writeLayoutFromAnotherMac(persistence, [theirs], project);
    registry.adoptLocalLayoutIfChanged(project);
    // Before this Mac's manifest lists ag-z, the other Mac splits again for its next spawn.
    theirs = splitPane(theirs, theirs.focusedLeafId, 'horizontal').workspace;
    const tab = required(focusedSurface(theirs)).id;
    writeLayoutFromAnotherMac(persistence, [theirs], project);

    registry.adoptLocalLayoutIfChanged(project);

    expect(surfaceById(ws(registry, 'ws-ag-a'), tab)?.content).toEqual({ kind: 'empty' });
    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-a', 'ag-z']);
    expect(registry.hasPendingSave(project)).toBe(false);
  });

  it('givenAgentSpawnedIntoPointGuardOnAnotherMacShouldLandInThisMacsPointGuard', () => {
    const project = root + '-pg';
    const registry = makeRegistry(project, []);
    const pointGuard = registry.ensurePointGuardWorkspace(project);
    registry.split(pointGuard, 0, 'vertical');
    const tab = required(focusedSurface(ws(registry, pointGuard))).id;

    const spawned = {
      ...agent('ag-new'),
      placement: { workspace: 'ws-point-guard:machine:other:' + project, surface: tab },
    };
    registry.reconcile(project, [spawned], []);

    expect(surfaceById(ws(registry, pointGuard), tab)?.content).toEqual(agentContent('ag-new'));
    expect(registry.workspaceIdForAgent('ag-new')).toBe(pointGuard);
  });

  it('givenOwnSavesShouldNotBeMistakenForAnotherMacs', () => {
    const project = root + '-own';
    const registry = makeRegistry(project, ['ag-a']);
    registry.saveAll();
    registry.split('ws-ag-a', 0, 'vertical');

    registry.adoptLocalLayoutIfChanged(project);

    expect(paneCount(ws(registry, 'ws-ag-a'))).toBe(2);
  });

  // MARK: - Remote commands

  it('remoteSplitAndCloseIgnoreUnknownLeaves', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.activeWorkspaceId = 'ws-ag-a';
    const before = registry.workspace('ws-ag-a');

    registry.handleRemoteCommand({ action: 'split', leaf_id: 99, axis: 'v' }, root);
    registry.handleRemoteCommand({ action: 'close', leaf_id: 99 }, root);

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('remoteSplitWithoutLeafUsesFocusedLeaf', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.activeWorkspaceId = 'ws-ag-a';

    registry.handleRemoteCommand({ action: 'split', leaf_id: null, axis: 'v' }, root);

    expect(paneCount(ws(registry, 'ws-ag-a'))).toBe(2);
  });

  it('remoteNewTabWithAgentOpensItBesideTheActiveTab', () => {
    const registry = makeRegistry(root, ['ag-a', 'ag-b']);
    registry.activeWorkspaceId = 'ws-ag-a';

    registry.handleRemoteCommand({ action: 'new_tab', leaf_id: null, agent_id: 'ag-b' }, root);

    const workspace = ws(registry, 'ws-ag-a');
    expect(workspace.panes[0]?.tabs.map((tab) => tab.content)).toEqual([
      agentContent('ag-a'),
      agentContent('ag-b'),
    ]);
    expect(focusedAgentId(workspace)).toBe('ag-b');
    expect(registry.workspace('ws-ag-b')).toBeNull();
  });

  it('remoteSelectTabUsesOneBasedPositionsAndCycles', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.activeWorkspaceId = 'ws-ag-a';
    registry.newTab('ws-ag-a', 0);

    registry.handleRemoteCommand({ action: 'select_tab', leaf_id: null, index: 1 }, root);
    expect(focusedAgentId(ws(registry, 'ws-ag-a'))).toBe('ag-a');

    registry.handleRemoteCommand({ action: 'select_tab', leaf_id: null, direction: 'prev' }, root);
    expect(focusedSurface(ws(registry, 'ws-ag-a'))?.content).toEqual({ kind: 'empty' });
  });

  it('remoteSetAgentWithoutLeafTargetsTheFocusedPanesActiveTab', () => {
    const registry = makeRegistry(root, ['ag-a', 'ag-b']);
    registry.activeWorkspaceId = 'ws-ag-a';
    registry.newTab('ws-ag-a', 0);

    registry.handleRemoteCommand({ action: 'set_agent', leaf_id: null, agent_id: 'ag-b' }, root);

    expect(workspaceAgentIds(ws(registry, 'ws-ag-a'))).toEqual(['ag-a', 'ag-b']);
  });

  it('remoteCommandNamingAWorkspaceTargetsItNotTheActiveOne', () => {
    const registry = makeRegistry(root, ['ag-a', 'ag-b']);
    registry.activeWorkspaceId = 'ws-ag-a';
    const killed = killedAgents(registry);

    registry.handleRemoteCommand(
      { action: 'close_tab', leaf_id: null, tab_id: 0, workspace_id: 'ws-ag-b' },
      root,
    );

    expect(killed).toEqual(['ag-b']);
    expect(registry.workspace('ws-ag-a')).not.toBeNull();
  });

  it('remoteCommandNamingAWorkspaceOfAnotherProjectIsIgnored', () => {
    const registry = makeRegistry(root, ['ag-a']);
    registry.handleRemoteCommand(
      { action: 'close_tab', leaf_id: null, tab_id: 0, workspace_id: 'ws-ag-a' },
      root + '-other',
    );
    expect(registry.workspace('ws-ag-a')).not.toBeNull();
  });

  // MARK: - Pending workspace restore

  it('pendingWorkspaceSurvivesUntilItsProjectReconciles', () => {
    const registry = newRegistry();
    const pending = { pendingId: 'ws-ag-a', awaitingLocalProjects: false, localProjectIds: [root] };

    const before = resolvePendingWorkspaceSelection(registry, pending);
    expect(before.selectedId).toBeNull();
    expect(before.pendingId).toBe('ws-ag-a');

    registry.reconcile(root, [agent('ag-a')], []);

    const after = resolvePendingWorkspaceSelection(registry, pending);
    expect(after.selectedId).toBe('ws-ag-a');
    expect(after.pendingId).toBeNull();
    expect(after.activeProjectRoot).toBe(root);
  });

  it('pendingWorkspaceClearsOnceAllProjectsReconciledWithoutIt', () => {
    const registry = newRegistry();
    const pending = { pendingId: 'ws-gone', awaitingLocalProjects: false, localProjectIds: [root] };

    registry.reconcile(root, [agent('ag-a')], []);

    const result = resolvePendingWorkspaceSelection(registry, pending);
    expect(result.selectedId).toBeNull();
    expect(result.pendingId).toBeNull();
  });
});

describe('WorkspaceRegistry store', () => {
  it('publishes the canonical workspaces through its Zustand store', () => {
    const registry = newRegistry();
    const seen: string[][] = [];
    const unsubscribe = registry.store.subscribe((state) =>
      seen.push((state.workspacesByProject[root] ?? []).map((workspace) => workspace.id)),
    );

    registry.reconcile(root, [agent('ag-a')], []);
    registry.setFocus('ws-ag-a', 0); // already focused: no publish
    unsubscribe();

    expect(seen).toEqual([[], ['ws-ag-a']]);
  });

  it('debounces local saves and writes once when the timer fires', () => {
    const persistence = new MemoryWorkspacePersistence();
    const scheduler = new ManualScheduler();
    const registry = new WorkspaceRegistry({ persistence, scheduler });
    registry.reconcile(root, [agent('ag-a')], []);
    registry.split('ws-ag-a', 0, 'vertical');
    expect(registry.hasPendingSave(root)).toBe(true);
    expect(persistence.readFile(persistence.filePath(root))).toBeNull();

    scheduler.flush();

    expect(registry.hasPendingSave(root)).toBe(false);
    expect(persistence.loadForRegistry(root).map(paneCount)).toEqual([2]);
  });

  it('reports a layout that cannot be restored and keeps the manifest for a retry', () => {
    let fail = true;
    const persistence = new MemoryWorkspacePersistence();
    const registry = new WorkspaceRegistry({
      persistence,
      scheduler: new ManualScheduler(),
      loadWorkspaces: (projectRoot) => {
        if (fail) throw new Error('migration failed');
        return persistence.loadForRegistry(projectRoot);
      },
    });

    registry.reconcile(root, [agent('ag-a')], []);
    expect(registry.layoutErrorsByProject[root]).toBe(
      "Could not restore this project's layout: migration failed",
    );
    expect(registry.workspaces(root)).toEqual([]);

    fail = false;
    expect(registry.ensurePointGuardWorkspace(root)).toBe('ws-point-guard:' + root);
    expect(registry.layoutErrorsByProject[root]).toBeUndefined();
    expect(ids(registry)).toEqual(['ws-ag-a', 'ws-point-guard:' + root]);
  });
});
