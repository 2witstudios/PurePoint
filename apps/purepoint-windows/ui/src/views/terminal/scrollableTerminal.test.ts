// ScrollableTerminal's pure rules (WIN-7.3): the local/reported grid split, the terminal's key
// rules (REQ-WINAPP-068) and which links Ctrl+click may open.
import { describe, expect, it } from 'vitest';
import { actionForEvent, type KeyChordEvent } from '../../model/keyBinding.ts';
import {
  DEFAULT_GRID,
  isOpenableUrl,
  TerminalGeometry,
  terminalKeyAction,
} from './scrollableTerminal.ts';

function key(code: string, mods: Partial<Omit<KeyChordEvent, 'code'>> = {}): KeyChordEvent {
  return { code, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...mods };
}

const defaults = (event: KeyChordEvent) => actionForEvent(event, true) !== null;

describe('TerminalGeometry', () => {
  // DaemonClientParseTests.testReportedSizeDoesNotReplaceLocalWindowDimensions
  it('testReportedSizeDoesNotReplaceLocalWindowDimensions', () => {
    const geometry = new TerminalGeometry();
    expect(geometry.render).toEqual(DEFAULT_GRID);
    expect(geometry.measure({ cols: 100, rows: 30 })).toBe(true);
    const local = geometry.local;
    geometry.report({ cols: 70, rows: 20 });
    expect(geometry.local).toEqual(local);
    expect(geometry.render).toEqual({ cols: 70, rows: 20 });
    // A wider pane changes the local size only; the daemon's grid still renders.
    expect(geometry.measure({ cols: 130, rows: 30 })).toBe(true);
    expect(geometry.local.cols).toBeGreaterThan(local.cols);
    expect(geometry.render).toEqual({ cols: 70, rows: 20 });
    // A bigger font fits fewer columns.
    expect(geometry.measure({ cols: 90, rows: 25 })).toBe(true);
    expect(geometry.local.cols).toBeLessThan(130);
    expect(geometry.render).toEqual({ cols: 70, rows: 20 });
  });

  it('ignores unchanged and empty measurements', () => {
    const geometry = new TerminalGeometry();
    expect(geometry.measure({ cols: 80, rows: 24 })).toBe(true);
    expect(geometry.measure({ cols: 80, rows: 24 })).toBe(false);
    expect(geometry.measure({ cols: 0, rows: 24 })).toBe(false);
    expect(geometry.local).toEqual({ cols: 80, rows: 24 });
  });
});

describe('terminalKeyAction', () => {
  const ctrlShift = { ctrlKey: true, shiftKey: true };

  it('copies on Ctrl+Shift+C, and on Ctrl+C only with a selection', () => {
    expect(terminalKeyAction(key('KeyC', ctrlShift), false, defaults)).toBe('copy');
    expect(terminalKeyAction(key('KeyC', { ctrlKey: true }), true, defaults)).toBe('copy');
    expect(terminalKeyAction(key('KeyC', { ctrlKey: true }), false, defaults)).toBe('terminal');
  });

  it('pastes on Ctrl+Shift+V', () => {
    expect(terminalKeyAction(key('KeyV', ctrlShift), false, defaults)).toBe('paste');
  });

  it('leaves plain Ctrl+letter to the terminal', () => {
    for (const code of ['KeyD', 'KeyR', 'KeyL', 'KeyZ', 'KeyV', 'KeyW', 'KeyN']) {
      expect(terminalKeyAction(key(code, { ctrlKey: true }), false, defaults)).toBe('terminal');
    }
    expect(terminalKeyAction(key('KeyA'), false, defaults)).toBe('terminal');
    expect(terminalKeyAction(key('ArrowUp'), false, defaults)).toBe('terminal');
  });

  it('hands app chords to the app', () => {
    expect(terminalKeyAction(key('Backslash', ctrlShift), false, defaults)).toBe('app');
    expect(terminalKeyAction(key('ArrowUp', { altKey: true }), false, defaults)).toBe('app');
    expect(terminalKeyAction(key('KeyN', ctrlShift), false, defaults)).toBe('app');
  });
});

describe('isOpenableUrl', () => {
  it('opens web and mail links only', () => {
    expect(isOpenableUrl('https://example.com/a;b?q=1%202')).toBe(true);
    expect(isOpenableUrl('http://localhost:3000')).toBe(true);
    expect(isOpenableUrl('mailto:someone@example.com')).toBe(true);
    expect(isOpenableUrl('file:///C:/Windows/system32/calc.exe')).toBe(false);
    expect(isOpenableUrl('javascript:alert(1)')).toBe(false);
    expect(isOpenableUrl('not a url')).toBe(false);
  });
});
