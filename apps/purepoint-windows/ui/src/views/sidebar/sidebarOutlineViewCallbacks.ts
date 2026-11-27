// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineView+Callbacks.swift
// (WIN-7.2).
//
// The sidebar controller's callbacks: what each row action does to the app's state.
//
// Differences from Swift:
// - Callbacks address projects and machines by id; the live `ProjectState` is looked up here.
// - The terminal view cache (`viewCache.remove`) is WIN-7.3; nothing is cached yet to drop.
// - The add buttons open the command palette (views/palette/openPalette.ts); the worktree add
//   menu's New Terminal (`onAddTerminal`) spawns a terminal, or asks the palette for its name when
//   Point Guard does not name agents (WIN-7.8).
// - Hide on This PC (`onHideProject`) is `AppState.closeProject`, which hides a remote project on
//   this PC only. The terminal view cache drops hidden terminals on its own (WIN-7.3).
// - A remote project's removal error goes to the daemon banner.
// - A machine row's add button opens the remote folder picker (WIN-8.14; the Mac adds remote
//   projects from the CLI or another device, the iPad from this picker).

import type { SidebarSelection } from '../../model/sidebarItem.ts';
import type { AppState } from '../../state/appState.ts';
import {
  type CommandPaletteRequest,
  openCommandPalette,
  palettePreferences,
  showAddTerminalPalette,
} from '../palette/openPalette.ts';
import { useMachineSheets } from './machineSheets.ts';
import type {
  SidebarOutlineCallbacks,
  SidebarSelectOptions,
} from './SidebarOutlineViewController.tsx';

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export interface WireCallbacksOptions {
  /** The shell's selection binding. */
  readonly select: (selection: SidebarSelection | null, options?: SidebarSelectOptions) => void;
  /** Defaults to the app's command palette. */
  readonly showCommandPalette?: (request: CommandPaletteRequest) => void;
  /** Swift `settingsState.usePointGuardNames`; defaults to the palette preference. */
  readonly pointGuardNamesAgents?: () => boolean;
}

/** Swift `SidebarOutlineView.wireCallbacks`. */
export function wireCallbacks(
  app: AppState,
  options: WireCallbacksOptions,
): SidebarOutlineCallbacks {
  const pointGuardNamesAgents =
    options.pointGuardNamesAgents ?? (() => palettePreferences().usePointGuardNames);
  const machine = (machineId: string) =>
    app.machines.find((candidate) => candidate.machine.id === machineId);
  const showCommandPalette =
    options.showCommandPalette ??
    ((request: CommandPaletteRequest) => {
      openCommandPalette(app, request);
    });
  return {
    onSelectionChanged: (selection, select) => {
      options.select(selection, select);
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
      else machine.disconnect({ remember: true });
    },

    onStartDaemon: (machineId) => {
      app.machines.find((candidate) => candidate.machine.id === machineId)?.startDaemon();
    },

    onOpenRemoteProject: (machineId) => {
      useMachineSheets.getState().openFolderPicker(machineId);
    },

    // The worktree add menu's New Terminal: Point Guard names it, or the palette asks for a name.
    onAddTerminal: (projectId, worktreeId) => {
      const project = app.projectState(projectId);
      if (project === null) return;
      const selection: SidebarSelection = { kind: 'worktree', id: worktreeId };
      if (pointGuardNamesAgents()) {
        project.createAgent({ agent: 'terminal', prompt: '', selection });
      } else {
        showAddTerminalPalette(app, project, selection);
      }
    },

    onKillWorktreeAgents: (projectId, worktreeId) => {
      app.projectState(projectId)?.killWorktreeAgents(worktreeId);
    },

    onKillAllProjectAgents: (projectId) => {
      app.projectState(projectId)?.killAllAgents();
    },

    onHideProject: (projectId) => {
      app.closeProject(projectId);
    },

    onRetryLayout: (projectId) => {
      app.projectState(projectId)?.retryRemoteLayout();
    },

    onMoveProject: (projectId, index) => {
      const project = app.projectState(projectId);
      if (project !== null) app.moveProject(project, index);
    },

    onConnectMachine: (machineId) => {
      machine(machineId)?.connect();
    },

    onDisconnectMachine: (machineId) => {
      machine(machineId)?.disconnect({ remember: true });
    },

    onShowHiddenProjects: (machineId) => {
      machine(machineId)?.showHiddenProjects();
    },

    onDismissMachineError: (machineId) => {
      machine(machineId)?.dismissError();
    },
  };
}
