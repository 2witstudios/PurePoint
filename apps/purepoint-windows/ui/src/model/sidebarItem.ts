// Port of apps/purepoint-macos/purepoint-macos/Models/SidebarItem.swift (WIN-6.5).
//
// The sidebar's selection and its tree nodes. On the Mac a node wraps the live `ProjectState`,
// `MachineState` and `WorktreeModel` objects; those observable states are not ported yet (WIN-6.0
// and WIN-8.14), so here a node holds a plain snapshot of what the sidebar reads from them —
// `ProjectSnapshot` and `MachineSnapshot` below. Whoever owns the live state builds the snapshots.

import type { AgentModel, WorktreeModel } from './workspaceModel.ts';
import { type Workspace, containersEqual, isPointGuard, worktreeContainer } from './workspace.ts';
import { lastPathComponent, parseProjectKey } from './projectKey.ts';

// MARK: - Nav items

export const SIDEBAR_NAV_ITEMS = ['dashboard', 'agents', 'schedule'] as const;
export type SidebarNavItem = (typeof SIDEBAR_NAV_ITEMS)[number];

export function navItemTitle(item: SidebarNavItem): string {
  return item.charAt(0).toUpperCase() + item.slice(1);
}

/** Lucide icon names (the parity matrix's SF Symbol table). */
export const NAV_ITEM_ICONS: Record<SidebarNavItem, string> = {
  dashboard: 'messages-square',
  agents: 'cpu',
  schedule: 'calendar',
};

// MARK: - Selection

/**
 * What the sidebar can have selected.
 *
 * There is no `agent` case: agents live in panes, and panes live in workspaces, so a pane can
 * never reappear as its own row. `channel` and `project` carry a project key id (the root path
 * for local projects).
 */
export type SidebarSelection =
  | { readonly kind: 'nav'; readonly item: SidebarNavItem }
  | { readonly kind: 'workspace'; readonly id: string }
  | { readonly kind: 'worktree'; readonly id: string }
  | { readonly kind: 'channel'; readonly id: string }
  | { readonly kind: 'project'; readonly id: string };

export const selection = {
  nav: (item: SidebarNavItem): SidebarSelection => ({ kind: 'nav', item }),
  workspace: (id: string): SidebarSelection => ({ kind: 'workspace', id }),
  worktree: (id: string): SidebarSelection => ({ kind: 'worktree', id }),
  channel: (id: string): SidebarSelection => ({ kind: 'channel', id }),
  project: (id: string): SidebarSelection => ({ kind: 'project', id }),
};

export function selectionsEqual(a: SidebarSelection | null, b: SidebarSelection | null): boolean {
  if (a === null || b === null) return a === b;
  if (a.kind === 'nav' || b.kind === 'nav') {
    return a.kind === 'nav' && b.kind === 'nav' && a.item === b.item;
  }
  return a.kind === b.kind && a.id === b.id;
}

// MARK: - Snapshots of the live state (shape assumed until WIN-6.0 / WIN-8.14)

/** What the sidebar reads from one `ProjectState`. */
export interface ProjectSnapshot {
  /** `ProjectKey` id: the bare root locally, `machine:<uuid>:<root>` on a machine. */
  readonly projectId: string;
  /** The root path on the machine that owns the project. */
  readonly projectRoot: string;
  readonly rootAgents: readonly AgentModel[];
  readonly worktrees: readonly WorktreeModel[];
  /** False until the project's first snapshot has been read. */
  readonly hasLoadedWorktrees: boolean;
  readonly isUnavailable: boolean;
  /** `project.channel.unreadCount`. */
  readonly channelUnread: number;
  /** `registry.layoutErrorsByProject[projectId]`; only ever shown for a remote project. */
  readonly layoutError: string | null;
}

export function projectIsRemote(project: ProjectSnapshot): boolean {
  return parseProjectKey(project.projectId).machineId !== null;
}

export function projectMachineId(project: ProjectSnapshot): string | null {
  return parseProjectKey(project.projectId).machineId;
}

export function projectName(project: ProjectSnapshot): string {
  return lastPathComponent(project.projectRoot);
}

/** Worktree agents first, then the root's, as Swift's `allAgents`. */
export function projectAllAgents(project: ProjectSnapshot): AgentModel[] {
  return [...project.worktrees.flatMap((worktree) => worktree.agents), ...project.rootAgents];
}

export function projectAgent(project: ProjectSnapshot, id: string): AgentModel | null {
  for (const worktree of project.worktrees) {
    const agent = worktree.agents.find((candidate) => candidate.id === id);
    if (agent !== undefined) return agent;
  }
  return project.rootAgents.find((candidate) => candidate.id === id) ?? null;
}

/** Swift `DaemonAvailability`. */
export type DaemonAvailability =
  | { readonly kind: 'running' }
  | { readonly kind: 'stopped' }
  /** The daemon speaks another protocol version; no other requests are made. */
  | { readonly kind: 'incompatible'; readonly version: number };

/** Swift `MachineConnectionState`. */
export type MachineConnectionState =
  | { readonly kind: 'disconnected' }
  | { readonly kind: 'connecting' }
  | { readonly kind: 'connected'; readonly daemon: DaemonAvailability }
  | { readonly kind: 'offline'; readonly reason: string };

export function isDaemonRunning(state: MachineConnectionState): boolean {
  return state.kind === 'connected' && state.daemon.kind === 'running';
}

export function isDaemonStopped(state: MachineConnectionState): boolean {
  return state.kind === 'connected' && state.daemon.kind === 'stopped';
}

/** What the sidebar reads from one `MachineState`. */
export interface MachineSnapshot {
  /** Lowercase UUID. */
  readonly id: string;
  readonly name: string;
  readonly sshHost: string;
  readonly state: MachineConnectionState;
  readonly statusText: string;
  /** The latest failure of an explicit action (start daemon, project discovery). */
  readonly actionError: string | null;
  readonly isStartingDaemon: boolean;
  readonly hiddenProjectCount: number;
  /** Bumped when the hidden set changes, so the render state notices. */
  readonly hiddenCountRevision: number;
  /** Whether the machine has reported its project list at least once. */
  readonly hasLoadedProjects: boolean;
  readonly projects: readonly ProjectSnapshot[];
}

export function machineIsConnectedOrConnecting(machine: MachineSnapshot): boolean {
  return machine.state.kind !== 'disconnected';
}

/**
 * Whether this machine answers for the rows it shows: its daemon is running and has reported its
 * list at least once.
 */
export function machineReportsCompleteProjects(machine: MachineSnapshot): boolean {
  return isDaemonRunning(machine.state) && machine.hasLoadedProjects;
}

// MARK: - SidebarNode

interface NodeBase {
  /** What a selection addresses: the project key, worktree id or workspace id. */
  readonly id: string;
  /**
   * Row identity: every ancestor's scope, then this row's own kind and id. `id` alone is not
   * enough — a worktree id under two projects, or the same project root under two machines,
   * would otherwise be one row as far as expansion is concerned.
   */
  readonly rowKey: string;
  readonly children: readonly SidebarNode[];
}

export type SidebarNode =
  | (NodeBase & { readonly kind: 'project'; readonly project: ProjectSnapshot })
  | (NodeBase & {
      readonly kind: 'worktree';
      readonly worktree: WorktreeModel;
      /** The owning project's key id, which a worktree's menu actions address. */
      readonly projectId: string;
    })
  | (NodeBase & { readonly kind: 'workspace'; readonly workspace: Workspace })
  /** The "MACHINES" section title below the local projects. */
  | (NodeBase & { readonly kind: 'sectionHeader'; readonly title: string })
  | (NodeBase & { readonly kind: 'machine'; readonly machine: MachineSnapshot });

export function isExpandable(node: SidebarNode): boolean {
  return node.kind === 'project' || node.kind === 'worktree' || node.kind === 'machine';
}

/**
 * Reserved by the key grammar: the tier separator `:`, the parent separator `/`, and the escape
 * itself. A project root is an arbitrary path, so these must not pass through raw.
 */
function escapeKey(id: string): string {
  return id.replaceAll('%', '%25').replaceAll(':', '%3A').replaceAll('/', '%2F');
}

export function rowKey(kind: string, id: string, parent: string | null = null): string {
  const component = `${kind}:${escapeKey(id)}`;
  return parent === null ? component : `${parent}/${component}`;
}

/** A machine row's key. Projects on it are keyed under this. */
export function machineKey(machineId: string): string {
  return rowKey('machine', machineId.toLowerCase());
}

export function machineNodeId(machineId: string): string {
  return 'machine-node:' + machineId.toLowerCase();
}

export function sectionHeaderNode(title: string): SidebarNode {
  return {
    kind: 'sectionHeader',
    title,
    id: 'section:' + title,
    rowKey: rowKey('section', title),
    children: [],
  };
}

export function machineNode(
  machine: MachineSnapshot,
  children: readonly SidebarNode[],
): SidebarNode {
  return {
    kind: 'machine',
    machine,
    id: machineNodeId(machine.id),
    rowKey: machineKey(machine.id),
    children,
  };
}

function workspaceNode(workspace: Workspace, parentKey: string): SidebarNode {
  return {
    kind: 'workspace',
    workspace,
    id: workspace.id,
    rowKey: rowKey('workspace', workspace.id, parentKey),
    children: [],
  };
}

/**
 * A project row with its subtree: the project root's workspaces first, then one row per worktree
 * holding that worktree's workspaces.
 */
export function projectNode(
  project: ProjectSnapshot,
  workspaces: readonly Workspace[],
  parentKey: string | null = null,
): SidebarNode {
  const projectKey = rowKey('project', project.projectId, parentKey);
  const children: SidebarNode[] = workspaces
    .filter((workspace) => workspace.container.kind === 'projectRoot' && !isPointGuard(workspace))
    .map((workspace) => workspaceNode(workspace, projectKey));
  for (const worktree of project.worktrees) {
    const worktreeKey = rowKey('worktree', worktree.id, projectKey);
    const container = worktreeContainer(worktree.id);
    children.push({
      kind: 'worktree',
      worktree,
      projectId: project.projectId,
      id: worktree.id,
      rowKey: worktreeKey,
      children: workspaces
        .filter((workspace) => containersEqual(workspace.container, container))
        .map((workspace) => workspaceNode(workspace, worktreeKey)),
    });
  }
  return { kind: 'project', project, id: project.projectId, rowKey: projectKey, children };
}
