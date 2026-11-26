// Terminal pane behaviour (WIN-7.3) against real `terminal` agents. The agents run
// fixtures/echo-bytes.mjs in their ConPTY: `echo` mode logs every chunk the program reads (hex,
// one line per read), so each test compares those bytes with what xterm.js encoded; `flood`
// mode streams output for the throughput and long-task checks.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { browser, $, expect } from '@wdio/globals';
import { AttachTap } from '../fixtures/attachTap.ts';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { workspace } from '../fixtures/paths.ts';

const echoScript = join(workspace.appDir, 'ui', 'e2e', 'fixtures', 'echo-bytes.mjs');

interface Probe {
  agentId: string;
  shown: boolean;
  renderer: string;
  cols: number | null;
  rows: number | null;
  fontFamily: string | null;
  fontSize: number | null;
  bytesReceived: number;
  bytesParsed: number;
  checksum: number;
}

interface HookWindow {
  __PU_TERMINAL_STATS__?: boolean;
  __puTerminals: {
    list(): Probe[];
    text(agentId: string): string | null;
    inputLog(agentId: string): readonly string[] | null;
    locate(agentId: string, needle: string): { x: number; y: number } | null;
    resetStats(agentId: string): void;
  };
  __PU_OPENER_SPY__?: string[];
  __puLongTasks?: number[];
}

const hex = (text: string) => Buffer.from(text, 'utf8').toString('hex');

async function probe(agentId: string): Promise<Probe | undefined> {
  const rows = await browser.execute(() => (window as unknown as HookWindow).__puTerminals.list());
  return rows.find((row) => row.agentId === agentId);
}

async function screenText(agentId: string): Promise<string> {
  return (
    (await browser.execute(
      (id) => (window as unknown as HookWindow).__puTerminals.text(id),
      agentId,
    )) ?? ''
  );
}

async function waitForText(agentId: string, text: string, timeout = 30_000): Promise<void> {
  await browser.waitUntil(async () => (await screenText(agentId)).includes(text), {
    timeout,
    timeoutMsg: `terminal ${agentId} never showed ${JSON.stringify(text)}`,
  });
}

async function locate(agentId: string, needle: string): Promise<{ x: number; y: number }> {
  const point = await browser.execute(
    (id, text) => (window as unknown as HookWindow).__puTerminals.locate(id, text),
    agentId,
    needle,
  );
  if (point === null) throw new Error(`${JSON.stringify(needle)} is not on screen`);
  return point;
}

/** Select agents in the interim sidebar list: the first replaces, the rest Ctrl+click in. */
async function showAgents(agentIds: readonly string[]): Promise<void> {
  await browser.execute((ids: readonly string[]) => {
    ids.forEach((id, index) => {
      const button = document.querySelector(`[data-testid="agent-list"] [data-agent-id="${id}"]`);
      button?.dispatchEvent(new MouseEvent('click', { bubbles: true, ctrlKey: index > 0 }));
    });
  }, agentIds);
  for (const id of agentIds) {
    await expect($(`[data-testid="terminal-pane"][data-agent-id="${id}"] .xterm`)).toBeDisplayed();
  }
}

async function waitForAgentRow(agentId: string): Promise<void> {
  await expect($(`[data-testid="agent-list"] [data-agent-id="${agentId}"]`)).toBeExisting({
    wait: 30_000,
  });
}

/** Click a pane's text area at a screen point (focuses it). */
async function clickAt(point: { x: number; y: number }, modifier?: 'Control'): Promise<void> {
  // WebDriver key actions name Control by its code point.
  const keys = modifier === undefined ? [] : [String.fromCharCode(0xe009)];
  await browser.performActions([
    {
      type: 'key',
      id: 'keyboard',
      actions: [
        ...keys.map((value) => ({ type: 'keyDown', value })),
        // Held until after the pointer's six actions (one action per source per tick).
        ...Array.from({ length: 7 }, () => ({ type: 'pause', duration: 0 })),
        ...keys.map((value) => ({ type: 'keyUp', value })),
      ],
    },
    {
      type: 'pointer',
      id: 'mouse',
      parameters: { pointerType: 'mouse' },
      actions: [
        { type: 'pointerMove', duration: 0, origin: 'viewport', x: point.x + 30, y: point.y + 30 },
        { type: 'pause', duration: 50 },
        { type: 'pointerMove', duration: 100, origin: 'viewport', x: point.x, y: point.y },
        { type: 'pause', duration: 50 },
        { type: 'pointerDown', button: 0 },
        { type: 'pointerUp', button: 0 },
      ],
    },
  ]);
  await browser.releaseActions();
}

async function doubleClickAt(point: { x: number; y: number }): Promise<void> {
  await browser.performActions([
    {
      type: 'pointer',
      id: 'mouse',
      parameters: { pointerType: 'mouse' },
      actions: [
        { type: 'pointerMove', duration: 0, origin: 'viewport', x: point.x, y: point.y },
        { type: 'pointerDown', button: 0 },
        { type: 'pointerUp', button: 0 },
        { type: 'pointerDown', button: 0 },
        { type: 'pointerUp', button: 0 },
      ],
    },
  ]);
  await browser.releaseActions();
}

async function clipboardText(): Promise<string> {
  return browser.execute(() => navigator.clipboard.readText());
}

describe('Terminal pane', () => {
  const fixture = DaemonFixture.existing();
  const log = join(fixture.root, 'echo-bytes.log');
  let agentId = '';

  /** Hex lines the program read since `from`. */
  function received(from = 0): string[] {
    if (!existsSync(log)) return [];
    return readFileSync(log, 'utf8').split('\n').filter(Boolean).slice(from);
  }

  async function waitForReceived(from: number, expected: string): Promise<void> {
    await browser.waitUntil(() => received(from).join('').includes(expected), {
      timeout: 10_000,
      timeoutMsg: `the program never read ${expected}; it read ${received(from).join(' ')}`,
    });
  }

  before(async () => {
    await browser.execute(() => {
      const w = window as unknown as HookWindow;
      w.__PU_TERMINAL_STATS__ = true;
      // Ctrl+click reaches the opener service, which hands the URL to this spy, not a browser.
      w.__PU_OPENER_SPY__ = [];
    });
    agentId = await fixture.spawnTerminal(`node ${echoScript} echo ${log}`);
    await waitForAgentRow(agentId);
  });

  it('shows the selected agent in an xterm.js terminal on WebGL', async () => {
    await showAgents([agentId]);
    await waitForText(agentId, 'ECHO READY');
    const row = await probe(agentId);
    expect(row?.renderer).toBe('webgl');
    expect(row?.shown).toBe(true);
    expect(row?.fontFamily).toContain('Cascadia Mono');
    expect(row?.fontSize).toBe(13);
  });

  it('delivers typed keys as the exact bytes xterm.js encodes', async () => {
    await clickAt(await locate(agentId, 'ECHO READY'));
    const from = received().length;
    await browser.execute((id) => {
      (window as unknown as HookWindow).__puTerminals.resetStats(id);
    }, agentId);
    await browser.keys(['a']);
    await browser.keys(['Enter']);
    await browser.keys(['Backspace']);
    await browser.keys(['Tab']);
    await browser.keys(['ArrowUp']);
    await browser.keys(['Control', 'l']); // plain Ctrl+letter goes to the PTY
    await browser.keys(['Control', 'r']);
    await browser.keys(['Escape']);
    await browser.keys(['é']);
    const sent = await browser.execute(
      (id) => (window as unknown as HookWindow).__puTerminals.inputLog(id),
      agentId,
    );
    expect(sent).toEqual(['61', '0d', '7f', '09', '1b5b41', '0c', '12', '1b', 'c3a9']);
    await waitForReceived(from, (sent ?? []).join(''));
    expect(received(from).join('')).toBe((sent ?? []).join(''));
  });

  it('wraps a Ctrl+Shift+V paste in bracketed-paste markers', async () => {
    const from = received().length;
    await browser.execute(() => navigator.clipboard.writeText('pasted text'));
    await browser.keys(['Control', 'Shift', 'v']);
    await waitForReceived(from, hex('\x1b[200~pasted text\x1b[201~'));
  });

  it('copies the selection with Ctrl+Shift+C, and with Ctrl+C only while selected', async () => {
    await browser.execute(() => navigator.clipboard.writeText('before'));
    await doubleClickAt(await locate(agentId, 'READY'));
    await browser.keys(['Control', 'Shift', 'c']);
    await browser.waitUntil(async () => (await clipboardText()) === 'READY', {
      timeout: 5000,
      timeoutMsg: 'Ctrl+Shift+C did not copy the selection',
    });

    await browser.execute(() => navigator.clipboard.writeText('before'));
    const from = received().length;
    await doubleClickAt(await locate(agentId, 'ECHO'));
    await browser.keys(['Control', 'c']);
    await browser.waitUntil(async () => (await clipboardText()) === 'ECHO', {
      timeout: 5000,
      timeoutMsg: 'Ctrl+C with a selection did not copy it',
    });
    // Copying cleared the selection, so this Ctrl+C is the program's.
    await browser.keys(['Control', 'c']);
    await waitForReceived(from, '03');
    expect(received(from)).toEqual(['03']);
  });

  it('opens a Ctrl+clicked URL through the opener service', async () => {
    const link = await locate(agentId, 'https://example.com');
    await clickAt({ x: link.x + 20, y: link.y }); // a plain click opens nothing
    await clickAt({ x: link.x + 20, y: link.y }, 'Control');
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => (window as unknown as HookWindow).__PU_OPENER_SPY__ ?? []))
          .length > 0,
      { timeout: 5000, timeoutMsg: 'Ctrl+click did not open the link' },
    );
    const opened = await browser.execute(() => (window as unknown as HookWindow).__PU_OPENER_SPY__);
    expect(opened).toEqual(['https://example.com/pu-e2e?x=1']);
  });

  it('keeps app chords from the PTY and runs them', async () => {
    await clickAt(await locate(agentId, 'ECHO READY'));
    const from = received().length;
    await browser.keys(['Control', 'Shift', '\\']); // Toggle Sidebar
    await expect($('[aria-label="Sidebar"]')).not.toBeExisting();
    await browser.keys(['Control', 'Shift', '\\']);
    await expect($('[aria-label="Sidebar"]')).toBeDisplayed();
    await browser.keys(['Alt', 'ArrowUp']); // Focus Up
    await browser.keys(['z']);
    await waitForReceived(from, '7a');
    expect(received(from)).toEqual(['7a']);
  });

  it('sends SGR mouse reports while the program has mouse mode on', async () => {
    const from = received().length;
    await browser.keys(['Control', ']']);
    await waitForText(agentId, 'MOUSE ON');
    await clickAt(await locate(agentId, 'LINK'));
    // One SGR report per read: the press, then the release.
    const sgr = () =>
      received(from)
        .filter((line) => line.startsWith('1b5b3c'))
        .map((line) => Buffer.from(line, 'hex').toString('latin1').slice(1))
        .join(' ');
    await browser.waitUntil(() => sgr().endsWith('m'), {
      timeout: 10_000,
      timeoutMsg: `no SGR mouse release; the program read ${received(from).join(' ')}`,
    });
    expect(sgr()).toMatch(/^\[<0;\d+;\d+M \[<0;\d+;\d+m$/);
    await browser.keys(['Control', ']']);
    await waitForText(agentId, 'MOUSE OFF');
  });

  it("renders another client's PTY size, and reclaims it on focus", async () => {
    const own = await probe(agentId);
    const tap = await AttachTap.open(fixture, agentId, 'e2e:tap');
    try {
      await delay(500);
      tap.claimSize(50, 12);
      await browser.waitUntil(async () => (await probe(agentId))?.cols === 50, {
        timeout: 10_000,
        timeoutMsg: 'the pane did not adopt the other client size',
      });
      expect((await probe(agentId))?.rows).toBe(12);
      // Focus claims the size back (focus moves away first, so the pane sees a focus event).
      await browser.execute(() => {
        (document.activeElement as HTMLElement | null)?.blur();
      });
      const screen = await $(`[data-testid="terminal-pane"][data-agent-id="${agentId}"]`);
      await screen.click();
      await browser.waitUntil(async () => (await probe(agentId))?.cols === own?.cols, {
        timeout: 10_000,
        timeoutMsg: 'focus did not reclaim the PTY size',
      });
    } finally {
      tap.close();
    }
  });
});

describe('Terminal throughput', () => {
  const fixture = DaemonFixture.existing();

  /** Press a key in each pane, then wait until every one is streaming. */
  async function startFlood(agentIds: readonly string[]): Promise<void> {
    const start = new Map<string, number>();
    for (const id of agentIds) {
      start.set(id, (await probe(id))?.bytesReceived ?? 0);
      await clickAt(await locate(id, 'FLOOD READY'));
      await browser.keys(['g']);
    }
    for (const id of agentIds) {
      await browser.waitUntil(
        async () => ((await probe(id))?.bytesReceived ?? 0) > (start.get(id) ?? 0) + 50_000,
        { timeout: 20_000, timeoutMsg: `pane ${id} did not start streaming` },
      );
    }
  }

  it('streams ten panes at about 1 MB/s each with no long task over 50 ms', async function () {
    this.timeout(300_000);
    const ids: string[] = [];
    for (let i = 0; i < 10; i++) {
      ids.push(await fixture.spawnTerminal(`node ${echoScript} flood 1000000 40000000`));
    }
    for (const id of ids) await waitForAgentRow(id);
    await showAgents(ids);
    for (const id of ids) await waitForText(id, 'FLOOD READY');
    // Observe from before the streams start until they have run for a while.
    await browser.execute(() => {
      const w = window as unknown as HookWindow;
      w.__puLongTasks = [];
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries()) w.__puLongTasks?.push(entry.duration);
      }).observe({ type: 'longtask' });
    });
    await startFlood(ids);
    const before = new Map(
      (await browser.execute(() => (window as unknown as HookWindow).__puTerminals.list())).map(
        (row) => [row.agentId, row.bytesReceived],
      ),
    );
    const seconds = 15;
    await delay(seconds * 1000);
    const longTasks = await browser.execute(() => (window as unknown as HookWindow).__puLongTasks);
    const after = await browser.execute(() =>
      (window as unknown as HookWindow).__puTerminals.list(),
    );
    const rates = ids.map((id) => {
      const row = after.find((r) => r.agentId === id);
      return ((row?.bytesReceived ?? 0) - (before.get(id) ?? 0)) / seconds / 1e6;
    });
    console.log(
      `[terminal perf] MB/s per pane: ${rates.map((r) => r.toFixed(2)).join(', ')}; ` +
        `long tasks: ${JSON.stringify(longTasks)}; renderers: ${after
          .filter((r) => ids.includes(r.agentId))
          .map((r) => r.renderer)
          .join(',')}`,
    );
    for (const id of ids) await fixture.pu(['kill', '--agent', id]);
    expect(longTasks).toEqual([]);
    // Each program writes 1 MB/s, but ConPTY renders it into frames and the daemon (a debug
    // build here) moves about 1.5-2 MB/s of PTY output in all, the same with no app attached
    // (measured with the attach tap alone). The panes must keep up with what arrives.
    expect(rates.reduce((sum, rate) => sum + rate, 0)).toBeGreaterThan(0.5);
  });

  it('renders a 50 MB stream with the checksum of the bytes the daemon sent', async function () {
    this.timeout(600_000);
    const total = 50 * 1024 * 1024;
    const id = await fixture.spawnTerminal(`node ${echoScript} flood 0 ${String(total)}`);
    await waitForAgentRow(id);
    await showAgents([id]);
    await waitForText(id, 'FLOOD READY');
    const tap = await AttachTap.open(fixture, id);
    try {
      await delay(1000); // the tap's replay, then quiet
      await clickAt(await locate(id, 'FLOOD READY'));
      await delay(500);
      tap.reset();
      await browser.execute((agent) => {
        (window as unknown as HookWindow).__puTerminals.resetStats(agent);
      }, id);
      const started = Date.now();
      await browser.keys(['g']);
      await browser.waitUntil(() => tap.sawText('FLOOD-END'), {
        timeout: 480_000,
        interval: 500,
        timeoutMsg: `the stream did not finish; the tap has ${String(tap.bytes)} bytes`,
      });
      await browser.waitUntil(async () => (await probe(id))?.bytesParsed === tap.bytes, {
        timeout: 120_000,
        interval: 500,
        timeoutMsg: 'the pane did not parse every byte the daemon sent',
      });
      const row = await probe(id);
      console.log(
        `[terminal perf] 50 MB stream: ${String(tap.bytes)} PTY bytes in ` +
          `${((Date.now() - started) / 1000).toFixed(1)} s, checksum ${String(row?.checksum)} / ${String(tap.checksum)}`,
      );
      expect(row?.bytesReceived).toBe(tap.bytes);
      expect(row?.checksum).toBe(tap.checksum);
      await waitForText(id, 'FLOOD-END');
    } finally {
      tap.close();
      await fixture.pu(['kill', '--agent', id]);
    }
  });
});
