// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineModel.swift (WIN-6.5),
// plus the active-project routing from `AppState.updateActiveProject` that its selection drives.
//
// The sidebar outline's platform-free model: what the tree holds, which rows are open and what a
// row's selection means. The view only draws it. Menus live in `sidebarMenu.ts`.

import type { AgentStatus } from '../protocol';
import {
  type Workspace,
  isPointGuard,
  paneCount,
  tabCount,
  primaryAgentId,
  workspaceAgentIds,
  workspaceSurfaces,
  containersEqual,
  worktreeContainer,
} from './workspace.ts';
import { type AgentModel, agentDisplayName, isAgentAlive } from './workspaceModel.ts';
import { lastPathComponent } from './projectKey.ts';
import {
  type MachineConnectionState,
  type MachineSnapshot,
  type ProjectSnapshot,
  type SidebarNode,
  type SidebarSelection,
  isExpandable,
  machineNode,
  machineReportsCompleteProjects,
  projectAgent,
  projectIsRemote,
  projectNode,
  sectionHeaderNode,
} from './sidebarItem.ts';

/** Workspaces by project key id (`registry.workspacesByProject`). */
export type WorkspacesByProject = Readonly<Record<string, readonly Workspace[] | undefined>>;

// MARK: - Render state

/**
 * A row's agent signal, before presentation: the workspace's first agent and whether its project
 * can be reached. The row presents it through `agentSignalPresentation` (Swift
 * `AgentModel.signalPresentation(available:)`) when it draws, since freshness depends on the time.
 */
export interface WorkspaceSignal {
  readonly agent: AgentModel;
  readonly available: boolean;
}

export interface WorkspaceRenderState {
  readonly id: string;
  readonly title: string;
  readonly paneCount: number;
  readonly tabCount: number;
  /** `null` for a workspace holding only file tabs — it has no agent to report on. */
  readonly status: AgentStatus | null;
  readonly signal: WorkspaceSignal | null;
}

export interface WorktreeRenderState {
  readonly id: string;
  readonly branch: string;
  readonly workspaces: readonly WorkspaceRenderState[];
}

export interface ProjectRenderState {
  readonly channelUnread: number;
  readonly isUnavailable: boolean;
  readonly layoutError: string | null;
  readonly projectRoot: string;
  readonly rootWorkspaces: readonly WorkspaceRenderState[];
  readonly worktrees: readonly WorktreeRenderState[];
  /** Reaches the render state so a load that adds no rows still lets the tree be pruned. */
  readonly hasLoadedWorktrees: boolean;
}

export interface MachineRenderState {
  readonly id: string;
  readonly name: string;
  readonly state: MachineConnectionState;
  readonly statusText: string;
  readonly actionError: string | null;
  readonly isStartingDaemon: boolean;
  readonly hiddenRevision: number;
  readonly projects: readonly ProjectRenderState[];
  readonly hasLoadedProjects: boolean;
}

/**
 * Everything the rows show, compared (`structurallyEqual`) before a rebuild so an unchanged
 * refresh costs nothing.
 */
export interface SidebarRenderState {
  readonly activeWorkspaceId: string | null;
  readonly projects: readonly ProjectRenderState[];
  readonly machines: readonly MachineRenderState[];
}

/**
 * How a workspace presents itself as one row: named after its first pane's agent, with the pane
 * count when it holds more than one.
 */
export function workspaceRenderState(
  workspace: Workspace,
  project: ProjectSnapshot,
): WorkspaceRenderState {
  const agents = workspaceAgentIds(workspace).flatMap((id) => {
    const agent = projectAgent(project, id);
    return agent === null ? [] : [agent];
  });
  let firstFile: string | null = null;
  for (const entry of workspaceSurfaces(workspace)) {
    const content = entry.surface.content;
    if (content.kind === 'file') {
      firstFile = content.path === null ? 'Files' : lastPathComponent(content.path);
      break;
    }
  }
  const first = agents[0];
  const title =
    (first === undefined ? null : agentDisplayName(first)) ??
    primaryAgentId(workspace) ??
    firstFile ??
    workspace.id;
  const status: AgentStatus | null =
    first === undefined
      ? null
      : agents.some((agent) => !isAgentAlive(agent))
        ? 'broken'
        : first.status;
  return {
    id: workspace.id,
    title,
    paneCount: paneCount(workspace),
    tabCount: tabCount(workspace),
    status,
    signal: first === undefined ? null : { agent: first, available: !project.isUnavailable },
  };
}

function projectRenderState(
  project: ProjectSnapshot,
  workspaces: readonly Workspace[],
): ProjectRenderState {
  return {
    channelUnread: project.channelUnread,
    isUnavailable: project.isUnavailable,
    layoutError: projectIsRemote(project) ? project.layoutError : null,
    projectRoot: project.projectId,
    rootWorkspaces: workspaces
      .filter((workspace) => workspace.container.kind === 'projectRoot' && !isPointGuard(workspace))
      .map((workspace) => workspaceRenderState(workspace, project)),
    worktrees: project.worktrees.map((worktree) => {
      const container = worktreeContainer(worktree.id);
      return {
        id: worktree.id,
        branch: worktree.branch,
        workspaces: workspaces
          .filter((workspace) => containersEqual(workspace.container, container))
          .map((workspace) => workspaceRenderState(workspace, project)),
      };
    }),
    hasLoadedWorktrees: project.hasLoadedWorktrees,
  };
}

export function makeSidebarRenderState(
  projects: readonly ProjectSnapshot[],
  machines: readonly MachineSnapshot[],
  workspacesByProject: WorkspacesByProject,
  activeWorkspaceId: string | null,
): SidebarRenderState {
  const render = (project: ProjectSnapshot) =>
    projectRenderState(project, workspacesByProject[project.projectId] ?? []);
  return {
    activeWorkspaceId,
    projects: projects.map(render),
    machines: machines.map((machine) => ({
      id: machine.id,
      name: machine.name,
      state: machine.state,
      statusText: machine.statusText,
      actionError: machine.actionError,
      isStartingDaemon: machine.isStartingDaemon,
      hiddenRevision: machine.hiddenCountRevision,
      projects: machine.projects.map(render),
      hasLoadedProjects: machine.hasLoadedProjects,
    })),
  };
}

// MARK: - Tree

/** The local projects, then a MACHINES section with each machine's projects nested under it. */
export function buildSidebarTree(
  projects: readonly ProjectSnapshot[],
  machines: readonly MachineSnapshot[],
  workspacesByProject: WorkspacesByProject,
): SidebarNode[] {
  const nodes = projects.map((project) =>
    projectNode(project, workspacesByProject[project.projectId] ?? []),
  );
  if (machines.length > 0) {
    nodes.push(sectionHeaderNode('MACHINES'));
    for (const machine of machines) {
      const key = machineNode(machine, []).rowKey;
      nodes.push(
        machineNode(
          machine,
          machine.projects.map((project) =>
            projectNode(project, workspacesByProject[project.projectId] ?? [], key),
          ),
        ),
      );
    }
  }
  return nodes;
}

export interface ProjectDrop {
  readonly project: ProjectSnapshot;
  readonly index: number;
}

/**
 * Where a dragged project lands when dropped at `childIndex` under `parent` (null for the top
 * level): the project and its index among its siblings once it's moved, or null when the drop is
 * outside its own section or would leave it where it is. A project only moves among its own
 * section: the local projects, or one machine's.
 */
export function projectDrop(
  projectId: string,
  parent: SidebarNode | null,
  childIndex: number,
  roots: readonly SidebarNode[],
): ProjectDrop | null {
  let siblings: readonly SidebarNode[];
  if (parent !== null) {
    if (parent.kind !== 'machine') return null;
    siblings = parent.children;
  } else {
    // Local projects lead the top level, ahead of the MACHINES section.
    const end = roots.findIndex((node) => node.kind !== 'project');
    siblings = end === -1 ? roots : roots.slice(0, end);
  }
  const projects = siblings.flatMap((node) => (node.kind === 'project' ? [node.project] : []));
  const from = projects.findIndex((project) => project.projectId === projectId);
  if (childIndex < 0 || childIndex > projects.length || from === -1) return null;
  // `childIndex` counts the dragged row, which leaves its old place first.
  const to = childIndex > from ? childIndex - 1 : childIndex;
  const project = projects[from];
  return to === from || project === undefined ? null : { project, index: to };
}

/**
 * Whether every row in the tree holds all the rows it will ever hold. False while a project has
 * yet to read its snapshot or a machine has yet to answer its inventory — an empty row in that
 * window is not evidence that nothing is under it, so nothing may be pruned on that basis.
 */
export function isSettled(nodes: readonly SidebarNode[]): boolean {
  return nodes.every((node) => {
    switch (node.kind) {
      case 'project':
        return node.project.hasLoadedWorktrees && isSettled(node.children);
      case 'machine':
        return machineReportsCompleteProjects(node.machine) && isSettled(node.children);
      case 'worktree':
      case 'workspace':
      case 'sectionHeader':
        return true;
    }
  });
}

export interface SidebarExpansion {
  /** Rows to expand, parents before children. */
  readonly open: SidebarNode[];
  /** Every recorded-closed row still in the tree. */
  readonly closed: Set<string>;
}

/**
 * Which rows to open and which are closed, given the rows the user closed. A row inside a closed
 * row has no row of its own, so it is not opened, but it is still closed — that is what lets it
 * come back as it was when its parent opens again.
 */
export function sidebarExpansion(
  nodes: readonly SidebarNode[],
  closedRowKeys: ReadonlySet<string>,
  parentOpen = true,
): SidebarExpansion {
  const open: SidebarNode[] = [];
  const closed = new Set<string>();
  for (const node of nodes) {
    if (!isExpandable(node)) continue;
    const isClosed = closedRowKeys.has(node.rowKey);
    if (isClosed) {
      closed.add(node.rowKey);
    } else if (parentOpen) {
      open.push(node);
    }
    const below = sidebarExpansion(node.children, closedRowKeys, parentOpen && !isClosed);
    open.push(...below.open);
    for (const key of below.closed) closed.add(key);
  }
  return { open, closed };
}

/** The rows above a matching row, outermost first. Empty when it is top-level or absent. */
export function sidebarAncestors(
  matches: (node: SidebarNode) => boolean,
  nodes: readonly SidebarNode[],
): SidebarNode[] {
  const find = (level: readonly SidebarNode[], chain: SidebarNode[]): SidebarNode[] | null => {
    for (const node of level) {
      if (matches(node)) return chain;
      const found = find(node.children, [...chain, node]);
      if (found !== null) return found;
    }
    return null;
  };
  return find(nodes, []) ?? [];
}

/** Local projects and every machine's projects, in sidebar order. */
export function allSidebarProjects(nodes: readonly SidebarNode[]): ProjectSnapshot[] {
  return nodes.flatMap((node) => {
    switch (node.kind) {
      case 'project':
        return [node.project];
      case 'machine':
        return node.children.flatMap((child) => (child.kind === 'project' ? [child.project] : []));
      default:
        return [];
    }
  });
}

/** The first worktree row with this id. */
export function worktreeNode(id: string, nodes: readonly SidebarNode[]): SidebarNode | null {
  for (const node of nodes) {
    if (node.kind === 'worktree' && node.worktree.id === id) return node;
    const found = worktreeNode(id, node.children);
    if (found !== null) return found;
  }
  return null;
}

/**
 * Section titles cannot be selected. A machine row can — so keyboard and screen-reader users can
 * reach its actions — but it addresses nothing in the detail view.
 */
export function isSelectable(node: SidebarNode): boolean {
  return node.kind !== 'sectionHeader';
}

/** What selecting a row tells the rest of the app; null for rows that report nothing. */
export function selectionFor(node: SidebarNode): SidebarSelection | null {
  switch (node.kind) {
    case 'project':
      return { kind: 'project', id: node.project.projectId };
    case 'worktree':
      return { kind: 'worktree', id: node.worktree.id };
    case 'workspace':
      return { kind: 'workspace', id: node.workspace.id };
    case 'sectionHeader':
    case 'machine':
      return null;
  }
}

/** The row id a selection addresses; null means no row should be selected. */
export function targetId(selection: SidebarSelection | null): string | null {
  if (selection === null || selection.kind === 'nav') return null;
  return selection.id;
}

// MARK: - Machine status

/** The machine row's status dot, as a theme colour role (the Mac's system colours). */
export type MachineStatusColor = 'green' | 'orange' | 'red' | 'yellow' | 'tertiary';

export function sidebarStatusColor(state: MachineConnectionState): MachineStatusColor {
  switch (state.kind) {
    case 'connected':
      return state.daemon.kind === 'running'
        ? 'green'
        : state.daemon.kind === 'stopped'
          ? 'orange'
          : 'red';
    case 'connecting':
      return 'yellow';
    case 'offline':
      return 'red';
    case 'disconnected':
      return 'tertiary';
  }
}

// MARK: - Active project routing (AppState.updateActiveProject)

/** The two `AppState` fields the sidebar selection drives. */
export interface ActiveProjectState {
  readonly activeProjectRoot: string | null;
  readonly activeSidebarSelection: SidebarSelection | null;
}

/**
 * What routing reads from `AppState`: every open project (local and machines'), and the
 * registry's workspaces by project key id.
 */
export interface ActiveProjectInputs {
  readonly projects: readonly ProjectSnapshot[];
  readonly workspacesByProject: WorkspacesByProject;
}

function projectRootForWorkspace(id: string, inputs: ActiveProjectInputs): string | null {
  for (const [root, workspaces] of Object.entries(inputs.workspacesByProject)) {
    if (workspaces?.some((workspace) => workspace.id === id) === true) return root;
  }
  return null;
}

/**
 * The state after the user selects `selection` in the sidebar. Workspace, worktree, channel and
 * project selections resolve to their owning project; nav and null selections, and ids that
 * resolve to nothing, keep the last-known project. A Point Guard workspace names no project.
 */
export function updateActiveProject(
  state: ActiveProjectState,
  selection: SidebarSelection | null,
  inputs: ActiveProjectInputs,
): ActiveProjectState {
  let activeProjectRoot = state.activeProjectRoot;
  switch (selection?.kind) {
    case 'workspace': {
      const root = isPointGuard({ id: selection.id })
        ? null
        : projectRootForWorkspace(selection.id, inputs);
      if (root !== null) activeProjectRoot = root;
      break;
    }
    case 'worktree': {
      const project = inputs.projects.find((candidate) =>
        candidate.worktrees.some((worktree) => worktree.id === selection.id),
      );
      if (project !== undefined) activeProjectRoot = project.projectId;
      break;
    }
    case 'project':
    case 'channel':
      activeProjectRoot = selection.id;
      break;
    case 'nav':
    case undefined:
      break; // keep last known project
  }
  return { activeProjectRoot, activeSidebarSelection: selection };
}

/** The active root after a project opens: `root` only if no project is active yet. */
export function initializeActiveProjectIfNeeded(
  activeProjectRoot: string | null,
  root: string,
): string {
  return activeProjectRoot ?? root;
}
