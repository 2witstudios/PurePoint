// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarOutlineView.swift (WIN-7.2).
//
// Bridges the app's state (projects, machines, the workspace registry) to the outline controller:
// Swift's `NSViewControllerRepresentable`, whose `updateNSViewController` hands the controller the
// canonical workspace list and rebuilds the tree. Here the stores are subscribed to and every
// change re-renders the controller, which rebuilds only when what the rows show has changed.

import { useMemo } from 'react';
import { createStore, useStore } from 'zustand';
import type { SidebarSelection } from '../../model/sidebarItem.ts';
import type { SidebarMenuContext } from '../../model/sidebarMenu.ts';
import type { WorkspaceRegistryState } from '../../state/workspaceRegistry.ts';
import { useAppState } from '../shell/appContext.ts';
import {
  type SidebarExpansionStoreApi,
  type SidebarSelectOptions,
  SidebarOutlineViewController,
} from './SidebarOutlineViewController.tsx';
import { wireCallbacks } from './sidebarOutlineViewCallbacks.ts';

/** Stands in for the registry's store while the app has none. */
const NO_REGISTRY = createStore<WorkspaceRegistryState>()(() => ({
  workspacesByProject: {},
  activeWorkspaceId: null,
  pendingPaletteSurfaceId: null,
  layoutErrorsByProject: {},
}));

export interface SidebarOutlineViewProps {
  readonly selection: SidebarSelection | null;
  readonly onSelectionChange: (
    selection: SidebarSelection | null,
    options?: SidebarSelectOptions,
  ) => void;
  readonly expansionStore?: SidebarExpansionStoreApi;
}

export function SidebarOutlineView({
  selection,
  onSelectionChange,
  expansionStore,
}: SidebarOutlineViewProps) {
  const app = useAppState();
  const projects = useStore(app.store, (state) => state.projects);
  const machines = useStore(app.store, (state) => state.machines);
  // Any project's own state (agents, worktrees) or a layout error changing re-renders the rows.
  useStore(app.store, (state) => state.projectRevision);
  const registryStore = app.registry?.store ?? NO_REGISTRY;
  const workspacesByProject = useStore(registryStore, (state) => state.workspacesByProject);
  const activeWorkspaceId = useStore(registryStore, (state) => state.activeWorkspaceId);
  useStore(registryStore, (state) => state.layoutErrorsByProject);

  // Snapshots of the live states: what the sidebar reads from each. The controller compares what
  // they show before it rebuilds anything.
  const projectSnapshots = projects.map((project) => project.snapshot());
  const machineSnapshots = machines.map((machine) => machine.snapshot());

  const callbacks = useMemo(
    () => wireCallbacks(app, { select: onSelectionChange }),
    [app, onSelectionChange],
  );

  const menuContext = useMemo<SidebarMenuContext>(
    () => ({
      machineName: (project) => {
        const machineId = app.projectState(project.projectId)?.machineId ?? null;
        return machines.find((machine) => machine.machine.id === machineId)?.machine.name ?? null;
      },
      isPointGuardFolder: (root) => app.isPointGuardFolder(root),
      siblingProjectIds: (project) => {
        const state = app.projectState(project.projectId);
        return state === null ? [] : app.siblingProjects(state).map((p) => p.projectId);
      },
      unsavedFileNames: (project) => {
        const state = app.projectState(project.projectId);
        const machine = machines.find((m) => m.machine.id === state?.machineId);
        return state === null ? [] : (machine?.unsavedFileNamesIn?.(state) ?? []);
      },
      machineUnsavedFileNames: (snapshot) =>
        machines.find((machine) => machine.machine.id === snapshot.id)?.unsavedFileNames ?? [],
    }),
    [app, machines],
  );

  return (
    <SidebarOutlineViewController
      projects={projectSnapshots}
      machines={machineSnapshots}
      workspacesByProject={workspacesByProject}
      activeWorkspaceId={activeWorkspaceId}
      selection={selection}
      menuContext={menuContext}
      {...(expansionStore === undefined ? {} : { expansionStore })}
      {...callbacks}
    />
  );
}
