// Smoke test (WIN-5.7): the app starts against the fixture daemon, shows its window title and
// lists the fixture project, and its startup installer targets the fixture HOME.

import { existsSync } from 'node:fs';
import { basename, join } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { projectRow } from '../fixtures/sidebar.ts';

function fixtureEnv(name: 'PU_E2E_HOME' | 'PU_E2E_PROJECT_ROOT'): string {
  const value = process.env[name];
  if (value === undefined) throw new Error(`${name} is not set`);
  return value;
}

interface TauriWindow {
  __TAURI_INTERNALS__: { invoke(cmd: string, args?: unknown): Promise<unknown> };
}

describe('PurePoint smoke', () => {
  it('opens the PurePoint window', async () => {
    await expect(browser).toHaveTitle('PurePoint');
    const nativeTitle = await browser.execute(() =>
      (window as unknown as TauriWindow).__TAURI_INTERNALS__.invoke('plugin:window|title', {
        label: 'main',
      }),
    );
    expect(nativeTitle).toBe('PurePoint');
    await expect($('[aria-label="Sidebar"]')).toBeDisplayed();
  });

  it('lists the fixture project from the daemon', async () => {
    const projectRoot = fixtureEnv('PU_E2E_PROJECT_ROOT');
    await expect($('[role="tree"][aria-label="Projects"]')).toBeExisting();
    const item = projectRow(basename(projectRoot));
    await expect(item).toBeDisplayed();
    await expect(item).toHaveAttribute('data-kind', 'project');
    await expect(item).toHaveAttribute('aria-label', basename(projectRoot));
  });

  it('installs its CLI copy into the fixture HOME', async () => {
    const bin = join(fixtureEnv('PU_E2E_HOME'), '.pu', 'bin');
    await browser.waitUntil(
      () => existsSync(join(bin, 'pu.exe')) && existsSync(join(bin, 'pu-engine.exe')),
      { timeout: 30_000, timeoutMsg: `the installer did not install into ${bin}` },
    );
  });
});
