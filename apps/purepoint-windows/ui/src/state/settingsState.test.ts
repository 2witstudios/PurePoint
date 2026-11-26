import { describe, expect, test, vi } from 'vitest';
import { builtInItem, itemId } from '../model/commandPalette.ts';
import { CLAUDE, CODEX, OPENCODE } from '../model/agentVariant.ts';
import { signalNotificationsEnabled } from '../services/agentSignalNotifications.ts';
import { memoryStorage } from '../views/shell/testApp.ts';
import { CommandPalettePreferences } from './commandPalettePreferences.ts';
import { createSettingsStore, loadSettings } from './settingsState.ts';

describe('settings state', () => {
  test('given nothing saved, should use the Mac defaults', () => {
    const settings = loadSettings(memoryStorage());
    expect(settings).toEqual({
      restoreProjectsOnLaunch: true,
      launchAtLogin: false,
      usePointGuardNames: true,
      commandPaletteOrder: [],
      signalNotifications: { permission: true, question: true, error: true, completion: false },
      gridGap: 1,
    });
  });

  test('given changes, should persist each one under the Mac key', () => {
    const storage = memoryStorage();
    const store = createSettingsStore({ storage });
    const s = store.getState();
    s.setRestoreProjectsOnLaunch(false);
    s.setLaunchAtLogin(true);
    s.setUsePointGuardNames(false);
    s.setCommandPaletteOrder(['codex', 'claude']);
    s.setGridGap(4);
    expect(storage.getItem('PP_restoreProjectsOnLaunch')).toBe('false');
    expect(storage.getItem('PP_launchAtLogin')).toBe('true');
    expect(storage.getItem('PP_usePointGuardNames')).toBe('false');
    expect(storage.getItem('PP_commandPaletteOrder')).toBe('["codex","claude"]');
    expect(storage.getItem('PP_gridGap')).toBe('4');
    const reloaded = createSettingsStore({ storage }).getState();
    expect(reloaded.restoreProjectsOnLaunch).toBe(false);
    expect(reloaded.launchAtLogin).toBe(true);
    expect(reloaded.usePointGuardNames).toBe(false);
    expect(reloaded.commandPaletteOrder).toEqual(['codex', 'claude']);
    expect(reloaded.gridGap).toBe(4);
  });

  test('given palette preferences, should share them with CommandPalettePreferences', () => {
    const storage = memoryStorage();
    createSettingsStore({ storage }).getState().setUsePointGuardNames(false);
    createSettingsStore({ storage }).getState().setCommandPaletteOrder(['opencode']);
    const prefs = new CommandPalettePreferences(storage);
    expect(prefs.usePointGuardNames).toBe(false);
    expect(prefs.commandPaletteOrder).toEqual(['opencode']);
  });

  test('given a notification toggle, should apply to the next alert through its key', () => {
    const storage = memoryStorage();
    const store = createSettingsStore({ storage });
    store.getState().setSignalNotifications('completion', true);
    store.getState().setSignalNotifications('permission', false);
    expect(signalNotificationsEnabled(storage, 'completion')).toBe(true);
    expect(signalNotificationsEnabled(storage, 'permission')).toBe(false);
    expect(store.getState().signalNotifications.completion).toBe(true);
  });

  test('given a bad saved grid gap, should fall back like Swift', () => {
    const storage = memoryStorage();
    storage.setItem('PP_gridGap', '-3');
    expect(loadSettings(storage).gridGap).toBe(1);
  });

  test('given a palette move, should swap neighbours and persist', () => {
    const storage = memoryStorage();
    const store = createSettingsStore({ storage });
    const items = [CLAUDE, CODEX, OPENCODE].map(builtInItem);
    const codex = itemId(builtInItem(CODEX));
    const claude = itemId(builtInItem(CLAUDE));
    store.getState().moveCommandPaletteItem(codex, -1, items);
    expect(store.getState().commandPaletteOrder.slice(0, 2)).toEqual([codex, claude]);
    expect(storage.getItem('PP_commandPaletteOrder')).toContain(codex);
    // Out of range: unchanged.
    const before = store.getState().commandPaletteOrder;
    store.getState().moveCommandPaletteItem(claude, -1, items);
    expect(store.getState().commandPaletteOrder).toBe(before);
  });

  test('given appearance and font size, should hand them to their stores', () => {
    const setAppearance = vi.fn();
    const setTerminalFontSize = vi.fn();
    const store = createSettingsStore({ storage: null, setAppearance, setTerminalFontSize });
    store.getState().setAppearance('dark');
    store.getState().setTerminalFontSize(16);
    expect(setAppearance).toHaveBeenCalledWith('dark');
    expect(setTerminalFontSize).toHaveBeenCalledWith(16);
  });
});
