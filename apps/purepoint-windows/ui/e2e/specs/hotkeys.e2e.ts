// Hotkeys (WIN-7.6): every category of `HotkeyAction` (application, navigation, panes, tabs,
// chat) runs from its DEC-8 chord through the app's keydown handler (services/hotkeys.ts); a
// focused terminal keeps every chord that is not an app binding; and WebView2's browser keys
// never reload, print, find, zoom or open DevTools (REQ-WINAPP-065).
//
// The browser keys are off in every build (src-tauri/src/webview.rs), so this debug build checks
// the release behaviour. A reload is caught by a marker on `window` (a reload drops it), a zoom by
// `devicePixelRatio`, a print by `beforeprint`, find by the page losing focus, and a DevTools or
// new window by the window handles. Ctrl+Shift+I/J/C are the exception (see the last test).
// The terminal half runs `echo-bytes.mjs` in the agent's ConPTY, which logs every byte it reads.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { browser, $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { agentRow, agentRowSelector } from '../fixtures/sidebar.ts';
import { workspace } from '../fixtures/paths.ts';

const echoScript = join(workspace.appDir, 'ui', 'e2e', 'fixtures', 'echo-bytes.mjs');

interface HookWindow {
  __PU_TERMINAL_STATS__?: boolean;
  __PU_DIALOG_SPY__?: (string | null)[];
  __puHotkeyMarker?: string;
  __puPrints?: number;
  __puTerminals: { text(agentId: string): string | null };
}

const palette = () => $('[data-testid="command-palette"]');
const cells = () => $$('[data-testid="pane-cell"]');

async function press(...keys: string[]): Promise<void> {
  await browser.keys(keys);
  // Let React commit what the chord did before the next check.
  await delay(150);
}

async function focus(selector: string): Promise<void> {
  const found = await browser.execute((sel: string) => {
    const element = document.querySelector<HTMLElement>(sel);
    element?.focus();
    return element !== null && document.activeElement === element;
  }, selector);
  expect(found).toBe(true);
}

async function activeMatches(selector: string): Promise<boolean> {
  return browser.execute((sel: string) => document.activeElement?.matches(sel) === true, selector);
}

async function paneCount(): Promise<number> {
  return (await cells()).length;
}

async function waitForPaneCount(count: number): Promise<void> {
  await browser.waitUntil(async () => (await paneCount()) === count, {
    timeout: 15_000,
    timeoutMsg: `expected ${String(count)} panes, saw ${String(await paneCount())}`,
  });
}

async function focusedLeaf(): Promise<number | null> {
  return browser.execute(() => {
    const cell = document.querySelector<HTMLElement>(
      '[data-testid="pane-cell"][data-focused="true"]',
    );
    return cell === null ? null : Number(cell.dataset.leafId);
  });
}

async function waitForFocusedLeaf(leafId: number): Promise<void> {
  await browser.waitUntil(async () => (await focusedLeaf()) === leafId, {
    timeout: 10_000,
    timeoutMsg: `expected pane ${String(leafId)} focused, saw ${String(await focusedLeaf())}`,
  });
}

/** The focused pane's tabs and which of them is selected. */
async function focusedTabs(): Promise<{ ids: number[]; active: number | null }> {
  return browser.execute(() => {
    const cell = document.querySelector<HTMLElement>(
      '[data-testid="pane-cell"][data-focused="true"]',
    );
    const bar = document.querySelector(
      `[data-testid="pane-tab-bar"][data-leaf-id="${cell?.dataset.leafId ?? ''}"]`,
    );
    const tabs = Array.from(bar?.querySelectorAll<HTMLElement>('[data-testid="pane-tab"]') ?? []);
    const active = tabs.find((tab) => tab.dataset.active === 'true');
    return {
      ids: tabs.map((tab) => Number(tab.dataset.surfaceId)),
      active: active === undefined ? null : Number(active.dataset.surfaceId),
    };
  });
}

async function waitForActiveTabAt(index: number): Promise<void> {
  await browser.waitUntil(
    async () => {
      const tabs = await focusedTabs();
      return tabs.active !== null && tabs.ids.indexOf(tabs.active) === index;
    },
    {
      timeout: 10_000,
      timeoutMsg: `expected tab ${String(index)} selected: ${JSON.stringify(await focusedTabs())}`,
    },
  );
}

/** A new empty tab opens the palette for itself; dismiss it. */
async function dismissPalette(): Promise<void> {
  await expect(palette()).toBeDisplayed({ wait: 15_000 });
  await press('Escape');
  await expect(palette()).not.toBeExisting({ wait: 10_000 });
}

async function navCurrent(item: 'dashboard' | 'agents' | 'schedule'): Promise<boolean> {
  return (await $(`[data-testid="sidebar-nav-${item}"]`).getAttribute('aria-current')) === 'page';
}

describe('Hotkeys', () => {
  const fixture = DaemonFixture.existing();
  const log = join(fixture.root, 'hotkeys-echo.log');
  let agentId = '';

  function received(): string {
    return existsSync(log) ? readFileSync(log, 'utf8').split('\n').join('') : '';
  }

  async function waitForReceived(from: number, expected: string): Promise<void> {
    await browser.waitUntil(() => received().slice(from).includes(expected), {
      timeout: 10_000,
      timeoutMsg: `the PTY never read ${expected}; it read ${received().slice(from)}`,
    });
  }

  async function selectAgentWorkspace(): Promise<void> {
    await expect(agentRow(agentId)).toBeExisting({ wait: 30_000 });
    await browser.execute((selector: string) => {
      document
        .querySelector(selector)
        ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    }, agentRowSelector(agentId));
    await expect($('[data-testid="pane-grid"]')).toBeDisplayed();
    await expect(
      $(`[data-testid="terminal-pane"][data-agent-id="${agentId}"] .xterm`),
    ).toBeDisplayed({ wait: 30_000 });
  }

  async function focusTerminal(): Promise<void> {
    await focus(`[data-testid="terminal-pane"][data-agent-id="${agentId}"] .xterm textarea`);
  }

  before(async () => {
    await browser.execute(() => {
      const w = window as unknown as HookWindow;
      w.__PU_TERMINAL_STATS__ = true;
      w.__puHotkeyMarker = 'before-hotkeys';
      w.__puPrints = 0;
      window.addEventListener('beforeprint', () => {
        w.__puPrints = (w.__puPrints ?? 0) + 1;
      });
    });
    agentId = await fixture.spawnTerminal(`node ${echoScript} echo ${log}`);
    await selectAgentWorkspace();
    await browser.waitUntil(
      async () =>
        (
          (await browser.execute(
            (id) => (window as unknown as HookWindow).__puTerminals.text(id),
            agentId,
          )) ?? ''
        ).includes('ECHO READY'),
      { timeout: 30_000, timeoutMsg: 'the echo program never started' },
    );
  });

  it('application: New Agent, Settings and Open Project', async () => {
    await focus('main.detail-column');
    await press('Control', 'Shift', 'n');
    await expect($('[data-testid="command-palette"] input[aria-label="Filter"]')).toBeFocused({
      wait: 15_000,
    });
    await press('Escape');
    await expect(palette()).not.toBeExisting({ wait: 10_000 });

    await press('Control', ',');
    await expect($('[role="dialog"][aria-label="Settings"]')).toBeDisplayed({ wait: 10_000 });
    await press('Escape');
    await expect($('[role="dialog"][aria-label="Settings"]')).not.toBeExisting({ wait: 10_000 });

    // The folder picker takes a queued answer (services/dialog.ts); a cancel opens nothing.
    await browser.execute(() => {
      (window as unknown as HookWindow).__PU_DIALOG_SPY__ = [null];
    });
    await press('Control', 'Shift', 'o');
    await browser.waitUntil(
      async () =>
        (await browser.execute(
          () => (window as unknown as HookWindow).__PU_DIALOG_SPY__?.length ?? -1,
        )) === 0,
      { timeout: 10_000, timeoutMsg: 'Ctrl+Shift+O never ran the folder picker' },
    );
  });

  it('navigation: the nav items, the sidebars and focus', async () => {
    await focus('main.detail-column');
    await press('Alt', 'Shift', '2');
    await browser.waitUntil(() => navCurrent('agents'), { timeoutMsg: 'Alt+Shift+2' });
    await press('Alt', 'Shift', '3');
    await browser.waitUntil(() => navCurrent('schedule'), { timeoutMsg: 'Alt+Shift+3' });
    await press('Alt', 'Shift', '1');
    await browser.waitUntil(
      async () =>
        (await $('[data-testid="sidebar-nav-dashboard"]').getAttribute('aria-current')) === 'page',
      { timeoutMsg: 'Alt+Shift+1' },
    );

    await press('Control', 'Shift', '\\');
    await expect($('aside.sidebar-column')).not.toBeExisting({ wait: 5000 });
    // Focus Sidebar shows a hidden sidebar and focuses it.
    await press('Control', 'Shift', '[');
    await expect($('aside.sidebar-column')).toBeExisting({ wait: 5000 });
    await browser.waitUntil(() => activeMatches('aside.sidebar-column'), {
      timeoutMsg: 'Ctrl+Shift+[ did not focus the sidebar',
    });
    await press('Control', 'Shift', ']');
    await browser.waitUntil(() => activeMatches('main.detail-column'), {
      timeoutMsg: 'Ctrl+Shift+] did not focus the content',
    });
    await press('Control', 'Shift', '\\');
    await expect($('aside.sidebar-column')).not.toBeExisting({ wait: 5000 });
    await press('Control', 'Shift', '\\');
    await expect($('aside.sidebar-column')).toBeExisting({ wait: 5000 });

    await selectAgentWorkspace();
    await focus('main.detail-column');
    const shown = await $('button[aria-label="Hide Workspace Sidebar"]').isExisting();
    await press('Alt', 'Shift', '\\');
    await expect(
      $(`button[aria-label="${shown ? 'Show' : 'Hide'} Workspace Sidebar"]`),
    ).toBeExisting({ wait: 5000 });
    await press('Alt', 'Shift', '\\');
    await expect(
      $(`button[aria-label="${shown ? 'Hide' : 'Show'} Workspace Sidebar"]`),
    ).toBeExisting({ wait: 5000 });
  });

  it('panes: split, move focus and close', async () => {
    await selectAgentWorkspace();
    await waitForPaneCount(1);
    await focusTerminal();
    await press('Alt', 'Shift', '=');
    await waitForPaneCount(2);
    await dismissPalette();
    await waitForFocusedLeaf(1);
    await press('Alt', 'ArrowLeft');
    await waitForFocusedLeaf(0);
    await press('Alt', 'ArrowRight');
    await waitForFocusedLeaf(1);
    await press('Alt', 'Shift', '-');
    await waitForPaneCount(3);
    await dismissPalette();
    await waitForFocusedLeaf(2);
    await press('Alt', 'ArrowUp');
    await waitForFocusedLeaf(1);
    await press('Alt', 'ArrowDown');
    await waitForFocusedLeaf(2);
    // The new panes hold empty tabs, so they close without asking.
    await press('Alt', 'Shift', 'w');
    await waitForPaneCount(2);
    await press('Alt', 'ArrowRight');
    await waitForFocusedLeaf(1);
    await press('Alt', 'Shift', 'w');
    await waitForPaneCount(1);
  });

  it('tabs: new, cycle, go to, break out and close', async () => {
    await selectAgentWorkspace();
    await waitForPaneCount(1);
    await focus('main.detail-column');
    await press('Control', 'Shift', 't');
    await dismissPalette();
    await press('Control', 'Shift', 't');
    await dismissPalette();
    await browser.waitUntil(async () => (await focusedTabs()).ids.length === 3, {
      timeoutMsg: 'Ctrl+Shift+T did not add two tabs',
    });
    await waitForActiveTabAt(2);
    await press('Control', 'Shift', '1');
    await waitForActiveTabAt(0);
    await press('Control', 'Shift', '9');
    await waitForActiveTabAt(2);
    await press('Control', 'Tab');
    await waitForActiveTabAt(0);
    await press('Control', 'Shift', 'Tab');
    await waitForActiveTabAt(2);
    await press('Control', 'Shift', '2');
    await waitForActiveTabAt(1);

    // Move Tab to New Pane, then Close Tab closes it and its pane with it.
    await press('Alt', 'Shift', 'd');
    await waitForPaneCount(2);
    await browser.waitUntil(async () => (await focusedTabs()).ids.length === 1, {
      timeoutMsg: 'the broken-out pane does not hold one tab',
    });
    await press('Control', 'Shift', 'w');
    await waitForPaneCount(1);
    await browser.waitUntil(async () => (await focusedTabs()).ids.length === 2, {
      timeoutMsg: 'Close Tab did not leave two tabs',
    });
    await press('Control', 'Shift', '9');
    await waitForActiveTabAt(1);
    await press('Control', 'Shift', 'w');
    await browser.waitUntil(async () => (await focusedTabs()).ids.length === 1, {
      timeoutMsg: 'Close Tab did not close the empty tab',
    });
  });

  it('chat: Toggle Chat Sidebar is the app’s even in a terminal', async () => {
    await selectAgentWorkspace();
    await focusTerminal();
    const from = received().length;
    await press('Control', 'Shift', 's');
    // A key the terminal took would reach the PTY; a plain key after it proves the PTY reads.
    await press('a');
    await waitForReceived(from, '61');
    expect(received().slice(from)).toBe('61');
  });

  it('given a focused terminal, should pass Ctrl+R and the other non-app chords to the PTY', async () => {
    await selectAgentWorkspace();
    await focusTerminal();
    const from = received().length;
    await press('Control', 'r');
    await press('Control', 'w');
    await press('Control', 'n');
    await press('Control', 'p');
    await press('Control', 'f');
    await press('F5');
    await waitForReceived(from, '12170e1006' + Buffer.from('\x1b[15~').toString('hex'));
    expect(await browser.execute(() => (window as unknown as HookWindow).__puHotkeyMarker)).toBe(
      'before-hotkeys',
    );
  });

  it('given browser keys, should never reload, print, find, zoom or open DevTools', async () => {
    await focus('main.detail-column');
    const before = await browser.execute(() => ({
      ratio: window.devicePixelRatio,
      width: window.innerWidth,
    }));
    const handles = (await browser.getWindowHandles()).length;
    const chords: string[][] = [
      ['F5'],
      ['Control', 'r'],
      ['Control', 'Shift', 'r'],
      ['Control', 'p'],
      ['Control', 'f'],
      ['Control', 'w'],
      ['Control', 'n'],
      ['Control', '='],
      ['Control', 'Shift', '='],
      ['Control', '-'],
      ['F12'],
    ];
    const opened: string[] = [];
    for (const chord of chords) {
      await press(...chord);
      await focus('main.detail-column');
      const now = await browser.getWindowHandles();
      if (now.length !== handles) {
        const main = await browser.getWindowHandle();
        const urls: string[] = [];
        for (const handle of now) {
          await browser.switchToWindow(handle);
          urls.push(`${await browser.getUrl()} ${await browser.getTitle()}`);
        }
        await browser.switchToWindow(main);
        opened.push(`${chord.join('+')}: ${urls.join(' | ')}`);
      }
    }
    expect(opened).toEqual([]);
    // Ctrl+wheel over the sidebar, both ways.
    const outline = await $('[role="tree"][aria-label="Projects"]').getLocation();
    for (const deltaY of [-300, 300]) {
      await browser.performActions([
        {
          type: 'key',
          id: 'keyboard',
          actions: [
            { type: 'keyDown', value: String.fromCharCode(0xe009) },
            { type: 'pause', duration: 0 },
            { type: 'pause', duration: 0 },
            { type: 'keyUp', value: String.fromCharCode(0xe009) },
          ],
        },
        {
          type: 'wheel',
          id: 'wheel',
          actions: [
            { type: 'pause', duration: 0 },
            {
              type: 'scroll',
              x: Math.round(outline.x + 40),
              y: Math.round(outline.y + 40),
              deltaX: 0,
              deltaY,
              duration: 100,
            },
          ],
        },
      ] as never);
      await browser.releaseActions();
    }
    await delay(1500);
    const after = await browser.execute(() => {
      const w = window as unknown as HookWindow;
      return {
        marker: w.__puHotkeyMarker,
        prints: w.__puPrints,
        ratio: window.devicePixelRatio,
        width: window.innerWidth,
        focused: document.hasFocus(),
      };
    });
    expect(after.marker).toBe('before-hotkeys');
    expect(after.prints).toBe(0);
    expect(after.ratio).toBe(before.ratio);
    expect(after.width).toBe(before.width);
    expect(after.focused).toBe(true);
    expect((await browser.getWindowHandles()).length).toBe(handles);

    // Ctrl+Shift+I/J/C: WebView2 turns them off with its accelerator keys, but msedgedriver's keys
    // arrive through its debugger connection, which still opens DevTools for them (even with
    // DevTools disabled). So here they are only checked not to reload, print or zoom, and any
    // DevTools window they open is closed again.
    const main = await browser.getWindowHandle();
    for (const chord of [
      ['Control', 'Shift', 'i'],
      ['Control', 'Shift', 'j'],
      ['Control', 'Shift', 'c'],
    ]) {
      await browser.switchToWindow(main);
      await focus('main.detail-column');
      await press(...chord);
      for (const handle of await browser.getWindowHandles()) {
        if (handle === main) continue;
        await browser.switchToWindow(handle);
        await browser.closeWindow();
      }
    }
    await browser.switchToWindow(main);
    await delay(1000);
    const last = await browser.execute(() => {
      const w = window as unknown as HookWindow;
      return { marker: w.__puHotkeyMarker, prints: w.__puPrints, ratio: window.devicePixelRatio };
    });
    expect(last).toEqual({ marker: 'before-hotkeys', prints: 0, ratio: before.ratio });
  });
});
