import { describe, expect, test } from 'vitest';
import { type KeyChordEvent, keyBinding } from '../../model/keyBinding.ts';
import { createKeyBindingStore } from '../../state/keyBindingStore.ts';
import { overrideBindings, recordKey } from './hotkeyRecorder.ts';

function key(
  code: string,
  mods: { ctrl?: boolean; alt?: boolean; shift?: boolean; meta?: boolean; altGraph?: boolean } = {},
): KeyChordEvent {
  return {
    code,
    ctrlKey: mods.ctrl ?? false,
    altKey: mods.alt ?? false,
    shiftKey: mods.shift ?? false,
    metaKey: mods.meta ?? false,
    getModifierState: (k) => k === 'AltGraph' && mods.altGraph === true,
  };
}

describe('hotkey recorder', () => {
  test('given plain Esc, should cancel', () => {
    const store = createKeyBindingStore(null).getState();
    expect(recordKey(key('Escape'), 'newAgent', store)).toEqual({ kind: 'cancel' });
  });

  test('given Ctrl+Esc, should record it rather than cancel', () => {
    const store = createKeyBindingStore(null).getState();
    expect(recordKey(key('Escape', { ctrl: true, shift: true }), 'newAgent', store).kind).toBe(
      'set',
    );
  });

  test('given a modifier alone, a bare key, Shift+key or AltGr, should keep listening', () => {
    const store = createKeyBindingStore(null).getState();
    for (const event of [
      key('ControlLeft', { ctrl: true }),
      key('ShiftLeft', { shift: true }),
      key('KeyK'),
      key('KeyK', { shift: true }),
      key('KeyQ', { ctrl: true, alt: true, altGraph: true }),
    ]) {
      expect(recordKey(event, 'newAgent', store)).toEqual({ kind: 'ignored' });
    }
  });

  test('given a free chord, should capture it by physical key code', () => {
    const store = createKeyBindingStore(null).getState();
    const result = recordKey(key('KeyY', { ctrl: true, shift: true }), 'navAgents', store);
    expect(result).toEqual({ kind: 'set', binding: keyBinding('KeyY', 'ctrl', 'shift') });
  });

  test('given a reserved terminal chord, should refuse it with a message', () => {
    const store = createKeyBindingStore(null).getState();
    for (const code of ['KeyC', 'KeyD', 'BracketLeft', 'Backslash', 'Space']) {
      const result = recordKey(key(code, { ctrl: true }), 'newAgent', store);
      expect(result.kind).toBe('reserved');
      if (result.kind === 'reserved') expect(result.message).toContain('kept for the terminal');
    }
    expect(recordKey(key('KeyC', { ctrl: true }), 'newAgent', store)).toMatchObject({
      message: 'Ctrl+C is kept for the terminal',
    });
  });

  test('given a chord another action has, should report the conflict', () => {
    const store = createKeyBindingStore(null).getState();
    const result = recordKey(key('KeyT', { ctrl: true, shift: true }), 'newAgent', store);
    expect(result).toEqual({
      kind: 'conflict',
      binding: keyBinding('KeyT', 'ctrl', 'shift'),
      action: 'newTab',
    });
  });

  test("given the action's own chord, should not report a conflict", () => {
    const store = createKeyBindingStore(null).getState();
    expect(recordKey(key('KeyN', { ctrl: true, shift: true }), 'newAgent', store).kind).toBe('set');
  });

  test('given Override, should swap the two chords so none is shared', () => {
    const store = createKeyBindingStore(null);
    overrideBindings(store.getState(), 'newAgent', 'newTab', keyBinding('KeyT', 'ctrl', 'shift'));
    const state = store.getState();
    expect(state.binding('newAgent')).toEqual(keyBinding('KeyT', 'ctrl', 'shift'));
    expect(state.binding('newTab')).toEqual(keyBinding('KeyN', 'ctrl', 'shift'));
    const ids = Object.values(state.bindings).map((b) => [...b.modifiers, b.code].join('+'));
    expect(new Set(ids).size).toBe(ids.length);
  });
});
