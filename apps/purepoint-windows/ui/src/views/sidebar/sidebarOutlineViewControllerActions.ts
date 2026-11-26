// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineViewController+Actions.swift
// (WIN-7.8).
//
// What the sidebar's menu actions do: each `SidebarMenuAction` descriptor (model/sidebarMenu.ts)
// becomes its controller callback, behind the confirmation the Mac asks for. The controller
// supplies `confirm` (the Mac's `showConfirmation`) and `beginRename`; everything else is here.
//
// Differences from Swift: rows and callbacks are addressed by ids, so the project, worktree and
// machine a descriptor names are looked up in the outline's nodes rather than held.

import {
  type MachineSnapshot,
  type ProjectSnapshot,
  type SidebarNode,
  machineIsConnectedOrConnecting,
} from '../../model/sidebarItem.ts';
import {
  type SidebarConfirmation,
  type SidebarMenuAction,
  type SidebarMenuContext,
  sidebarConfirmation,
} from '../../model/sidebarMenu.ts';
import type { WorkspacesByProject } from '../../model/sidebarOutline.ts';
import type { WorktreeModel } from '../../model/workspaceModel.ts';
import { type Workspace, workspaceAgentIds } from '../../model/workspace.ts';
import type { SidebarOutlineCallbacks } from './SidebarOutlineViewController.tsx';

export interface SidebarActionContext {
  readonly callbacks: SidebarOutlineCallbacks;
  /** The outline's tree. */
  readonly nodes: readonly SidebarNode[];
  readonly workspacesByProject: WorkspacesByProject;
  readonly menuContext?: SidebarMenuContext | undefined;
  /** Swift `showConfirmation(_:action:)`: ask, and run `action` on confirm. */
  readonly confirm: (confirmation: SidebarConfirmation, action: () => void) => void;
  /** Swift `beginRename(_:)`: the workspace row's inline rename field. */
  readonly beginRename: (workspaceId: string) => void;
}

// MARK: - Lookups (Swift `findProject`, `findWorkspace`)

function walk(nodes: readonly SidebarNode[], visit: (node: SidebarNode) => boolean): boolean {
  for (const node of nodes) {
    if (visit(node) || walk(node.children, visit)) return true;
  }
  return false;
}

export function findProject(
  nodes: readonly SidebarNode[],
  projectId: string,
): ProjectSnapshot | null {
  let found: ProjectSnapshot | null = null;
  walk(nodes, (node) => {
    if (node.kind === 'project' && node.project.projectId === projectId) found = node.project;
    return found !== null;
  });
  return found;
}

export function findWorktree(
  nodes: readonly SidebarNode[],
  worktreeId: string,
): WorktreeModel | null {
  let found: WorktreeModel | null = null;
  walk(nodes, (node) => {
    if (node.kind === 'worktree' && node.worktree.id === worktreeId) found = node.worktree;
    return found !== null;
  });
  return found;
}

export function findMachine(
  nodes: readonly SidebarNode[],
  machineId: string,
): MachineSnapshot | null {
  return (
    nodes
      .flatMap((node) => (node.kind === 'machine' ? [node.machine] : []))
      .find((machine) => machine.id === machineId) ?? null
  );
}

export function findWorkspace(
  workspacesByProject: WorkspacesByProject,
  workspaceId: string,
): { projectId: string; workspace: Workspace } | null {
  for (const [projectId, list] of Object.entries(workspacesByProject)) {
    const workspace = list?.find((candidate) => candidate.id === workspaceId);
    if (workspace !== undefined) return { projectId, workspace };
  }
  return null;
}

// MARK: - Perform

/** The destructive item of a row's menu, which Delete runs (Kill Agent, Delete Worktree, ...). */
export function destructiveAction(node: SidebarNode): SidebarMenuAction | null {
  switch (node.kind) {
    case 'workspace':
      return {
        kind: 'killWorkspace',
        workspaceId: node.workspace.id,
        agentIds: workspaceAgentIds(node.workspace),
      };
    case 'worktree':
      return { kind: 'deleteWorktree', projectId: node.projectId, worktreeId: node.worktree.id };
    case 'project':
      return { kind: 'removeProject', projectId: node.project.projectId };
    case 'machine':
      return { kind: 'removeMachine', machineId: node.machine.id };
    case 'sectionHeader':
      return null;
  }
}

/** Swift `perform(_:on:)`: runs a menu action chosen on a row. */
export function performSidebarMenuAction(
  action: SidebarMenuAction,
  context: SidebarActionContext,
): void {
  const { callbacks, nodes, menuContext, confirm } = context;
  switch (action.kind) {
    case 'connectMachine':
      callbacks.onConnectMachine?.(action.machineId);
      return;
    case 'disconnectMachine':
      confirmDisconnect(action.machineId, context);
      return;
    case 'startDaemon':
      callbacks.onStartDaemon?.(action.machineId);
      return;
    case 'showHiddenProjects':
      callbacks.onShowHiddenProjects?.(action.machineId);
      return;
    case 'dismissMachineError':
      callbacks.onDismissMachineError?.(action.machineId);
      return;
    case 'removeMachine': {
      const machine = findMachine(nodes, action.machineId);
      if (machine === null) return;
      confirm(sidebarConfirmation.removeMachine(machine), () => {
        callbacks.onRemoveMachine?.(machine.id);
      });
      return;
    }
    case 'renameWorkspace':
      context.beginRename(action.workspaceId);
      return;
    case 'killWorkspace': {
      const found = findWorkspace(context.workspacesByProject, action.workspaceId);
      if (found === null) return;
      const kill = () => {
        callbacks.onKillWorkspace?.(found.projectId, action.workspaceId);
      };
      const agentCount = workspaceAgentIds(found.workspace).length;
      if (agentCount > 1) confirm(sidebarConfirmation.closeWorkspace(agentCount), kill);
      else kill();
      return;
    }
    case 'killWorktreeAgents': {
      const worktree = findWorktree(nodes, action.worktreeId);
      if (worktree === null || findProject(nodes, action.projectId) === null) return;
      confirm(sidebarConfirmation.killWorktreeAgents(worktree), () => {
        callbacks.onKillWorktreeAgents?.(action.projectId, action.worktreeId);
      });
      return;
    }
    case 'deleteWorktree': {
      const worktree = findWorktree(nodes, action.worktreeId);
      if (worktree === null || findProject(nodes, action.projectId) === null) return;
      confirm(sidebarConfirmation.deleteWorktree(worktree), () => {
        callbacks.onDeleteWorktree?.(action.projectId, action.worktreeId);
      });
      return;
    }
    case 'killAllProjectAgents': {
      const project = findProject(nodes, action.projectId);
      if (project === null) return;
      confirm(sidebarConfirmation.killAllProjectAgents(project), () => {
        callbacks.onKillAllProjectAgents?.(action.projectId);
      });
      return;
    }
    case 'retryLayout':
      callbacks.onRetryLayout?.(action.projectId);
      return;
    case 'hideProject': {
      const project = findProject(nodes, action.projectId);
      if (project === null) return;
      confirm(sidebarConfirmation.hideProject(project, menuContext), () => {
        callbacks.onHideProject?.(action.projectId);
      });
      return;
    }
    case 'removeProject': {
      const project = findProject(nodes, action.projectId);
      if (project === null) return;
      confirm(sidebarConfirmation.removeProject(project, menuContext), () => {
        callbacks.onRemoveProject?.(action.projectId);
      });
      return;
    }
    case 'moveProjectUp':
    case 'moveProjectDown': {
      const project = findProject(nodes, action.projectId);
      const siblings =
        project === null ? null : (menuContext?.siblingProjectIds?.(project) ?? null);
      const index = siblings?.indexOf(action.projectId) ?? -1;
      if (index === -1) return;
      callbacks.onMoveProject?.(
        action.projectId,
        index + (action.kind === 'moveProjectUp' ? -1 : 1),
      );
      return;
    }
    case 'newAgentInWorktree':
      if (findProject(nodes, action.projectId) === null) return;
      callbacks.onShowCommandPalette?.(
        action.projectId,
        { kind: 'worktree', id: action.worktreeId },
        false,
      );
      return;
    case 'newTerminalInWorktree':
      if (findWorktree(nodes, action.worktreeId) === null) return;
      callbacks.onAddTerminal?.(action.projectId, action.worktreeId);
      return;
  }
}

/** Swift `toggleConnection(of:)`: Enter on a machine row connects or disconnects it. */
export function toggleConnection(machineId: string, context: SidebarActionContext): void {
  const machine = findMachine(context.nodes, machineId);
  if (machine === null) return;
  if (machineIsConnectedOrConnecting(machine)) confirmDisconnect(machineId, context);
  else context.callbacks.onConnectMachine?.(machineId);
}

/** Swift `confirmDisconnect(_:)`: asks only when unsaved files would be dropped. */
function confirmDisconnect(machineId: string, context: SidebarActionContext): void {
  const machine = findMachine(context.nodes, machineId);
  if (machine === null) return;
  const unsaved = context.menuContext?.machineUnsavedFileNames?.(machine) ?? [];
  const disconnect = () => {
    context.callbacks.onDisconnectMachine?.(machineId);
  };
  if (unsaved.length === 0) disconnect();
  else context.confirm(sidebarConfirmation.disconnect(machine, unsaved), disconnect);
}
