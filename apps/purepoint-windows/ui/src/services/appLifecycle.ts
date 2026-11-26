// Port of apps/purepoint-macos/purepoint-macos/Platform/AppLifecycleHooks.swift (WIN-8.15).
//
// Hook points the shared app code calls where a platform supplies the behaviour; the features
// that implement them register a handler from their own files.
//
// Differences from Swift:
// - `openProject` is the Open Project command everywhere it is offered (File menu, Ctrl+Shift+O,
//   the sidebar footer). On Windows it defaults to the native folder picker
//   (`openProjectWithPicker`, the Mac's `NSOpenPanel` in `AppCommands.openProjectWithPicker` and
//   `SidebarFooter.openProjectWithPicker`); the Mac's hook exists only for the iPad.
// - `scenePhaseChanged` and `presentPalette` are iPad-only (scene phases, a palette without a key
//   `NSWindow`); a Windows window has neither, so they are not ported.

import { CommandPaletteOpenCoordinator } from '../state/commandPaletteOpenCoordinator.ts';
import { pickFolder } from './dialog.ts';

/** What Open Project needs of the app (`AppState.openProject`). */
export interface ProjectOpener {
  openProject(root: string): void;
}

/**
 * The Mac's `openProjectWithPicker`: choose one folder and open it as a project. The daemon
 * registers it (`init`); its refusal — a drive root or the home folder — shows in the daemon
 * banner, as on the Mac. Resolves the folder opened, or null when the user cancelled.
 */
export async function openProjectWithPicker(
  app: ProjectOpener,
  pick: typeof pickFolder = pickFolder,
): Promise<string | null> {
  const path = await pick({ title: 'Choose a project directory' });
  if (path === null || path === '') return null;
  app.openProject(path);
  return path;
}

export interface AppLifecycleHooks {
  /** The Open Project command; replaceable so a test (or another picker) can stand in. */
  openProject: (app: ProjectOpener) => void;
  /** Coalesces palette open requests while their data loads, as the Mac panel's coordinator. */
  readonly paletteOpenCoordinator: CommandPaletteOpenCoordinator;
}

export const appLifecycleHooks: AppLifecycleHooks = {
  openProject: (app) => {
    openProjectWithPicker(app).catch((error: unknown) => {
      console.error('PurePoint: Open Project failed', error);
    });
  },
  paletteOpenCoordinator: new CommandPaletteOpenCoordinator(),
};
