// Open Project (WIN-8.15): Ctrl+Shift+O and the sidebar footer's Open run the native folder
// picker, and the chosen folder opens as a project registered with the daemon. WebDriver cannot
// drive the native dialog, so each pick is queued in `__PU_DIALOG_SPY__` (services/dialog.ts).

import { existsSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { projectRow } from '../fixtures/sidebar.ts';

interface SpyWindow {
  __PU_DIALOG_SPY__?: (string | null)[];
}

async function queuePick(path: string | null): Promise<void> {
  await browser.execute((next) => {
    const spy = window as unknown as SpyWindow;
    spy.__PU_DIALOG_SPY__ = [...(spy.__PU_DIALOG_SPY__ ?? []), next];
  }, path);
}

async function remainingPicks(): Promise<number> {
  return browser.execute(() => (window as unknown as SpyWindow).__PU_DIALOG_SPY__?.length ?? 0);
}

describe('Open Project', () => {
  const fixture = DaemonFixture.existing();

  before(async () => {
    await expect(projectRow('e2e-project')).toBeDisplayed({ wait: 30_000 });
  });

  it('given Ctrl+Shift+O, should open the picked git folder as a project', async () => {
    const root = join(fixture.home, 'picked-repo');
    mkdirSync(root, { recursive: true });
    await fixture.gitIn(root, 'init', '--quiet', '--initial-branch=main');
    await queuePick(root);
    await $('[aria-label="Sidebar"]').click();
    await browser.keys(['Control', 'Shift', 'o']);
    await expect(projectRow('picked-repo')).toBeDisplayed({ wait: 30_000 });
    // The daemon registered it (`init` wrote the manifest).
    await browser.waitUntil(() => existsSync(join(root, '.pu', 'manifest.json')), {
      timeout: 30_000,
      timeoutMsg: 'the daemon never initialized the picked folder',
    });
    expect(await remainingPicks()).toBe(0);
  });

  it('given the footer Open on a folder that is not a git repository, should open it as the Mac does', async () => {
    const root = join(fixture.home, 'plain-folder');
    mkdirSync(root, { recursive: true });
    await queuePick(root);
    await $('button[aria-label="Open"]').click();
    await expect(projectRow('plain-folder')).toBeDisplayed({ wait: 30_000 });
    await browser.waitUntil(() => existsSync(join(root, '.pu', 'manifest.json')), {
      timeout: 30_000,
      timeoutMsg: 'the daemon never initialized the plain folder',
    });
  });

  it('given Cancel, should open nothing', async () => {
    const before = await $$('[data-kind="project"]').length;
    await queuePick(null);
    await $('button[aria-label="Open"]').click();
    await browser.waitUntil(async () => (await remainingPicks()) === 0, { timeout: 10_000 });
    expect(await $$('[data-kind="project"]').length).toBe(before);
  });

  it("given the home folder, should show the daemon's refusal in the banner", async () => {
    await queuePick(fixture.home);
    await $('button[aria-label="Open"]').click();
    const banner = $('.daemon-error-banner');
    await expect(banner).toBeDisplayed({ wait: 30_000 });
    await expect(banner).toHaveText(
      expect.stringContaining('register a project directory instead'),
    );
  });
});
