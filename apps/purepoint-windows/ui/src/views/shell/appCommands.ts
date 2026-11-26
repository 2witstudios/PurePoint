// Port of apps/purepoint-macos/purepoint-macos/AppCommands.swift (WIN-7.1).
//
// The app's menu commands, shown in the in-app menu bar (MenuBar.tsx) with accelerators from the
// key binding store. With `decorations: false` (DEC-10) a Windows window has no native menu bar
// to host a Tauri menu, and a Tauri menu would sit outside the custom title bar; an in-app menu
// bar in the title bar is what Windows apps with drawn chrome do (Windows Terminal, VS Code).
//
// The Mac's menus map to: File (New Agent, Open Project, Settings, Exit — Settings moves from the
// app menu, which Windows has no counterpart for), View (the sidebar toggles), Navigation, Panes,
// Tabs, and Help (Check for Updates, WIN-10.3). Every command dispatches to a named app action; the
// actions whose screens are later leaves exist as named no-ops (`PENDING_ACTIONS`) until those
// leaves land.

import {
  type HotkeyAction,
  type KeyBinding,
  SELECT_TAB_ACTIONS,
  actionDisplayName,
  displayString,
  isHotkeyAction,
} from '../../model/keyBinding.ts';
import type { SidebarNavItem, SidebarSelection } from '../../model/sidebarItem.ts';

/** Every menu command: the hotkey actions plus the menu-only ones. */
export type AppCommand = HotkeyAction | 'exit' | 'checkForUpdates';

export const MENU_SEPARATOR = 'separator';
export type MenuEntry = AppCommand | typeof MENU_SEPARATOR;

export interface AppMenu {
  readonly id: string;
  readonly title: string;
  readonly entries: readonly MenuEntry[];
}

export const APP_MENUS: readonly AppMenu[] = [
  {
    id: 'file',
    title: 'File',
    entries: ['newAgent', 'openProject', MENU_SEPARATOR, 'settings', MENU_SEPARATOR, 'exit'],
  },
  {
    id: 'view',
    title: 'View',
    entries: ['toggleSidebar', 'toggleWorkspaceSidebar', 'toggleChatSidebar'],
  },
  {
    id: 'navigation',
    title: 'Navigation',
    entries: [
      'focusSidebar',
      'focusContent',
      MENU_SEPARATOR,
      'navDashboard',
      'navAgents',
      'navSchedule',
      MENU_SEPARATOR,
      'closeAgent',
    ],
  },
  {
    id: 'panes',
    title: 'Panes',
    entries: [
      'splitBelow',
      'splitRight',
      'closePane',
      MENU_SEPARATOR,
      'focusUp',
      'focusDown',
      'focusLeft',
      'focusRight',
    ],
  },
  {
    id: 'tabs',
    title: 'Tabs',
    entries: [
      'newTab',
      MENU_SEPARATOR,
      'nextTab',
      'previousTab',
      ...SELECT_TAB_ACTIONS,
      'selectLastTab',
      MENU_SEPARATOR,
      'breakTab',
    ],
  },
  {
    id: 'help',
    title: 'Help',
    entries: ['checkForUpdates'],
  },
];

/** Every command the menus list, in menu order. */
export function menuCommands(): AppCommand[] {
  return APP_MENUS.flatMap((menu) =>
    menu.entries.filter((entry): entry is AppCommand => entry !== MENU_SEPARATOR),
  );
}

/** Commands that open a dialog or screen get an ellipsis, as on the Mac. */
const ELLIPSIS: ReadonlySet<AppCommand> = new Set([
  'newAgent',
  'openProject',
  'settings',
  'checkForUpdates',
]);

export function commandLabel(command: AppCommand): string {
  let label: string;
  switch (command) {
    case 'exit':
      label = 'Exit';
      break;
    case 'checkForUpdates':
      label = 'Check for Updates';
      break;
    default:
      label = actionDisplayName(command);
  }
  return ELLIPSIS.has(command) ? `${label}…` : label;
}

/** The accelerator a menu item shows (`Ctrl+Shift+N`), from the current bindings. */
export function acceleratorLabel(
  command: AppCommand,
  bindings: Readonly<Record<HotkeyAction, KeyBinding>>,
): string | null {
  if (command === 'exit') return 'Alt+F4';
  if (!isHotkeyAction(command)) return null;
  return displayString(bindings[command]);
}

// MARK: - Actions

export type AppActions = Readonly<Record<AppCommand, () => void>>;

/** What the shell can do today; ContentView supplies these. */
export interface AppActionDeps {
  selectNav: (item: SidebarNavItem) => void;
  /** Toggles a sidebar by its hotkey action (`toggleSidebar`, `toggleWorkspaceSidebar`). */
  toggleSidebar: (action: HotkeyAction) => void;
  showSettings: () => void;
  focusSidebar: () => void;
  focusContent: () => void;
  closeWindow: () => void;
  /** Check for Updates… (Swift `CheckForUpdatesViewModel.checkForUpdates`, WIN-10.3). */
  checkForUpdates: () => void;
}

// Commands whose screens are later leaves. Each is a named no-op until its leaf replaces it.

/**
 * What the command palette opens for (Swift `onShowCommandPalette`'s project, selection and
 * whether worktree variants are offered). Null: the palette's own default (Swift `activeProject`).
 */
export interface CommandPaletteRequest {
  readonly projectId: string;
  readonly selection: SidebarSelection | null;
  readonly includeWorktree: boolean;
}

/**
 * WIN-7.5: open the command palette (Swift `CommandPalettePanel.requestOpen` + `show`). The
 * sidebar's footer New button and its rows' add buttons call it.
 */
export const openCommandPalette: (request: CommandPaletteRequest | null) => void = () => {
  // Pending WIN-7.5.
};

/** WIN-7.5: the command palette (Swift `showNewAgentPalette`). */
function newAgent(): void {
  // Pending WIN-7.5.
}
/** WIN-8.x: the folder picker (Tauri dialog plugin, Swift `openProjectWithPicker`). */
function openProject(): void {
  // Pending the dialog plugin.
}
/** WIN-7.4: close the focused tab (Swift `.closeAgent` + `TabCloseConfirmation`). */
function closeTab(): void {
  // Pending WIN-7.4.
}
/** WIN-7.4: the pane grid's splits, focus moves and tab commands. */
function splitBelow(): void {
  // Pending WIN-7.4.
}
function splitRight(): void {
  // Pending WIN-7.4.
}
function closePane(): void {
  // Pending WIN-7.4.
}
function focusUp(): void {
  // Pending WIN-7.4.
}
function focusDown(): void {
  // Pending WIN-7.4.
}
function focusLeft(): void {
  // Pending WIN-7.4.
}
function focusRight(): void {
  // Pending WIN-7.4.
}
function newTab(): void {
  // Pending WIN-7.4.
}
function nextTab(): void {
  // Pending WIN-7.4.
}
function previousTab(): void {
  // Pending WIN-7.4.
}
function selectTab(): void {
  // Pending WIN-7.4 (Go to Tab N).
}
function selectLastTab(): void {
  // Pending WIN-7.4.
}
function breakTab(): void {
  // Pending WIN-7.4.
}
/** WIN-9.x: the Point Guard chat sidebar (Swift `.toggleChatSidebar` notification). */
function toggleChatSidebar(): void {
  // Pending the chat sidebar leaf.
}

/** The named no-ops, by command; exported so tests can tell stubs from real actions. */
export const PENDING_ACTIONS = {
  newAgent,
  openProject,
  closeAgent: closeTab,
  splitBelow,
  splitRight,
  closePane,
  focusUp,
  focusDown,
  focusLeft,
  focusRight,
  newTab,
  nextTab,
  previousTab,
  selectTab1: selectTab,
  selectTab2: selectTab,
  selectTab3: selectTab,
  selectTab4: selectTab,
  selectTab5: selectTab,
  selectTab6: selectTab,
  selectTab7: selectTab,
  selectTab8: selectTab,
  selectLastTab,
  breakTab,
  toggleChatSidebar,
} as const satisfies Partial<AppActions>;

export function createAppActions(deps: AppActionDeps): AppActions {
  return {
    ...PENDING_ACTIONS,
    settings: () => {
      deps.showSettings();
    },
    exit: () => {
      deps.closeWindow();
    },
    checkForUpdates: () => {
      deps.checkForUpdates();
    },
    toggleSidebar: () => {
      deps.toggleSidebar('toggleSidebar');
    },
    toggleWorkspaceSidebar: () => {
      deps.toggleSidebar('toggleWorkspaceSidebar');
    },
    focusSidebar: () => {
      deps.focusSidebar();
    },
    focusContent: () => {
      deps.focusContent();
    },
    navDashboard: () => {
      deps.selectNav('dashboard');
    },
    navAgents: () => {
      deps.selectNav('agents');
    },
    navSchedule: () => {
      deps.selectNav('schedule');
    },
  };
}
