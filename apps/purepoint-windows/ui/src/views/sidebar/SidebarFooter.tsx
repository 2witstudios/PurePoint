// Port of apps/purepoint-macos/purepoint-macos/Views/Sidebar/SidebarFooter.swift (WIN-7.2).
//
// The bar under the outline: Settings, then New (the command palette) and Open (a project folder),
// each with its accelerator from the key binding store.
//
// Differences from Swift:
// - New opens the command palette (views/palette/openPalette.ts) for the active project. Open runs
//   the menu's Open Project command (the folder picker, `appLifecycleHooks.openProject`, WIN-8.15).
// - Add Machine (the `desktopcomputer` button) opens `AddMachineSheet` through the machine sheets
//   store, which SidebarView renders (WIN-8.14).

import { displayString } from '../../model/keyBinding.ts';
import type { SidebarSelection } from '../../model/sidebarItem.ts';
import type { AppState } from '../../state/appState.ts';
import { useKeyBindingStore } from '../../state/keyBindingStore.ts';
import { Icon } from '../../theme/Icon.tsx';
import { isPointGuard } from '../../model/workspace.ts';
import { useAppState } from '../shell/appContext.ts';
import { type CommandPaletteRequest, openCommandPalette } from '../palette/openPalette.ts';
import { appLifecycleHooks } from '../../services/appLifecycle.ts';
import { useMachineSheets } from './machineSheets.ts';

/** The project New opens the palette for (Swift `activeProject`). */
export function footerActiveProjectId(
  app: AppState,
  selection: SidebarSelection | null,
): string | null {
  switch (selection?.kind) {
    case 'workspace': {
      const root = app.registry?.projectRoot(selection.id) ?? null;
      return root === null ? null : (app.projectState(root)?.projectId ?? null);
    }
    case 'worktree':
      return app.projectStateForWorktree(selection.id)?.projectId ?? null;
    case 'project':
    case 'channel':
      return app.projectState(selection.id)?.projectId ?? null;
    default:
      return app.projects[0]?.projectId ?? null;
  }
}

function isActivePointGuard(app: AppState): boolean {
  const workspace = app.registry?.activeWorkspace ?? null;
  return workspace !== null && isPointGuard(workspace);
}

/** Swift `showCommandPalette()`. */
export function showFooterCommandPalette(
  app: AppState,
  selection: SidebarSelection | null,
  open: (request: CommandPaletteRequest) => void = (request) => {
    openCommandPalette(app, request);
  },
): void {
  const onDashboard = selection?.kind === 'nav' && selection.item === 'dashboard';
  if (onDashboard || isActivePointGuard(app)) {
    app.newPointGuardTab();
    return;
  }
  const projectId = footerActiveProjectId(app, selection);
  if (projectId === null) return;
  open({ projectId, selection, includeWorktree: true });
}

export function SidebarFooter({
  selection,
  openPalette,
}: {
  selection: SidebarSelection | null;
  openPalette?: (request: CommandPaletteRequest) => void;
}) {
  const app = useAppState();
  const bindings = useKeyBindingStore((state) => state.bindings);

  return (
    <div className="sidebar-footer">
      <button
        type="button"
        className="sidebar-footer-button"
        aria-label="Settings"
        title="Settings"
        onClick={() => {
          app.setShowSettings(true);
        }}
      >
        <Icon name="gear" size={14} />
      </button>
      <button
        type="button"
        className="sidebar-footer-button"
        aria-label="Add Machine"
        title="Add Machine…"
        onClick={() => {
          useMachineSheets.getState().showAddMachine(true);
        }}
      >
        <Icon name="desktopcomputer" size={14} />
      </button>
      <span className="sidebar-footer-spacer" />
      <button
        type="button"
        className="sidebar-footer-button"
        aria-label="New"
        onClick={() => {
          showFooterCommandPalette(app, selection, openPalette);
        }}
      >
        <Icon name="plus" size={12} />
        <span>New</span>
        <span className="sidebar-footer-shortcut">{displayString(bindings.newAgent)}</span>
      </button>
      <button
        type="button"
        className="sidebar-footer-button"
        aria-label="Open"
        onClick={() => {
          appLifecycleHooks.openProject(app);
        }}
      >
        <span>Open</span>
        <span className="sidebar-footer-shortcut">{displayString(bindings.openProject)}</span>
      </button>
    </div>
  );
}
