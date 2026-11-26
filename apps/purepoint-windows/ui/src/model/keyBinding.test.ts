import { describe, expect, it } from 'vitest';
import {
  HOTKEY_ACTIONS,
  HOTKEY_CATEGORIES,
  type HotkeyAction,
  type KeyChordEvent,
  type KeyModifier,
  actionCategory,
  actionDisplayName,
  actionForEvent,
  actionsFor,
  bindingFromEvent,
  chordId,
  decodeBinding,
  defaultBinding,
  displayString,
  encodeBinding,
  isAppChord,
  keyBinding,
  matches,
  reservedTerminalChords,
  tabIndex,
} from './keyBinding.ts';

function press(code: string, ...mods: KeyModifier[]): KeyChordEvent {
  return {
    code,
    ctrlKey: mods.includes('ctrl'),
    altKey: mods.includes('alt'),
    shiftKey: mods.includes('shift'),
    metaKey: mods.includes('meta'),
    getModifierState: () => false,
  };
}

const DEC8_TABLE: Record<HotkeyAction, string> = {
  newAgent: 'Ctrl+Shift+N',
  openProject: 'Ctrl+Shift+O',
  settings: 'Ctrl+,',
  closeAgent: 'Ctrl+Shift+W',
  focusSidebar: 'Ctrl+Shift+[',
  focusContent: 'Ctrl+Shift+]',
  toggleSidebar: 'Ctrl+Shift+\\',
  toggleWorkspaceSidebar: 'Alt+Shift+\\',
  navDashboard: 'Alt+Shift+1',
  navAgents: 'Alt+Shift+2',
  navSchedule: 'Alt+Shift+3',
  splitBelow: 'Alt+Shift+-',
  splitRight: 'Alt+Shift+=',
  closePane: 'Alt+Shift+W',
  focusUp: 'Alt+Up',
  focusDown: 'Alt+Down',
  focusLeft: 'Alt+Left',
  focusRight: 'Alt+Right',
  newTab: 'Ctrl+Shift+T',
  nextTab: 'Ctrl+Tab',
  previousTab: 'Ctrl+Shift+Tab',
  selectTab1: 'Ctrl+Shift+1',
  selectTab2: 'Ctrl+Shift+2',
  selectTab3: 'Ctrl+Shift+3',
  selectTab4: 'Ctrl+Shift+4',
  selectTab5: 'Ctrl+Shift+5',
  selectTab6: 'Ctrl+Shift+6',
  selectTab7: 'Ctrl+Shift+7',
  selectTab8: 'Ctrl+Shift+8',
  selectLastTab: 'Ctrl+Shift+9',
  breakTab: 'Alt+Shift+D',
  toggleChatSidebar: 'Ctrl+Shift+S',
};

describe('HotkeyAction', () => {
  it('has the 32 Mac actions in five categories', () => {
    expect(HOTKEY_ACTIONS).toHaveLength(32);
    expect(HOTKEY_CATEGORIES).toEqual(['application', 'navigation', 'panes', 'tabs', 'chat']);
    expect(HOTKEY_CATEGORIES.flatMap(actionsFor).sort()).toEqual([...HOTKEY_ACTIONS].sort());
    expect(actionCategory('closeAgent')).toBe('tabs');
    expect(actionsFor('chat')).toEqual(['toggleChatSidebar']);
  });

  it('names the tab actions by position', () => {
    expect(actionDisplayName('selectTab3')).toBe('Go to Tab 3');
    expect(tabIndex('selectTab1')).toBe(0);
    expect(tabIndex('selectTab8')).toBe(7);
    expect(tabIndex('selectLastTab')).toBeNull();
  });
});

describe('default bindings (DEC-8)', () => {
  it('given key bindings, should map every Mac hotkey to a Windows chord', () => {
    for (const action of HOTKEY_ACTIONS) {
      expect([action, displayString(defaultBinding(action))]).toEqual([action, DEC8_TABLE[action]]);
    }
  });

  it('has no duplicate chord', () => {
    const ids = HOTKEY_ACTIONS.map((a) => chordId(defaultBinding(a)));
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('never uses Ctrl+Alt together (AltGr) or the Win key', () => {
    for (const action of HOTKEY_ACTIONS) {
      const mods = defaultBinding(action).modifiers;
      expect(mods.includes('ctrl') && mods.includes('alt')).toBe(false);
      expect(mods).not.toContain('meta');
    }
  });

  it('uses KeyboardEvent.code keys', () => {
    expect(defaultBinding('newAgent').code).toBe('KeyN');
    expect(defaultBinding('selectTab1').code).toBe('Digit1');
    expect(defaultBinding('focusSidebar').code).toBe('BracketLeft');
    expect(defaultBinding('toggleSidebar').code).toBe('Backslash');
    expect(defaultBinding('splitBelow').code).toBe('Minus');
    expect(defaultBinding('splitRight').code).toBe('Equal');
    expect(defaultBinding('settings').code).toBe('Comma');
    expect(defaultBinding('nextTab').code).toBe('Tab');
    expect(defaultBinding('focusUp').code).toBe('ArrowUp');
  });
});

describe('matching', () => {
  it('matches on the physical key and exact modifiers', () => {
    const newAgent = defaultBinding('newAgent');
    expect(matches(newAgent, press('KeyN', 'ctrl', 'shift'))).toBe(true);
    expect(matches(newAgent, press('KeyN', 'ctrl'))).toBe(false);
    expect(matches(newAgent, press('KeyN', 'ctrl', 'shift', 'alt'))).toBe(false);
    expect(matches(newAgent, press('KeyM', 'ctrl', 'shift'))).toBe(false);
  });

  it('ignores the layout: a code match is a match whatever character the key types', () => {
    // On AZERTY the key at `Digit1` types "&"; `code` is still Digit1.
    const event = { ...press('Digit1', 'ctrl', 'shift'), key: '&' };
    expect(actionForEvent(event, false)).toBe('selectTab1');
  });

  it('never matches an AltGr press', () => {
    const altGr: KeyChordEvent = {
      ...press('KeyD', 'ctrl', 'alt'),
      getModifierState: (k) => k === 'AltGraph',
    };
    expect(matches(keyBinding('KeyD', 'ctrl', 'alt'), altGr)).toBe(false);
    expect(bindingFromEvent(altGr)).toBeNull();
  });

  it('records chords that include Ctrl, Alt or Win', () => {
    expect(bindingFromEvent(press('KeyK', 'shift', 'ctrl'))).toEqual(
      keyBinding('KeyK', 'ctrl', 'shift'),
    );
    expect(bindingFromEvent(press('KeyK', 'shift'))).toBeNull();
    expect(bindingFromEvent(press('KeyK'))).toBeNull();
    expect(bindingFromEvent(press('ControlLeft', 'ctrl'))).toBeNull();
  });
});

describe('terminal pass-through', () => {
  it('reserves plain Ctrl+letter', () => {
    for (const code of ['KeyC', 'KeyD', 'KeyR', 'KeyL', 'KeyZ']) {
      expect(reservedTerminalChords.has(chordId(keyBinding(code, 'ctrl')))).toBe(true);
    }
    expect(reservedTerminalChords.has(chordId(keyBinding('KeyC', 'ctrl', 'shift')))).toBe(false);
  });

  it('never captures Ctrl+C, Ctrl+D, Ctrl+R, Ctrl+L, Ctrl+Z while a terminal has focus', () => {
    const bindings = Object.fromEntries(
      HOTKEY_ACTIONS.map((a) => [a, defaultBinding(a)]),
    ) as Record<HotkeyAction, ReturnType<typeof defaultBinding>>;
    // Even when the user has bound them to app actions.
    bindings.newAgent = keyBinding('KeyC', 'ctrl');
    bindings.openProject = keyBinding('KeyD', 'ctrl');
    bindings.settings = keyBinding('KeyR', 'ctrl');
    bindings.closeAgent = keyBinding('KeyL', 'ctrl');
    bindings.newTab = keyBinding('KeyZ', 'ctrl');
    for (const code of ['KeyC', 'KeyD', 'KeyR', 'KeyL', 'KeyZ']) {
      expect(isAppChord(press(code, 'ctrl'), true)).toBe(false);
      expect(isAppChord(press(code, 'ctrl'), true, bindings)).toBe(false);
      expect(isAppChord(press(code, 'ctrl'), false, bindings)).toBe(true);
    }
  });

  it('captures app chords in a terminal', () => {
    expect(isAppChord(press('KeyN', 'ctrl', 'shift'), true)).toBe(true);
    expect(isAppChord(press('Tab', 'ctrl'), true)).toBe(true);
    expect(isAppChord(press('ArrowUp', 'alt'), true)).toBe(true);
    expect(isAppChord(press('KeyN', 'ctrl'), false)).toBe(false);
  });
});

describe('persistence encoding', () => {
  it('round-trips a binding', () => {
    const binding = keyBinding('Backslash', 'shift', 'alt');
    const json = JSON.parse(JSON.stringify(encodeBinding(binding))) as unknown;
    expect(json).toEqual({ code: 'Backslash', modifiers: ['alt', 'shift'] });
    expect(decodeBinding(json)).toEqual(binding);
  });

  it('rejects malformed bindings', () => {
    expect(decodeBinding(null)).toBeNull();
    expect(decodeBinding({ code: '', modifiers: [] })).toBeNull();
    expect(decodeBinding({ code: 'KeyN', modifiers: ['command'] })).toBeNull();
    expect(decodeBinding({ code: 'KeyN' })).toBeNull();
  });
});
