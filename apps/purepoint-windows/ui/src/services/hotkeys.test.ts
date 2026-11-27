import { afterEach, describe, expect, it, vi } from 'vitest';
import { onToggleChatSidebar } from '../views/chat/chatSidebarEvents.ts';
import {
  HOTKEY_ACTIONS,
  type HotkeyAction,
  defaultBinding,
  displayString,
} from '../model/keyBinding.ts';
import { createKeyBindingStore } from '../state/keyBindingStore.ts';
import {
  type AppActionDeps,
  PANE_COMMANDS,
  PENDING_ACTIONS,
  type PaneActions,
  createAppActions,
} from '../views/shell/appCommands.ts';
import { hotkeyActionFor, isTerminalTarget, startHotkeyMonitor } from './hotkeys.ts';

function chord(
  code: string,
  mods: { ctrl?: boolean; alt?: boolean; shift?: boolean } = {},
  target: EventTarget = document.body,
  repeat = false,
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    code,
    repeat,
    ctrlKey: mods.ctrl === true,
    altKey: mods.alt === true,
    shiftKey: mods.shift === true,
    bubbles: true,
    cancelable: true,
  });
  Object.defineProperty(event, 'target', { value: target });
  return event;
}

function eventFor(
  action: HotkeyAction,
  target: EventTarget = document.body,
  repeat = false,
): KeyboardEvent {
  const binding = defaultBinding(action);
  return chord(
    binding.code,
    {
      ctrl: binding.modifiers.includes('ctrl'),
      alt: binding.modifiers.includes('alt'),
      shift: binding.modifiers.includes('shift'),
    },
    target,
    repeat,
  );
}

function terminalTextarea(): HTMLTextAreaElement {
  const host = document.createElement('div');
  host.className = 'xterm';
  const textarea = document.createElement('textarea');
  host.append(textarea);
  document.body.append(host);
  return textarea;
}

afterEach(() => {
  document.body.innerHTML = '';
});

describe('hotkeyActionFor', () => {
  it('given each default chord, should name its action', () => {
    const bindings = createKeyBindingStore(null).getState();
    for (const action of HOTKEY_ACTIONS) {
      expect(
        hotkeyActionFor(eventFor(action), bindings),
        displayString(defaultBinding(action)),
      ).toBe(action);
    }
  });

  it('given a focused terminal, should leave it plain Ctrl+letter but still take app chords', () => {
    const textarea = terminalTextarea();
    expect(isTerminalTarget(textarea)).toBe(true);
    const bindings = createKeyBindingStore(null).getState();
    for (const code of ['KeyR', 'KeyC', 'KeyD', 'KeyL', 'KeyZ', 'KeyW', 'KeyN', 'KeyP', 'KeyF']) {
      expect(hotkeyActionFor(chord(code, { ctrl: true }, textarea), bindings)).toBeNull();
    }
    expect(hotkeyActionFor(eventFor('toggleChatSidebar', textarea), bindings)).toBe(
      'toggleChatSidebar',
    );
    expect(hotkeyActionFor(eventFor('focusLeft', textarea), bindings)).toBe('focusLeft');
  });

  it('given a custom binding on a reserved chord, should still give it to the terminal', () => {
    const store = createKeyBindingStore(null);
    store.getState().setBinding({ code: 'KeyR', modifiers: ['ctrl'] }, 'newTab');
    const textarea = terminalTextarea();
    expect(hotkeyActionFor(chord('KeyR', { ctrl: true }, textarea), store.getState())).toBeNull();
    expect(hotkeyActionFor(chord('KeyR', { ctrl: true }), store.getState())).toBe('newTab');
  });

  it('given a recording or a key a control handled, should take nothing', () => {
    const store = createKeyBindingStore(null);
    store.getState().setRecordingAction('newTab');
    expect(hotkeyActionFor(eventFor('newTab'), store.getState())).toBeNull();
    store.getState().setRecordingAction(null);
    const handled = eventFor('newTab');
    handled.preventDefault();
    expect(hotkeyActionFor(handled, store.getState())).toBeNull();
  });

  it('given browser chords, should not claim them for the app', () => {
    const bindings = createKeyBindingStore(null).getState();
    for (const event of [
      chord('F5'),
      chord('F12'),
      chord('KeyR', { ctrl: true }),
      chord('KeyP', { ctrl: true }),
      chord('KeyF', { ctrl: true }),
      chord('KeyI', { ctrl: true, shift: true }),
      chord('Equal', { ctrl: true }),
    ]) {
      expect(hotkeyActionFor(event, bindings)).toBeNull();
    }
  });
});

describe('startHotkeyMonitor', () => {
  it('given an app chord, should consume it and run its action until stopped', () => {
    const run = vi.fn();
    const store = createKeyBindingStore(null);
    const stop = startHotkeyMonitor({ bindings: () => store.getState(), run });
    const event = eventFor('splitRight');
    window.dispatchEvent(event);
    expect(run).toHaveBeenCalledWith('splitRight');
    expect(event.defaultPrevented).toBe(true);

    const plain = chord('KeyA', { ctrl: true });
    window.dispatchEvent(plain);
    expect(plain.defaultPrevented).toBe(false);
    expect(run).toHaveBeenCalledTimes(1);

    stop();
    window.dispatchEvent(eventFor('splitRight'));
    expect(run).toHaveBeenCalledTimes(1);
  });

  it('given a held destructive or creating chord, should run it once and consume its repeats', () => {
    const run = vi.fn();
    const store = createKeyBindingStore(null);
    const stop = startHotkeyMonitor({ bindings: () => store.getState(), run });
    for (const action of [
      'closeAgent',
      'closePane',
      'newTab',
      'splitRight',
      'splitBelow',
    ] as const) {
      run.mockClear();
      window.dispatchEvent(eventFor(action));
      for (let i = 0; i < 3; i++) {
        const repeat = eventFor(action, document.body, true);
        window.dispatchEvent(repeat);
        expect(repeat.defaultPrevented, action).toBe(true);
      }
      expect(run, action).toHaveBeenCalledTimes(1);
    }
    stop();
  });

  it('given a held navigation chord, should repeat it', () => {
    const run = vi.fn();
    const store = createKeyBindingStore(null);
    const stop = startHotkeyMonitor({ bindings: () => store.getState(), run });
    for (const action of ['nextTab', 'previousTab', 'focusLeft'] as const) {
      run.mockClear();
      window.dispatchEvent(eventFor(action));
      window.dispatchEvent(eventFor(action, document.body, true));
      window.dispatchEvent(eventFor(action, document.body, true));
      expect(run, action).toHaveBeenCalledTimes(3);
    }
    stop();
  });

  it('given a held reserved chord in a terminal, should still leave it to the terminal', () => {
    const run = vi.fn();
    const store = createKeyBindingStore(null);
    const stop = startHotkeyMonitor({ bindings: () => store.getState(), run });
    const repeat = chord('KeyC', { ctrl: true }, terminalTextarea(), true);
    window.dispatchEvent(repeat);
    expect(repeat.defaultPrevented).toBe(false);
    expect(run).not.toHaveBeenCalled();
    stop();
  });

  it('given a rebinding, should apply it live', () => {
    const run = vi.fn();
    const store = createKeyBindingStore(null);
    const stop = startHotkeyMonitor({ bindings: () => store.getState(), run });
    store.getState().setBinding({ code: 'KeyY', modifiers: ['alt'] }, 'navAgents');
    window.dispatchEvent(chord('KeyY', { alt: true }));
    expect(run).toHaveBeenCalledWith('navAgents');
    stop();
  });
});

describe('every hotkey action', () => {
  it('given its chord, should reach the shell, the pane grid or the chat sidebar', () => {
    // Toggle Chat Sidebar is posted to the Point Guard surfaces on screen (WIN-9.5).
    const chatSidebar = vi.fn();
    const stopChatSidebar = onToggleChatSidebar(chatSidebar);
    const pane = Object.fromEntries(PANE_COMMANDS.map((command) => [command, vi.fn()])) as Record<
      (typeof PANE_COMMANDS)[number],
      ReturnType<typeof vi.fn>
    >;
    const deps = {
      selectNav: vi.fn(),
      toggleSidebar: vi.fn(),
      showSettings: vi.fn(),
      openProject: vi.fn(),
      focusSidebar: vi.fn(),
      focusContent: vi.fn(),
      closeWindow: vi.fn(),
      newAgent: vi.fn(),
      checkForUpdates: vi.fn(),
      pane: pane as unknown as PaneActions,
    } satisfies AppActionDeps;
    const actions = createAppActions(deps);
    const spies = [
      ...Object.values(deps).filter((v) => typeof v === 'function'),
      ...Object.values(pane),
      chatSidebar,
    ];
    const bindings = createKeyBindingStore(null).getState();
    const pending: string[] = [];
    for (const action of HOTKEY_ACTIONS) {
      const calls = () => spies.reduce((sum, spy) => sum + spy.mock.calls.length, 0);
      const before = calls();
      const chosen = hotkeyActionFor(eventFor(action), bindings);
      expect(chosen).toBe(action);
      if (chosen !== null) actions[chosen]();
      if (calls() === before) pending.push(action);
    }
    stopChatSidebar();
    expect(pending).toEqual(Object.keys(PENDING_ACTIONS));
    expect(pending).toEqual([]);
  });
});
