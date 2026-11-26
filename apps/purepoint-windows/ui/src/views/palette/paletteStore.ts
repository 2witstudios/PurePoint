// The presented palette: the static `CommandPalettePanel.current` / `requestOpen` / `show` of
// apps/purepoint-macos/purepoint-macos/Views/CommandPalette/CommandPalettePanel.swift (WIN-7.5).
// One palette shows at a time; `CommandPaletteHost` (CommandPalettePanel.tsx) renders it.
import { create } from 'zustand';
import type { CommandPaletteItem, CommandPaletteResult } from '../../model/commandPalette.ts';
import { CommandPaletteOpenCoordinator } from '../../state/commandPaletteOpenCoordinator.ts';

export interface PalettePresentation {
  readonly items: readonly CommandPaletteItem[];
  readonly usePointGuardNames: boolean;
  /** Selected as soon as the palette shows (the worktree add menu's New Terminal). */
  readonly initialItem: CommandPaletteItem | null;
  readonly onSelect: (result: CommandPaletteResult) => void;
  /** Tells one presentation from the next, so a reopened palette starts fresh. */
  readonly id: number;
}

interface PaletteStoreState {
  readonly presentation: PalettePresentation | null;
}

export const usePaletteStore = create<PaletteStoreState>()(() => ({ presentation: null }));

const openCoordinator = new CommandPaletteOpenCoordinator();
let nextId = 1;

export const isPaletteVisible = (): boolean => usePaletteStore.getState().presentation !== null;

export function dismissPalette(): void {
  usePaletteStore.setState({ presentation: null });
}

/**
 * Swift `requestOpen`: toggles a visible palette closed at once; otherwise runs `open` (which
 * loads the items and calls `showPalette`) unless an earlier open is still loading.
 */
export function requestPaletteOpen(open: () => Promise<void> | void): Promise<void> | null {
  if (isPaletteVisible()) {
    dismissPalette();
    return null;
  }
  return openCoordinator.request(open);
}

/** Swift `show(relativeTo:items:usePointGuardNames:initialItem:onSelect:)`. */
export function showPalette(options: {
  items: readonly CommandPaletteItem[];
  usePointGuardNames: boolean;
  initialItem?: CommandPaletteItem | null;
  onSelect: (result: CommandPaletteResult) => void;
}): void {
  if (isPaletteVisible()) {
    dismissPalette();
    return;
  }
  const { onSelect } = options;
  usePaletteStore.setState({
    presentation: {
      items: options.items,
      usePointGuardNames: options.usePointGuardNames,
      initialItem: options.initialItem ?? null,
      onSelect: (result) => {
        dismissPalette();
        onSelect(result);
      },
      id: nextId++,
    },
  });
}
