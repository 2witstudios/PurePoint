// The command palette (WIN-7.5) against real `terminal` agents: Ctrl+Shift+N opens it over the
// window with the Mac's rows and closes on Escape; a New Tab opens it for the empty tab, and
// choosing Terminal spawns into that tab of the current workspace (a `spawn` with `placement`),
// which shows a live terminal; Ctrl+Shift+N → Terminal spawns an agent whose workspace the app
// then selects, with its terminal live.

import { browser, $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { agentRow, agentRowSelector } from '../fixtures/sidebar.ts';

interface HookWindow {
  __puTerminals: { text(agentId: string): string | null };
}

const palette = () => $('[data-testid="command-palette"]');
const paletteRows = () => $$('[data-testid="palette-row"]');
const search = () => $('[data-testid="command-palette"] input[aria-label="Filter"]');

async function rowNames(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll('[data-testid="palette-row"] .palette-row-name'),
      (node) => node.textContent,
    ),
  );
}

/** Agent IDs of the terminals the on-screen grid shows. */
async function shownTerminals(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>(
        '[data-testid="pane-grid"] [data-testid="terminal-pane"]',
      ),
      (node) => node.dataset.agentId ?? '',
    ),
  );
}

async function tabCount(): Promise<number> {
  return (await $$('[data-testid="pane-grid"] [data-testid="pane-tab"]')).length;
}

async function screenText(agentId: string): Promise<string> {
  return (
    (await browser.execute(
      (id) => (window as unknown as HookWindow).__puTerminals.text(id),
      agentId,
    )) ?? ''
  );
}

/** The terminal echoes a typed command and its output: the PTY is live in both directions. */
async function expectLive(agentId: string, marker: string): Promise<void> {
  await expect($(`[data-testid="terminal-pane"][data-agent-id="${agentId}"] .xterm`)).toBeDisplayed(
    { wait: 30_000 },
  );
  // The shell prints its prompt once it is up.
  await browser.waitUntil(async () => (await screenText(agentId)).trim() !== '', {
    timeout: 30_000,
    timeoutMsg: `terminal ${agentId} never printed anything`,
  });
  await browser.keys(`echo ${marker}`.split(''));
  await browser.keys(['Enter']);
  await browser.waitUntil(async () => (await screenText(agentId)).split(marker).length - 1 >= 2, {
    timeout: 30_000,
    timeoutMsg: `terminal ${agentId} never echoed ${marker}`,
  });
}

/** Opens the palette with Ctrl+Shift+N, on whatever has focus. */
async function openWithShortcut(): Promise<void> {
  await browser.keys(['Control', 'Shift', 'n']);
  await expect(palette()).toBeDisplayed({ wait: 15_000 });
  await expect(search()).toBeFocused();
}

/** Filters to Terminal, takes it, and submits its (empty) initial command. */
async function chooseTerminal(): Promise<void> {
  await browser.keys('term'.split(''));
  await browser.waitUntil(async () => (await paletteRows()).length === 1, {
    timeout: 10_000,
    timeoutMsg: `the filter left ${JSON.stringify(await rowNames())}`,
  });
  expect(await rowNames()).toEqual(['Terminal']);
  await browser.keys(['Enter']);
  await expect(palette()).toHaveAttribute('data-phase', 'prompt');
  await expect($('[data-testid="command-palette"] textarea[aria-label="Prompt"]')).toBeFocused();
  await browser.keys(['Enter']);
  await expect(palette()).not.toBeExisting({ wait: 10_000 });
}

describe('Command palette', () => {
  const fixture = DaemonFixture.existing();
  let first = '';
  let workspaceId = '';

  before(async () => {
    first = await fixture.spawnTerminal();
    await expect(agentRow(first)).toBeExisting({ wait: 30_000 });
    await browser.execute((selector: string) => {
      document
        .querySelector(selector)
        ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    }, agentRowSelector(first));
    await expect($('[data-testid="pane-grid"]')).toBeDisplayed();
    workspaceId = (await $('[data-testid="pane-grid"]').getAttribute('data-workspace-id')) ?? '';
    await expect($(`[data-testid="terminal-pane"][data-agent-id="${first}"] .xterm`)).toBeDisplayed(
      { wait: 30_000 },
    );
  });

  it('opens on Ctrl+Shift+N with the built-in rows, and closes on Escape', async () => {
    await openWithShortcut();
    const names = await rowNames();
    expect(names.slice(0, 5)).toEqual(['Claude', 'Codex', 'OpenCode', 'Terminal', 'Worktree']);
    await browser.keys(['ArrowDown']);
    await expect($('[data-testid="palette-row"][data-highlighted="true"]')).toHaveText(
      expect.stringContaining('Codex'),
    );
    await browser.keys(['Escape']);
    await expect(palette()).not.toBeExisting({ wait: 10_000 });
    // The shortcut toggles it too.
    await openWithShortcut();
    await browser.keys(['Control', 'Shift', 'n']);
    await expect(palette()).not.toBeExisting({ wait: 10_000 });
  });

  it('spawns a terminal into a new tab of the current workspace', async () => {
    const tabsBefore = await tabCount();
    await $('[data-testid="pane-tab-bar"] button[aria-label="New Tab"]').click();
    // The new, empty tab opens the palette for itself.
    await expect(palette()).toBeDisplayed({ wait: 15_000 });
    await expect(search()).toBeFocused();
    await chooseTerminal();

    await browser.waitUntil(async () => (await tabCount()) === tabsBefore + 1, {
      timeout: 15_000,
      timeoutMsg: 'the new tab never appeared',
    });
    let spawned = '';
    await browser.waitUntil(
      async () => {
        spawned = (await shownTerminals()).find((id) => id !== first && id !== '') ?? '';
        return spawned !== '';
      },
      { timeout: 30_000, timeoutMsg: 'the new tab never showed the spawned terminal' },
    );
    // Still the same workspace, its new tab the active one.
    expect(await $('[data-testid="pane-grid"]').getAttribute('data-workspace-id')).toBe(
      workspaceId,
    );
    await expect($('[data-testid="pane-empty-tab"]')).not.toBeExisting();
    await expectLive(spawned, 'palette-tab-ok');
  });

  it('spawns from Ctrl+Shift+N and selects the new agent with a live terminal', async () => {
    const shown = await shownTerminals();
    await openWithShortcut();
    await chooseTerminal();
    let spawned = '';
    await browser.waitUntil(
      async () => {
        spawned = (await shownTerminals()).find((id) => !shown.includes(id) && id !== '') ?? '';
        return spawned !== '';
      },
      { timeout: 30_000, timeoutMsg: 'the spawned agent was never shown' },
    );
    await expect(agentRow(spawned)).toBeExisting({ wait: 30_000 });
    await expectLive(spawned, 'palette-menu-ok');
  });
});
