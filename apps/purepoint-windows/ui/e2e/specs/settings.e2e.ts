// Settings (WIN-8.11) against the fixture daemon: appearance and the terminal font size apply
// live, a recorded hotkey works once Settings closes, and an All Projects launch arg typed in
// Settings › Agents reaches the daemon's ~/.pu/agent-settings.yaml in the temp HOME. Nothing here
// launches a real AI tool or opens a URL.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

interface HookWindow {
  __puTerminals: { list(): { agentId: string; fontSize: number | null }[] };
  __PU_OPENER_SPY__?: string[];
}

const fixture = DaemonFixture.existing();
const settings = () => $('[data-testid="settings-view"]');

async function openSettings(): Promise<void> {
  if (await settings().isExisting()) return;
  await browser.keys(['Control', ',']);
  await expect(settings()).toBeDisplayed();
}

async function closeSettings(): Promise<void> {
  await browser.keys('Escape');
  await settings().waitForExist({ reverse: true });
}

async function section(name: string): Promise<void> {
  await $(`[data-testid="settings-section-${name}"]`).click();
}

async function fontSizes(): Promise<(number | null)[]> {
  const rows = await browser.execute(() => (window as unknown as HookWindow).__puTerminals.list());
  return rows.map((row) => row.fontSize);
}

describe('Settings', () => {
  before(async () => {
    await browser.execute(() => {
      (window as unknown as HookWindow).__PU_OPENER_SPY__ = [];
    });
    // Show the fixture's terminal agent, so the font size has a terminal to apply to.
    const row = $('[data-testid^="sidebar-row-agent-"]');
    await row.waitForExist({ timeout: 30_000 });
    await browser.execute(() => {
      document
        .querySelector('[data-testid^="sidebar-row-agent-"]')
        ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    });
    await expect($('[data-testid="terminal-pane"] .xterm')).toBeDisplayed({ wait: 30_000 });
  });

  it('changes the appearance and the terminal font size live', async () => {
    await openSettings();
    await section('display');
    await $('[role="radio"][data-value="dark"]').click();
    await browser.waitUntil(
      async () => (await browser.execute(() => document.documentElement.dataset.theme)) === 'dark',
      { timeoutMsg: 'the app never switched to dark' },
    );
    await $('[role="radio"][data-value="light"]').click();
    await browser.waitUntil(
      async () => (await browser.execute(() => document.documentElement.dataset.theme)) === 'light',
      { timeoutMsg: 'the app never switched to light' },
    );

    const before = await fontSizes();
    expect(before.length).toBeGreaterThan(0);
    expect(before.every((size) => size === 13)).toBe(true);
    await browser.execute(() => {
      document.querySelector<HTMLInputElement>('#settings-terminal-font-size')?.focus();
    });
    await browser.keys(['ArrowRight', 'ArrowRight', 'ArrowRight']);
    await expect($('[data-testid="terminal-font-size-value"]')).toHaveText('16pt');
    await browser.waitUntil(async () => (await fontSizes()).every((size) => size === 16), {
      timeoutMsg: 'the open terminal never took the new font size',
    });
  });

  it('records a new hotkey and runs it', async () => {
    await openSettings();
    await section('hotkeys');
    await $('[data-testid="hotkey-row-navAgents"]').click();
    await expect($('[data-testid="hotkey-row-navAgents"]')).toHaveAttribute(
      'data-recording',
      'true',
    );
    // A chord the terminal keeps is refused.
    await browser.keys(['Control', 'c']);
    await expect($('[data-testid="hotkey-row-navAgents"] [role="status"]')).toHaveText(
      'Ctrl+C is kept for the terminal',
    );
    await browser.keys(['Control', 'Shift', 'y']);
    await expect($('[data-testid="hotkey-keys-navAgents"]')).toHaveText(
      expect.stringMatching(/Ctrl\s*Shift\s*Y/),
    );
    await closeSettings();

    await expect($('[data-testid="agents-hub"]')).not.toBeExisting();
    await browser.keys(['Control', 'Shift', 'y']);
    await expect($('[data-testid="agents-hub"]')).toBeDisplayed();
  });

  it('saves an All Projects launch arg through the daemon', async () => {
    await openSettings();
    await section('agents');
    const table = $('[data-testid="agents-machine"]');
    await browser.waitUntil(async () => (await table.getAttribute('disabled')) === null, {
      timeoutMsg: 'the machine settings never loaded',
    });
    const field = table.$('input[aria-label="Arguments claude"]');
    await field.click();
    await field.setValue('--e2e-flag');
    await browser.keys('Enter');

    const file = join(fixture.home, '.pu', 'agent-settings.yaml');
    await browser.waitUntil(
      () => existsSync(file) && readFileSync(file, 'utf8').includes('--e2e-flag'),
      { timeout: 30_000, timeoutMsg: `${file} never got the launch arg` },
    );
    expect(readFileSync(file, 'utf8')).toMatch(/claude:\s*['"]?--e2e-flag/);
    await closeSettings();

    const opened = await browser.execute(() => (window as unknown as HookWindow).__PU_OPENER_SPY__);
    expect(opened).toEqual([]);
  });
});
