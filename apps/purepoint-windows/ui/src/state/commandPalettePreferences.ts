// The command palette preferences of apps/purepoint-macos/purepoint-macos/State/SettingsState.swift
// (`usePointGuardNames`, `commandPaletteOrder`, `moveCommandPaletteItem`), ported with WIN-6.6 so
// the palette order and naming tests run. WIN-8.11 folds these into the full settings state.
import { type CommandPaletteItem, moveCommandPaletteItem } from '../model/commandPalette';

export const USE_POINT_GUARD_NAMES_KEY = 'PP_usePointGuardNames';
export const COMMAND_PALETTE_ORDER_KEY = 'PP_commandPaletteOrder';

/** The slice of Web Storage this needs (`localStorage` in the app). */
export type PreferenceStorage = Pick<Storage, 'getItem' | 'setItem'>;

function readOrder(raw: string | null): string[] {
  if (raw === null) return [];
  try {
    const value: unknown = JSON.parse(raw);
    return Array.isArray(value) ? value.filter((v): v is string => typeof v === 'string') : [];
  } catch {
    return [];
  }
}

export class CommandPalettePreferences {
  private pointGuardNames: boolean;
  private order: readonly string[];

  constructor(private readonly storage: PreferenceStorage) {
    const names = storage.getItem(USE_POINT_GUARD_NAMES_KEY);
    this.pointGuardNames = names === null ? true : names === 'true';
    this.order = readOrder(storage.getItem(COMMAND_PALETTE_ORDER_KEY));
  }

  /** Point Guard names new agents; off asks for a name first. Defaults to on. */
  get usePointGuardNames(): boolean {
    return this.pointGuardNames;
  }

  set usePointGuardNames(value: boolean) {
    this.pointGuardNames = value;
    this.storage.setItem(USE_POINT_GUARD_NAMES_KEY, String(value));
  }

  /** Item ids in the user's palette order. */
  get commandPaletteOrder(): readonly string[] {
    return this.order;
  }

  set commandPaletteOrder(value: readonly string[]) {
    this.order = [...value];
    this.storage.setItem(COMMAND_PALETTE_ORDER_KEY, JSON.stringify(this.order));
  }

  /** Swap visible neighbours while retaining preferences for entries unavailable in this project. */
  moveCommandPaletteItem(id: string, offset: number, items: readonly CommandPaletteItem[]): void {
    const next = moveCommandPaletteItem(this.order, id, offset, items);
    if (next !== this.order) this.commandPaletteOrder = next;
  }
}
