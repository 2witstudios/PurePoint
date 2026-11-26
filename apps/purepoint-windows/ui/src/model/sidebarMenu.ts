// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarMenuModel.swift (WIN-6.5).
//
// The sidebar's menus and the confirmations their actions ask for. Each entry carries an action
// descriptor naming what it acts on — never a callback — so the view only renders entries and
// WIN-7.8 maps each descriptor to its daemon request or app command.

import type { Workspace } from './workspace.ts';
import { tabCount, workspaceAgentIds } from './workspace.ts';
import { type AgentModel, type WorktreeModel, isAgentAlive } from './workspaceModel.ts';
import {
  type MachineSnapshot,
  type ProjectSnapshot,
  type SidebarNode,
  isDaemonStopped,
  machineIsConnectedOrConnecting,
  projectAllAgents,
  projectIsRemote,
  projectName,
} from './sidebarItem.ts';

/**
 * The device a hidden project stays hidden on — "Mac" on the Mac, "iPad" on the iPad. Windows
 * names itself "PC"; every other word of the labels is the Mac's.
 */
export const SIDEBAR_DEVICE = 'PC';

// MARK: - Menus

/** Everything a sidebar row's menus can do, with the ids each action addresses. */
export type SidebarMenuAction =
  | { readonly kind: 'connectMachine'; readonly machineId: string }
  | { readonly kind: 'disconnectMachine'; readonly machineId: string }
  | { readonly kind: 'startDaemon'; readonly machineId: string }
  | { readonly kind: 'showHiddenProjects'; readonly machineId: string }
  | { readonly kind: 'dismissMachineError'; readonly machineId: string }
  | { readonly kind: 'removeMachine'; readonly machineId: string }
  | { readonly kind: 'renameWorkspace'; readonly workspaceId: string }
  | {
      readonly kind: 'killWorkspace';
      readonly workspaceId: string;
      readonly agentIds: readonly string[];
    }
  | { readonly kind: 'killWorktreeAgents'; readonly projectId: string; readonly worktreeId: string }
  | { readonly kind: 'deleteWorktree'; readonly projectId: string; readonly worktreeId: string }
  | { readonly kind: 'killAllProjectAgents'; readonly projectId: string }
  | { readonly kind: 'retryLayout'; readonly projectId: string }
  | { readonly kind: 'hideProject'; readonly projectId: string }
  | { readonly kind: 'removeProject'; readonly projectId: string }
  | { readonly kind: 'moveProjectUp'; readonly projectId: string }
  | { readonly kind: 'moveProjectDown'; readonly projectId: string }
  | {
      readonly kind: 'newAgentInWorktree';
      readonly projectId: string;
      readonly worktreeId: string;
    }
  | {
      readonly kind: 'newTerminalInWorktree';
      readonly projectId: string;
      readonly worktreeId: string;
    };

/** One line of a sidebar menu. */
export type SidebarMenuEntry =
  | {
      readonly type: 'action';
      readonly title: string;
      readonly action: SidebarMenuAction;
      readonly isEnabled: boolean;
    }
  | { readonly type: 'separator' };

function item(title: string, action: SidebarMenuAction, isEnabled = true): SidebarMenuEntry {
  return { type: 'action', title, action, isEnabled };
}

const SEPARATOR: SidebarMenuEntry = { type: 'separator' };

/**
 * What the menus read from `AppState` beyond the row itself. Every member is optional: an absent
 * one behaves as the Mac does with no `AppState` attached (no sibling order, the machine called
 * "Machine", no Point Guard folders, nothing unsaved).
 */
export interface SidebarMenuContext {
  /** `AppState.siblingProjects(of:)`: the project ids listed beside this project, in order. */
  readonly siblingProjectIds?: (project: ProjectSnapshot) => readonly string[];
  /** The name of the machine a remote project lives on. */
  readonly machineName?: (project: ProjectSnapshot) => string | null;
  /** `AppState.isPointGuardFolder(_:)`. */
  readonly isPointGuardFolder?: (root: string) => boolean;
  /** `MachineState.unsavedFileNames(in:)` for a remote project. */
  readonly unsavedFileNames?: (project: ProjectSnapshot) => readonly string[];
}

/** A row's context menu, top to bottom. Empty for rows without one. */
export function contextMenu(
  node: SidebarNode,
  context: SidebarMenuContext = {},
): SidebarMenuEntry[] {
  switch (node.kind) {
    case 'workspace':
      return workspaceMenu(node.workspace);
    case 'worktree':
      return worktreeMenu(node.worktree, node.projectId);
    case 'project':
      return projectMenu(node.project, context);
    case 'machine':
      return machineMenu(node.machine);
    case 'sectionHeader':
      return [];
  }
}

/** The menu behind a worktree row's add button. */
export function worktreeAddMenu(projectId: string, worktreeId: string): SidebarMenuEntry[] {
  return [
    item('New Agent', { kind: 'newAgentInWorktree', projectId, worktreeId }),
    item('New Terminal', { kind: 'newTerminalInWorktree', projectId, worktreeId }),
  ];
}

export interface SidebarAccessibilityAction {
  readonly name: string;
  readonly action: SidebarMenuAction;
}

/** The machine's menu actions, reachable from a screen reader's actions list. */
export function machineAccessibilityActions(
  machine: MachineSnapshot,
): SidebarAccessibilityAction[] {
  const machineId = machine.id;
  const actions: SidebarAccessibilityAction[] = [];
  if (machineIsConnectedOrConnecting(machine)) {
    actions.push({ name: 'Disconnect', action: { kind: 'disconnectMachine', machineId } });
  } else {
    actions.push({ name: 'Connect', action: { kind: 'connectMachine', machineId } });
  }
  if (isDaemonStopped(machine.state) && !machine.isStartingDaemon) {
    actions.push({ name: 'Start Daemon', action: { kind: 'startDaemon', machineId } });
  }
  if (machine.hiddenProjectCount > 0) {
    actions.push({
      name: 'Show Hidden Projects',
      action: { kind: 'showHiddenProjects', machineId },
    });
  }
  if (machine.actionError !== null) {
    actions.push({ name: 'Dismiss Error', action: { kind: 'dismissMachineError', machineId } });
  }
  actions.push({ name: 'Remove Machine', action: { kind: 'removeMachine', machineId } });
  return actions;
}

function machineMenu(machine: MachineSnapshot): SidebarMenuEntry[] {
  const machineId = machine.id;
  const entries: SidebarMenuEntry[] = [];
  if (machineIsConnectedOrConnecting(machine)) {
    entries.push(item('Disconnect', { kind: 'disconnectMachine', machineId }));
  } else {
    entries.push(item('Connect', { kind: 'connectMachine', machineId }));
  }
  if (isDaemonStopped(machine.state)) {
    const title = machine.isStartingDaemon ? 'Starting Daemon…' : 'Start Daemon';
    entries.push(item(title, { kind: 'startDaemon', machineId }, !machine.isStartingDaemon));
  }
  if (machine.hiddenProjectCount > 0) {
    entries.push(
      item(`Show Hidden Projects (${String(machine.hiddenProjectCount)})`, {
        kind: 'showHiddenProjects',
        machineId,
      }),
    );
  }
  if (machine.actionError !== null) {
    entries.push(item('Dismiss Error', { kind: 'dismissMachineError', machineId }));
  }
  entries.push(SEPARATOR);
  entries.push(item('Remove Machine…', { kind: 'removeMachine', machineId }));
  return entries;
}

function workspaceMenu(workspace: Workspace): SidebarMenuEntry[] {
  const agentIds = workspaceAgentIds(workspace);
  const agentCount = agentIds.length;
  let title: string;
  if (agentCount === 0) title = 'Close Workspace';
  else if (agentCount === 1 && tabCount(workspace) === 1) title = 'Kill Agent';
  else if (agentCount === 1) title = 'Close Workspace (1 agent)';
  else title = `Close Workspace (${String(agentCount)} agents)`;
  return [
    item('Rename…', { kind: 'renameWorkspace', workspaceId: workspace.id }),
    SEPARATOR,
    item(title, { kind: 'killWorkspace', workspaceId: workspace.id, agentIds }),
  ];
}

function aliveCount(agents: readonly AgentModel[]): number {
  return agents.filter(isAgentAlive).length;
}

function worktreeMenu(worktree: WorktreeModel, projectId: string): SidebarMenuEntry[] {
  const entries: SidebarMenuEntry[] = [];
  const alive = aliveCount(worktree.agents);
  const worktreeId = worktree.id;
  if (alive > 0) {
    entries.push(
      item(`Kill All Agents (${String(alive)})`, {
        kind: 'killWorktreeAgents',
        projectId,
        worktreeId,
      }),
    );
    entries.push(SEPARATOR);
  }
  entries.push(item('Delete Worktree…', { kind: 'deleteWorktree', projectId, worktreeId }));
  return entries;
}

function projectMenu(project: ProjectSnapshot, context: SidebarMenuContext): SidebarMenuEntry[] {
  const projectId = project.projectId;
  const entries: SidebarMenuEntry[] = [];
  const alive = aliveCount(projectAllAgents(project));
  if (alive > 0) {
    entries.push(
      item(`Kill All Agents (${String(alive)})`, { kind: 'killAllProjectAgents', projectId }),
    );
    entries.push(SEPARATOR);
  }
  const isRemote = projectIsRemote(project);
  if (isRemote && project.layoutError !== null) {
    entries.push(item('Retry Layout', { kind: 'retryLayout', projectId }));
  }
  const siblings = context.siblingProjectIds?.(project) ?? null;
  const index = siblings?.indexOf(projectId) ?? -1;
  if (siblings !== null && siblings.length > 1 && index !== -1) {
    entries.push(item('Move Up', { kind: 'moveProjectUp', projectId }, index > 0));
    entries.push(
      item('Move Down', { kind: 'moveProjectDown', projectId }, index < siblings.length - 1),
    );
    entries.push(SEPARATOR);
  }
  if (isRemote) {
    // Hiding is this device's choice; removing changes the machine for every device.
    entries.push(item(`Hide on This ${SIDEBAR_DEVICE}…`, { kind: 'hideProject', projectId }));
    entries.push(
      item(`Remove from ${machineName(project, context)}…`, {
        kind: 'removeProject',
        projectId,
      }),
    );
  } else {
    entries.push(item('Remove Project…', { kind: 'removeProject', projectId }));
  }
  return entries;
}

export function machineName(project: ProjectSnapshot, context: SidebarMenuContext = {}): string {
  return context.machineName?.(project) ?? 'Machine';
}

// MARK: - Confirmations

/** A confirmation a sidebar action asks for before it runs. */
export interface SidebarConfirmation {
  readonly title: string;
  readonly message: string;
  readonly confirmTitle: string;
}

function confirmation(title: string, message: string, confirmTitle = 'Kill All') {
  return { title, message, confirmTitle };
}

/** Closing a remote project drops its file tabs, edits included. */
function unsavedWarning(names: readonly string[]): string {
  return names.length === 0 ? '' : ` Unsaved changes to ${names.join(', ')} will be lost.`;
}

function plural(count: number): string {
  return count === 1 ? '' : 's';
}

function unsavedFileNames(
  project: ProjectSnapshot,
  context: SidebarMenuContext,
): readonly string[] {
  if (!projectIsRemote(project)) return [];
  return context.unsavedFileNames?.(project) ?? [];
}

export const sidebarConfirmation = {
  disconnect(machine: MachineSnapshot, unsaved: readonly string[]): SidebarConfirmation {
    return confirmation(
      `Disconnect ${machine.name}?`,
      'Its projects close in PurePoint until you connect again.' + unsavedWarning(unsaved),
      'Disconnect',
    );
  },

  /** `unsaved` is `MachineState.unsavedFileNames` across the machine's projects. */
  removeMachine(machine: MachineSnapshot, unsaved: readonly string[] = []): SidebarConfirmation {
    return confirmation(
      `Remove ${machine.name}?`,
      'PurePoint will close its connection and forget it. The daemon, projects and agents on ' +
        `${machine.name} keep running and are not changed.` +
        unsavedWarning(unsaved),
      'Remove',
    );
  },

  closeWorkspace(agentCount: number): SidebarConfirmation {
    return confirmation(
      'Close this workspace?',
      `This will kill ${String(agentCount)} agents across its panes.`,
      'Close',
    );
  },

  killWorktreeAgents(worktree: WorktreeModel): SidebarConfirmation {
    const alive = aliveCount(worktree.agents);
    return confirmation(
      `Kill All Agents in ${worktree.branch}?`,
      `This will kill ${String(alive)} running agent${plural(alive)} in this worktree.`,
    );
  },

  deleteWorktree(worktree: WorktreeModel): SidebarConfirmation {
    const alive = aliveCount(worktree.agents);
    const agentNote =
      alive > 0 ? `This will kill ${String(alive)} running agent${plural(alive)}, ` : 'This will ';
    return confirmation(
      `Delete worktree ${worktree.branch}?`,
      `${agentNote}remove the worktree directory, and delete the branch locally and from GitHub.`,
      'Delete',
    );
  },

  killAllProjectAgents(project: ProjectSnapshot): SidebarConfirmation {
    const alive = aliveCount(projectAllAgents(project));
    return confirmation(
      `Kill All Agents in ${projectName(project)}?`,
      `This will kill ${String(alive)} running agent${plural(alive)} in this project.`,
    );
  },

  hideProject(project: ProjectSnapshot, context: SidebarMenuContext = {}): SidebarConfirmation {
    return confirmation(
      `Hide ${projectName(project)}?`,
      `This hides the project in PurePoint on this ${SIDEBAR_DEVICE}. Nothing on the remote machine is changed.` +
        unsavedWarning(unsavedFileNames(project, context)),
      'Hide',
    );
  },

  removeProject(project: ProjectSnapshot, context: SidebarMenuContext = {}): SidebarConfirmation {
    const name = projectName(project);
    const isRemote = projectIsRemote(project);
    if (!isRemote && context.isPointGuardFolder?.(project.projectRoot) === true) {
      return confirmation(
        `Remove ${name}?`,
        `Point Guard still uses this folder, so it only leaves the sidebar on this ${SIDEBAR_DEVICE}.`,
        'Remove',
      );
    }
    const machine = isRemote ? machineName(project, context) : null;
    const alive = projectAllAgents(project).filter(
      (agent) => isAgentAlive(agent) && !agent.suspended,
    ).length;
    const agents =
      alive === 0
        ? ''
        : ` Its ${String(alive)} running agent` +
          (alive === 1 ? ' is suspended' : 's are suspended') +
          ' and can be resumed once the project is added again.';
    return confirmation(
      machine === null ? `Remove ${name}?` : `Remove ${name} from ${machine}?`,
      `It disappears from PurePoint on ${machine ?? `this ${SIDEBAR_DEVICE}`} and every device connected to it.` +
        agents +
        ' Any other session in the folder is stopped. The folder and .pu/ are kept.' +
        unsavedWarning(unsavedFileNames(project, context)),
      'Remove',
    );
  },
};
