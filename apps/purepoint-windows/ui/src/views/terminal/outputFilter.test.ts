// TerminalOutputFilterTests.swift, decided per filter (WIN-7.3). The Mac filters PTY output
// before SwiftTerm sees it (`DaemonAttachSession.filterTerminalOutput`):
// - DEC 2026 synchronized output: SwiftTerm mishandled `CSI ? 2026 h/l` (SwiftTerm#203), so the
//   Mac strips the brackets. xterm.js 6 implements mode 2026 (it holds rendering until `l`), so
//   the bytes go through unchanged. Not ported.
// - Cursor-up clamp: SwiftTerm moved `CSI n A` past row 0, so the Mac clamps n to the viewport.
//   xterm.js clamps cursor-up at the top margin itself. Not ported.
// These tests run the Swift cases' inputs through xterm.js (headless: the same parser and buffer
// as the app's) and assert that it already renders what the filtered output would.
import { Terminal } from '@xterm/headless';
import { describe, expect, it } from 'vitest';

function terminal(rows = 24): Terminal {
  return new Terminal({ cols: 40, rows, allowProposedApi: true });
}

function write(term: Terminal, data: string): Promise<void> {
  return new Promise((resolve) => {
    term.write(data, resolve);
  });
}

function line(term: Terminal, y: number): string {
  return term.buffer.active.getLine(y)?.translateToString(true) ?? '';
}

describe('TerminalOutputFilterTests (xterm.js needs no filter)', () => {
  it('plainTextPassesThroughUnchanged', async () => {
    const term = terminal();
    await write(term, 'hello world\r\n');
    expect(line(term, 0)).toBe('hello world');
  });

  it('stripsSynchronizedOutputBeginAndEnd: xterm.js handles DEC 2026 itself', async () => {
    const term = terminal();
    await write(term, 'a\x1b[?2026hb');
    expect(term.modes.synchronizedOutputMode).toBe(true);
    await write(term, '\x1b[?2026lc');
    expect(term.modes.synchronizedOutputMode).toBe(false);
    expect(line(term, 0)).toBe('abc');
  });

  it('keepsOtherPrivateModes', async () => {
    const term = terminal();
    await write(term, '\x1b[?2004h\x1b[?1049h');
    expect(term.modes.bracketedPasteMode).toBe(true);
    expect(term.buffer.active.type).toBe('alternate');
  });

  it('clampsCursorUpToViewport: xterm.js clamps at the top row', async () => {
    const term = terminal(10);
    await write(term, '\r\n'.repeat(5) + 'x\x1b[100Ay');
    expect(term.buffer.active.cursorY).toBe(0);
    expect(line(term, 0)).toBe(' y');
    expect(line(term, 5)).toBe('x');
  });

  it('leavesSmallCursorUpAlone', async () => {
    const term = terminal(10);
    await write(term, '\r\n'.repeat(5) + '\x1b[3A');
    expect(term.buffer.active.cursorY).toBe(2);
  });

  it('leavesOtherCSISequencesAlone', async () => {
    const term = terminal();
    await write(term, '\x1b[31mred\x1b[0m\x1b[2B');
    expect(line(term, 0)).toBe('red');
    expect(term.buffer.active.getLine(0)?.getCell(0)?.getFgColor()).toBe(1);
    expect(term.buffer.active.cursorY).toBe(2);
  });

  it('handlesTruncatedSequenceAtChunkEnd: the parser carries state across writes', async () => {
    const term = terminal();
    await write(term, 'x\x1b[?202');
    await write(term, '6hy\x1b');
    await write(term, '[?2026lz');
    expect(line(term, 0)).toBe('xyz');
    expect(term.modes.synchronizedOutputMode).toBe(false);
  });
});
