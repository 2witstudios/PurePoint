import { describe, expect, it } from 'vitest';
import { adoptingWorkspace, PROJECT_ROOT, worktreeContainer, type Workspace } from './workspace.ts';
import { makeAgent, makeMachine, makeProject } from './sidebarFixtures.ts';
import { makeWorktreeModel } from './workspaceModel.ts';
import { machineKey, selection, type ProjectSnapshot } from './sidebarItem.ts';
import {
  buildSidebarTree,
  initializeActiveProjectIfNeeded,
  isSelectable,
  isSettled,
  selectionFor,
  sidebarAncestors,
  sidebarExpansion,
  targetId,
  updateActiveProject,
  worktreeNode,
  type ActiveProjectState,
  type WorkspacesByProject,
} from './sidebarOutline.ts';

// The sidebar's shared model: the tree, expansion and selection the outline draws.
// (Port of SidebarOutlineModelTests.swift.)
describe('SidebarOutlineModelTests', () => {
  // MARK: - Tree

  it('givenMachinesShouldAppendAMachinesSectionWithEachMachinesProjectsUnderIt', () => {
    const machine = makeMachine('studio', { sshHost: 'me@studio' });
    const nodes = buildSidebarTree([], [machine], {});

    expect(nodes.map((node) => node.id)).toEqual([
      'section:MACHINES',
      'machine-node:' + machine.id.toLowerCase(),
    ]);
    expect(nodes[1]?.rowKey).toBe(machineKey(machine.id));
  });

  it('givenNoMachinesShouldShowNoMachinesSection', () => {
    const project = makeProject('/tmp/p', { worktreeCount: 1 });
    const nodes = buildSidebarTree([project], [], {});
    expect(nodes.map((node) => node.id)).toEqual([project.projectId]);
    expect(nodes[0]?.children.map((node) => node.id)).toEqual(['wt-0']);
  });

  it('givenAClosedRowShouldNotOpenItsChildrenButShouldKeepTheirClosedState', () => {
    const project = makeProject('/tmp/expansion', { worktreeCount: 2 });
    const nodes = buildSidebarTree([project], [], {});
    const root = nodes[0];
    if (root === undefined) throw new Error('no project row');
    const projectKey = root.rowKey;
    const closedChild = root.children[0]?.rowKey ?? '';

    const closedParent = sidebarExpansion(nodes, new Set([projectKey, closedChild]));
    expect(closedParent.open).toEqual([]);
    expect(closedParent.closed).toEqual(new Set([projectKey, closedChild]));

    const openParent = sidebarExpansion(nodes, new Set([closedChild, 'gone']));
    expect(openParent.open.map((node) => node.rowKey)).toEqual([
      projectKey,
      root.children[1]?.rowKey,
    ]);
    expect(openParent.closed).toEqual(new Set([closedChild]));
  });

  it('givenAProjectThatHasNotLoadedShouldNotCountAsSettled', () => {
    const project = makeProject('/tmp/settled');
    const nodes = (p: ProjectSnapshot) => buildSidebarTree([p], [], {});
    expect(isSettled(nodes(project))).toBe(false);
    // `markWorktreesLoadedForTesting()`: the snapshot has been read.
    expect(isSettled(nodes({ ...project, hasLoadedWorktrees: true }))).toBe(true);
  });

  it('givenARowShouldReportTheSelectionTheMacReports', () => {
    const project = makeProject('/tmp/select', { worktreeCount: 1 });
    const workspace = adoptingWorkspace('agent-0', worktreeContainer('wt-0'));
    const machine = makeMachine('m');
    const nodes = buildSidebarTree([project], [machine], { [project.projectId]: [workspace] });
    const [projectRow, section, machineRow] = nodes;
    const worktree = projectRow?.children[0];
    if (projectRow === undefined || section === undefined || machineRow === undefined) {
      throw new Error('missing rows');
    }
    if (worktree === undefined) throw new Error('no worktree row');
    const workspaceRow = worktree.children[0];
    if (workspaceRow === undefined) throw new Error('no workspace row');

    expect(selectionFor(projectRow)).toEqual(selection.project(project.projectId));
    expect(selectionFor(worktree)).toEqual(selection.worktree('wt-0'));
    expect(selectionFor(workspaceRow)).toEqual(selection.workspace(workspace.id));
    expect(selectionFor(section)).toBeNull();
    expect(selectionFor(machineRow)).toBeNull();
    expect(isSelectable(section)).toBe(false);
    expect(isSelectable(machineRow)).toBe(true);
    expect(targetId(selection.nav('agents'))).toBeNull();
    expect(targetId(selection.channel(project.projectId))).toBe(project.projectId);
    expect(
      sidebarAncestors((node) => node.id === workspace.id, nodes).map((node) => node.id),
    ).toEqual([project.projectId, 'wt-0']);
    expect(worktreeNode('wt-0', nodes)).toBe(worktree);
  });
});

// MARK: - updateActiveProject routing (port of ActiveProjectRoutingTests.swift)

function localProject(
  root: string,
  agentIds: readonly string[] = [],
  worktreeId: string | null = null,
  worktreeAgentIds: readonly string[] = [],
): ProjectSnapshot {
  const base = makeProject(root, { machineId: null, rootAgentIds: agentIds });
  if (worktreeId === null) return base;
  const worktree = makeWorktreeModel({
    id: worktreeId,
    name: 'wt',
    path: '/tmp/wt',
    branch: 'main',
    status: 'active',
    agents: worktreeAgentIds.map((id) => makeAgent(id)),
  });
  return { ...base, worktrees: [worktree] };
}

/**
 * The registry's workspaces: one single-pane workspace per agent, mirroring what reconcile
 * produces for a project whose agents have never been grouped.
 */
function makeRegistry(projects: readonly ProjectSnapshot[]): WorkspacesByProject {
  const byProject: Record<string, Workspace[]> = {};
  for (const project of projects) {
    byProject[project.projectId] = [
      ...project.rootAgents.map((agent) => adoptingWorkspace(agent.id, PROJECT_ROOT)),
      ...project.worktrees.flatMap((worktree) =>
        worktree.agents.map((agent) => adoptingWorkspace(agent.id, worktreeContainer(worktree.id))),
      ),
    ];
  }
  return byProject;
}

/** `registry.workspaceId(forAgent:)`. */
function workspaceId(registry: WorkspacesByProject, agentId: string): string {
  for (const workspaces of Object.values(registry)) {
    const found = workspaces?.find((workspace) => workspace.id === `ws-${agentId}`);
    if (found !== undefined) return found.id;
  }
  return '';
}

const fresh: ActiveProjectState = { activeProjectRoot: null, activeSidebarSelection: null };
const at = (root: string): ActiveProjectState => ({ ...fresh, activeProjectRoot: root });

describe('ActiveProjectRoutingTests', () => {
  it('workspaceSelectionSetsActiveProjectRoot', () => {
    const projects = [localProject('/a', ['a1']), localProject('/b', ['b1'])];
    const registry = makeRegistry(projects);

    const state = updateActiveProject(fresh, selection.workspace(workspaceId(registry, 'b1')), {
      projects,
      workspacesByProject: registry,
    });

    expect(state.activeProjectRoot).toBe('/b');
  });

  it('worktreeSelectionSetsActiveProjectRoot', () => {
    const projects = [localProject('/a'), localProject('/b', [], 'wt-1')];

    const state = updateActiveProject(fresh, selection.worktree('wt-1'), {
      projects,
      workspacesByProject: {},
    });

    expect(state.activeProjectRoot).toBe('/b');
  });

  it('projectSelectionSetsActiveProjectRoot', () => {
    const projects = [localProject('/a'), localProject('/b')];

    const state = updateActiveProject(fresh, selection.project('/b'), {
      projects,
      workspacesByProject: {},
    });

    expect(state.activeProjectRoot).toBe('/b');
  });

  it('navSelectionKeepsExistingActiveProject', () => {
    const state = updateActiveProject(at('/a'), selection.nav('dashboard'), {
      projects: [],
      workspacesByProject: {},
    });

    expect(state.activeProjectRoot).toBe('/a');
  });

  it('nilSelectionKeepsExistingActiveProject', () => {
    const state = updateActiveProject(at('/a'), null, { projects: [], workspacesByProject: {} });

    expect(state.activeProjectRoot).toBe('/a');
  });

  it('updateAlsoSetsActiveSidebarSelection', () => {
    const projects = [localProject('/a', ['a1'])];
    const registry = makeRegistry(projects);
    const wsId = workspaceId(registry, 'a1');

    const state = updateActiveProject(fresh, selection.workspace(wsId), {
      projects,
      workspacesByProject: registry,
    });

    expect(state.activeSidebarSelection).toEqual(selection.workspace(wsId));
  });

  it('unknownWorktreeIdPreservesActiveProjectRoot', () => {
    const projects = [localProject('/a'), localProject('/b')];

    const state = updateActiveProject(at('/a'), selection.worktree('nonexistent'), {
      projects,
      workspacesByProject: {},
    });

    expect(state.activeProjectRoot).toBe('/a');
  });

  it('unknownWorkspaceIdPreservesActiveProjectRoot', () => {
    const projects = [localProject('/a'), localProject('/b')];
    const registry = makeRegistry(projects);

    const state = updateActiveProject(at('/a'), selection.workspace('ws-nonexistent'), {
      projects,
      workspacesByProject: registry,
    });

    expect(state.activeProjectRoot).toBe('/a');
  });

  it('worktreeAgentWorkspaceRoutesThroughItsProject', () => {
    const projects = [localProject('/a'), localProject('/b', [], 'wt-1', ['wt-agent'])];
    const registry = makeRegistry(projects);

    const state = updateActiveProject(
      fresh,
      selection.workspace(workspaceId(registry, 'wt-agent')),
      { projects, workspacesByProject: registry },
    );

    expect(state.activeProjectRoot).toBe('/b');
  });
});

describe('OpenProjectActiveRootTests', () => {
  it('firstProjectSetsActiveProjectRoot', () => {
    const state = fresh;
    expect(state.activeProjectRoot).toBeNull();

    expect(initializeActiveProjectIfNeeded(state.activeProjectRoot, '/first')).toBe('/first');
  });

  it('secondProjectDoesNotOverrideActiveProjectRoot', () => {
    expect(initializeActiveProjectIfNeeded('/first', '/second')).toBe('/first');
  });
});
