// WebdriverIO against the real app through tauri-driver + msedgedriver (WIN-5.7).
// Run it with `npm run e2e` (ui/e2e/run.mjs), which fetches the drivers, builds what is missing
// and stages the app first.
//
// Every spec file gets its own worker, daemon fixture (temp HOME, git repo, `pu init`, a
// `terminal` agent) and tauri-driver, all torn down in `afterSession`.

import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { basename, join } from 'node:path';
import { DaemonFixture } from './fixtures/daemon.ts';
import { workspace } from './fixtures/paths.ts';
import { TauriDriver } from './fixtures/tauriDriver.ts';

const runRoot = process.env.PU_E2E_RUN_ROOT;

let fixture: DaemonFixture | null = null;
let driver: TauriDriver | null = null;
let specName = 'spec';

function specArtifacts(): string {
  const dir = join(workspace.artifactsDir, specName);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function safeName(text: string): string {
  return (
    text
      .replace(/[^\w.-]+/g, '-')
      .replace(/^-+|-+$/g, '')
      .slice(0, 80) || 'test'
  );
}

async function teardown(): Promise<void> {
  try {
    await driver?.stop();
  } finally {
    driver = null;
    const current = fixture;
    fixture = null;
    if (current !== null) {
      if (existsSync(current.daemonLog)) {
        copyFileSync(current.daemonLog, join(specArtifacts(), 'pu-engine.log'));
      }
      await current.dispose();
    }
  }
}

export const config: WebdriverIO.Config = {
  runner: 'local',
  specs: ['./specs/**/*.e2e.ts'],
  maxInstances: 1,
  capabilities: [
    {
      'tauri:options': { application: workspace.appExe },
      // msedgedriver's WebView2 mode speaks classic WebDriver only.
      'wdio:enforceWebDriverClassic': true,
    } as WebdriverIO.Capabilities,
  ],
  hostname: '127.0.0.1',
  port: 4444,
  logLevel: 'warn',
  outputDir: join(workspace.artifactsDir, 'logs'),
  waitforTimeout: 30_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,
  framework: 'mocha',
  reporters: ['spec'],
  mochaOpts: { ui: 'bdd', timeout: 120_000 },

  beforeSession: async (sessionConfig, _caps, specs) => {
    if (runRoot === undefined) {
      throw new Error('PU_E2E_RUN_ROOT is not set: run the end-to-end tests with `npm run e2e`.');
    }
    specName = basename(specs[0] ?? 'spec').replace(/\.e2e\.ts$/, '');
    try {
      fixture = await DaemonFixture.create({ root: join(runRoot, specName) });
      await fixture.startDaemon();
      await fixture.init();
      await fixture.spawnTerminal();
      process.env.PU_E2E_HOME = fixture.home;
      process.env.PU_E2E_PROJECT_ROOT = fixture.projectRoot;
      driver = await TauriDriver.start(fixture.env, join(specArtifacts(), 'tauri-driver.log'));
      sessionConfig.port = driver.port;
    } catch (error) {
      await teardown();
      throw error;
    }
  },

  afterTest: async (test, _context, result) => {
    const status = result.passed ? 'passed' : 'failed';
    const file = `${safeName(test.title)}-${status}.png`;
    try {
      await browser.saveScreenshot(join(specArtifacts(), file));
    } catch (error) {
      console.warn(`Could not save screenshot ${file}:`, error);
    }
  },

  afterSession: async () => {
    await teardown();
  },
};
