import { describe, expect, it, vi } from 'vitest';
import { HOTKEY_ACTIONS } from '../../model/keyBinding.ts';
import { defaultBindings } from '../../state/keyBindingStore.ts';
import {
  APP_MENUS,
  PENDING_ACTIONS,
  acceleratorLabel,
  commandLabel,
  createAppActions,
  menuCommands,
  type AppActionDeps,
} from './appCommands.ts';

function deps(): AppActionDeps {
  return {
    selectNav: vi.fn(),
    toggleSidebar: vi.fn(),
    showSettings: vi.fn(),
    focusSidebar: vi.fn(),
    focusContent: vi.fn(),
    closeWindow: vi.fn(),
  };
}

describe('app menus', () => {
  it('list every hotkey action once, in the Mac menu groups', () => {
    const commands = menuCommands();
    for (const action of HOTKEY_ACTIONS) expect(commands).toContain(action);
    expect(new Set(commands).size).toBe(commands.length);
    expect(APP_MENUS.map((menu) => menu.title)).toEqual([
      'File',
      'View',
      'Navigation',
      'Panes',
      'Tabs',
      'Help',
    ]);
  });

  it('label commands as the Mac does, with Windows accelerators', () => {
    const bindings = defaultBindings();
    expect(commandLabel('newAgent')).toBe('New Agent…');
    expect(commandLabel('settings')).toBe('Settings…');
    expect(commandLabel('closeAgent')).toBe('Close Tab');
    expect(commandLabel('selectTab3')).toBe('Go to Tab 3');
    expect(commandLabel('exit')).toBe('Exit');
    expect(acceleratorLabel('newAgent', bindings)).toBe('Ctrl+Shift+N');
    expect(acceleratorLabel('settings', bindings)).toBe('Ctrl+,');
    expect(acceleratorLabel('splitBelow', bindings)).toBe('Alt+Shift+-');
    expect(acceleratorLabel('exit', bindings)).toBe('Alt+F4');
    expect(acceleratorLabel('checkForUpdates', bindings)).toBeNull();
  });
});

describe('app actions', () => {
  it('dispatch every command', () => {
    const d = deps();
    const actions = createAppActions(d);
    for (const command of menuCommands()) expect(typeof actions[command]).toBe('function');
    actions.navAgents();
    actions.toggleWorkspaceSidebar();
    actions.settings();
    actions.focusContent();
    actions.focusSidebar();
    actions.exit();
    expect(d.selectNav).toHaveBeenCalledWith('agents');
    expect(d.toggleSidebar).toHaveBeenCalledWith('toggleWorkspaceSidebar');
    expect(d.showSettings).toHaveBeenCalledOnce();
    expect(d.focusContent).toHaveBeenCalledOnce();
    expect(d.focusSidebar).toHaveBeenCalledOnce();
    expect(d.closeWindow).toHaveBeenCalledOnce();
  });

  it('keep the commands of unported screens as named no-ops', () => {
    const actions = createAppActions(deps());
    for (const [command, stub] of Object.entries(PENDING_ACTIONS)) {
      expect(stub.name).not.toBe('');
      expect(actions[command as keyof typeof PENDING_ACTIONS]).toBe(stub);
      stub();
    }
    expect(PENDING_ACTIONS.newAgent.name).toBe('newAgent');
  });
});
