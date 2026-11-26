import { beforeEach, describe, expect, it } from 'vitest';
import {
  HOTKEY_ACTIONS,
  type HotkeyAction,
  type KeyBinding,
  chordId,
  defaultBinding,
  keyBinding,
} from '../model/keyBinding.ts';
import {
  KEY_BINDINGS_STORAGE_KEY,
  createKeyBindingStore,
  defaultBindings,
  resolve,
} from './keyBindingStore.ts';

/**
 * A saved shortcut always wins, and no two actions ever end up sharing one — even when a new
 * action's default is a shortcut the user had already given to something else.
 * (Port of KeyBindingStateTests.swift with the DEC-8 Windows chords.)
 */
describe('KeyBindingStateTests', () => {
  const workspaceToggle = keyBinding('Backslash', 'alt', 'shift');
  const chatToggle = keyBinding('KeyS', 'ctrl', 'shift');

  function expectNoSharedBindings(bindings: Partial<Record<HotkeyAction, KeyBinding>>) {
    const values = Object.values(bindings);
    expect(new Set(values.map(chordId)).size).toBe(values.length);
  }

  it('givenTheDefaultsShouldGiveEveryActionItsOwnShortcut', () => {
    const defaults = defaultBindings();

    expect(Object.keys(defaults)).toHaveLength(HOTKEY_ACTIONS.length);
    expectNoSharedBindings(defaults);
  });

  it('givenNoSavedShortcutsShouldUseEveryDefault', () => {
    const resolved = resolve(defaultBindings(), {});

    expect(resolved).toEqual(defaultBindings());
  });

  it('givenASavedShortcutOnANewActionsDefaultShouldKeepItAndMoveTheNewActionOff', () => {
    const resolved = resolve(defaultBindings(), { toggleChatSidebar: workspaceToggle });

    expect(resolved.toggleChatSidebar).toEqual(workspaceToggle);
    expect(resolved.toggleWorkspaceSidebar).toEqual(chatToggle);
    expect(Object.keys(resolved)).toHaveLength(HOTKEY_ACTIONS.length);
    expectNoSharedBindings(resolved);
  });

  it('givenChainedSavedShortcutsShouldStillGiveEveryActionItsOwnShortcut', () => {
    const newAgentDefault = defaultBinding('newAgent');
    const openProjectDefault = defaultBinding('openProject');

    const resolved = resolve(defaultBindings(), {
      newAgent: workspaceToggle,
      openProject: newAgentDefault,
    });

    expect(resolved.newAgent).toEqual(workspaceToggle);
    expect(resolved.openProject).toEqual(newAgentDefault);
    expect(resolved.toggleWorkspaceSidebar).toEqual(openProjectDefault);
    expect(Object.keys(resolved)).toHaveLength(HOTKEY_ACTIONS.length);
    expectNoSharedBindings(resolved);
  });

  it('givenASavedShortcutThatIsNobodysDefaultShouldLeaveTheOtherDefaultsAlone', () => {
    // Swift uses ⌃⌘K; Ctrl+Alt is AltGr on Windows, so the Windows case uses Ctrl+Shift+K.
    const custom = keyBinding('KeyK', 'ctrl', 'shift');

    const resolved = resolve(defaultBindings(), { toggleWorkspaceSidebar: custom });

    const expected = { ...defaultBindings(), toggleWorkspaceSidebar: custom };
    expect(resolved).toEqual(expected);
  });
});

describe('keyBindingStore', () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it('starts on the defaults with nothing customized', () => {
    const store = createKeyBindingStore(localStorage).getState();
    expect(store.bindings).toEqual(defaultBindings());
    expect(store.isCustomized('newAgent')).toBe(false);
    expect(store.recordingAction).toBeNull();
  });

  it('persists a custom binding per user and loads it back', () => {
    const custom = keyBinding('KeyK', 'ctrl', 'shift');
    const store = createKeyBindingStore(localStorage);
    store.getState().setBinding(custom, 'newAgent');

    expect(store.getState().binding('newAgent')).toEqual(custom);
    expect(store.getState().isCustomized('newAgent')).toBe(true);
    expect(JSON.parse(localStorage.getItem(KEY_BINDINGS_STORAGE_KEY) ?? '')).toEqual({
      newAgent: { code: 'KeyK', modifiers: ['ctrl', 'shift'] },
    });

    const reloaded = createKeyBindingStore(localStorage).getState();
    expect(reloaded.binding('newAgent')).toEqual(custom);
    expect(reloaded.isCustomized('newAgent')).toBe(true);
  });

  it('skips unknown actions and malformed saved data', () => {
    localStorage.setItem(
      KEY_BINDINGS_STORAGE_KEY,
      JSON.stringify({
        removedAction: { code: 'KeyK', modifiers: ['ctrl'] },
        newTab: { code: 'KeyY', modifiers: ['command'] },
        settings: { code: 'KeyP', modifiers: ['ctrl', 'shift'] },
      }),
    );
    const store = createKeyBindingStore(localStorage).getState();
    expect(store.overrides).toEqual({ settings: keyBinding('KeyP', 'ctrl', 'shift') });

    localStorage.setItem(KEY_BINDINGS_STORAGE_KEY, '{not json');
    expect(createKeyBindingStore(localStorage).getState().bindings).toEqual(defaultBindings());
  });

  it('detects a conflict with another action but not with the action itself', () => {
    const store = createKeyBindingStore(localStorage).getState();
    expect(store.detectConflict(defaultBinding('newTab'), 'newAgent')).toBe('newTab');
    expect(store.detectConflict(defaultBinding('newTab'), 'newTab')).toBeNull();
    expect(store.detectConflict(keyBinding('KeyK', 'ctrl', 'shift'), 'newAgent')).toBeNull();
  });

  it('swaps a displaced default when a binding takes another action’s chord', () => {
    const store = createKeyBindingStore(localStorage);
    store.getState().setBinding(defaultBinding('newTab'), 'newAgent');

    const { bindings } = store.getState();
    expect(bindings.newAgent).toEqual(defaultBinding('newTab'));
    expect(bindings.newTab).toEqual(defaultBinding('newAgent'));
  });

  it('resets one binding and then all bindings', () => {
    const store = createKeyBindingStore(localStorage);
    store.getState().setBinding(keyBinding('KeyK', 'ctrl', 'shift'), 'newAgent');
    store.getState().setBinding(keyBinding('KeyJ', 'ctrl', 'shift'), 'newTab');

    store.getState().resetBinding('newAgent');
    expect(store.getState().binding('newAgent')).toEqual(defaultBinding('newAgent'));
    expect(store.getState().isCustomized('newTab')).toBe(true);
    expect(createKeyBindingStore(localStorage).getState().overrides).toEqual({
      newTab: keyBinding('KeyJ', 'ctrl', 'shift'),
    });

    store.getState().resetAllBindings();
    expect(store.getState().bindings).toEqual(defaultBindings());
    expect(store.getState().overrides).toEqual({});
    expect(localStorage.getItem(KEY_BINDINGS_STORAGE_KEY)).toBe('{}');
  });

  it('looks up the action for a keydown with the current bindings', () => {
    const store = createKeyBindingStore(null);
    const event = {
      code: 'KeyK',
      ctrlKey: true,
      altKey: false,
      shiftKey: true,
      metaKey: false,
    };
    expect(store.getState().actionFor(event, false)).toBeNull();
    store.getState().setBinding(keyBinding('KeyK', 'ctrl', 'shift'), 'newAgent');
    expect(store.getState().actionFor(event, false)).toBe('newAgent');
  });

  it('tracks the action being recorded', () => {
    const store = createKeyBindingStore(null);
    store.getState().setRecordingAction('splitRight');
    expect(store.getState().recordingAction).toBe('splitRight');
  });
});
