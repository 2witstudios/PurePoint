// The Panes and Tabs menu commands and Close Tab (WIN-7.4): Swift AppCommands.swift's split and tab
// helpers and ContentView.swift's `.closeAgent`, run on the workspace on screen
// (`registry.activeWorkspace`). Each does nothing when no workspace is on screen.
import { tabIndex } from '../../model/keyBinding.ts';
import { focusedPane, focusedSurface, type FocusDirection } from '../../model/workspace.ts';
import type { PaneAxis } from '../../model/paneSplit.ts';
import type { WorkspaceRegistry } from '../../state/workspaceRegistry.ts';
import type { PaneActions } from '../shell/appCommands.ts';
import { type CloseConfirmationSource, confirmTabClose } from './tabCloseConfirmation.tsx';

export function createPaneActions(
  registry: WorkspaceRegistry,
  source: CloseConfirmationSource,
): PaneActions {
  const active = () => registry.activeWorkspace;

  const split = (axis: PaneAxis) => {
    const workspace = active();
    if (workspace === null) return;
    registry.split(workspace.id, workspace.focusedLeafId, axis);
    const after = registry.workspace(workspace.id);
    registry.pendingPaletteSurfaceId = after === null ? null : (focusedSurface(after)?.id ?? null);
  };
  const moveFocus = (direction: FocusDirection) => {
    const workspace = active();
    if (workspace !== null) registry.moveFocus(workspace.id, direction);
  };
  const cycle = (offset: number) => {
    const workspace = active();
    if (workspace !== null) registry.cycleTab(workspace.id, workspace.focusedLeafId, offset);
  };
  const selectAt = (action: Parameters<typeof tabIndex>[0]) => () => {
    const workspace = active();
    const index = tabIndex(action);
    if (workspace !== null && index !== null) {
      registry.selectTabAt(workspace.id, workspace.focusedLeafId, index);
    }
  };

  return {
    // Ctrl+W closes the focused tab; its pane goes with its last tab, the workspace with its last pane.
    closeAgent: () => {
      const workspace = active();
      const surface = workspace === null ? null : focusedSurface(workspace);
      if (workspace === null || surface === null) return;
      confirmTabClose([surface], workspace.id, source, () => {
        registry.closeTab(workspace.id, surface.id);
      });
    },
    splitBelow: () => {
      split('horizontal');
    },
    splitRight: () => {
      split('vertical');
    },
    closePane: () => {
      const workspace = active();
      const pane = workspace === null ? null : focusedPane(workspace);
      if (workspace === null || pane === null) return;
      const leafId = workspace.focusedLeafId;
      confirmTabClose(pane.tabs, workspace.id, source, () => {
        registry.closePane(workspace.id, leafId);
      });
    },
    focusUp: () => {
      moveFocus('up');
    },
    focusDown: () => {
      moveFocus('down');
    },
    focusLeft: () => {
      moveFocus('left');
    },
    focusRight: () => {
      moveFocus('right');
    },
    newTab: () => {
      const workspace = active();
      if (workspace === null) return;
      registry.pendingPaletteSurfaceId = registry.newTab(workspace.id, workspace.focusedLeafId);
    },
    nextTab: () => {
      cycle(1);
    },
    previousTab: () => {
      cycle(-1);
    },
    selectTab1: selectAt('selectTab1'),
    selectTab2: selectAt('selectTab2'),
    selectTab3: selectAt('selectTab3'),
    selectTab4: selectAt('selectTab4'),
    selectTab5: selectAt('selectTab5'),
    selectTab6: selectAt('selectTab6'),
    selectTab7: selectAt('selectTab7'),
    selectTab8: selectAt('selectTab8'),
    selectLastTab: () => {
      const workspace = active();
      if (workspace !== null) registry.selectLastTab(workspace.id, workspace.focusedLeafId);
    },
    breakTab: () => {
      const workspace = active();
      const surface = workspace === null ? null : focusedSurface(workspace);
      if (workspace !== null && surface !== null) {
        registry.breakTab(workspace.id, surface.id, 'vertical');
      }
    },
  };
}
