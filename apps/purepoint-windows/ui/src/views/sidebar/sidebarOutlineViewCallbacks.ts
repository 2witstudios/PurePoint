// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineView+Callbacks.swift
// (WIN-7.2).
//
// The sidebar controller's callbacks: what each row action does to the app's state.
//
// Differences from Swift:
// - Callbacks address projects and machines by id; the live `ProjectState` is looked up here.
// - The terminal view cache (`viewCache.remove`) is WIN-7.3; nothing is cached yet to drop.
// - The command palette is WIN-7.5: the add buttons call the named `openCommandPalette` stub, and
//   `onAddTerminal` (the worktree add menu's New Terminal) arrives with the menus (WIN-7.8).
// - A remote project's removal error goes to the daemon banner; the machine's own `actionError`
//   is part of the machines port (WIN-8.14).

import type { SidebarSelection } from '../../model/sidebarItem.ts';
import type { AppState } from '../../state/appState.ts';
import { openCommandPalette } from '../shell/appCommands.ts';
import type { SidebarOutlineCallbacks } from './SidebarOutlineViewController.tsx';

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface WireCallbacksOptions {
  /** The shell's selection binding. */
  readonly select: (selection: SidebarSelection | null) => void;
  /** Defaults to the app's `openCommandPalette` stub. */
  readonly showCommandPalette?: typeof openCommandPalette;
}

/** Swift `SidebarOutlineView.wireCallbacks`. */
export function wireCallbacks(
  app: AppState,
  options: WireCallbacksOptions,
): SidebarOutlineCallbacks {
  const showCommandPalette = options.showCommandPalette ?? openCommandPalette;
  return {
    onSelectionChanged: (selection) => {
      options.select(selection);
    },

    onShowCommandPalette: (projectId, selection, includeWorktree) => {
      showCommandPalette({ projectId, selection, includeWorktree });
    },

    // Closing the row closes everything in it — agent tabs (killed via the registry's
    // onCloseAgent) and file tabs alike — so a workspace of only files can be closed too.
    onKillWorkspace: (_projectId, workspaceId) => {
      app.registry?.closeWorkspace(workspaceId);
    },

    onRenameAgent: (projectId, agentId, name) => {
      app.projectState(projectId)?.renameAgent(agentId, name);
    },

    onDeleteWorktree: (projectId, worktreeId) => {
      app.projectState(projectId)?.deleteWorktree(worktreeId);
    },

    // The machine's daemon forgets the project, so it leaves every device's sidebar. Its files,
    // including .pu/, stay: adding the folder again brings the project back.
    onRemoveProject: (projectId) => {
      const project = app.projectState(projectId);
      if (project === null) return;
      app.removeProject(project).catch((error: unknown) => {
        app.setDaemonError(`Could not remove ${project.projectName}: ${errorMessage(error)}`);
      });
    },

    onRemoveMachine: (machineId) => {
      const machine = app.machines.find((candidate) => candidate.machine.id === machineId);
      if (machine !== undefined) app.removeMachine(machine);
    },

    // Return on a machine row connects or disconnects it.
    onToggleMachineConnection: (machineId) => {
      const machine = app.machines.find((candidate) => candidate.machine.id === machineId);
      if (machine === undefined) return;
      if (machine.snapshot().state.kind === 'disconnected') machine.connect();
      else machine.disconnect();
    },
  };
}
