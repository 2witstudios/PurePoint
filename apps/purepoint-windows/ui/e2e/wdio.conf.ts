// WebdriverIO against the real app through tauri-driver + msedgedriver (WIN-5.7).
// Run it with `npm run e2e` (ui/e2e/run.mjs), which fetches the drivers, builds what is missing
// and stages the app first.
//
// Every spec file gets its own worker, daemon fixture (temp HOME, git repo, `pu init`, a
// `terminal` agent) and tauri-driver, all torn down in `afterSession`. The machines spec also
// gets a second daemon in its own temp HOME standing in for a remote machine, reached through a
// fake `ssh.cmd` (`PU_SSH`; fixtures/fakeSsh.ts). The Point Guard spec runs the app-managed runtime
// in development mode with its LOCALAPPDATA and APPDATA in the temp profile and Pi answering from
// a loopback fixture provider (fixtures/fakeProvider.ts); the Credential Manager entries its chat
// saves are removed afterwards (fixtures/credentials.ts). The parity walkthrough (WIN-11.1) ends in
// a Point Guard chat, so it gets the same runtime setup.

import { copyFileSync, existsSync, mkdirSync } from 'node:fs';
import { basename, join } from 'node:path';
import { DaemonFixture } from './fixtures/daemon.ts';
import { writeFakeSsh } from './fixtures/fakeSsh.ts';
import { FakeProvider } from './fixtures/fakeProvider.ts';
import { pairingCredentials, removePairingCredentials } from './fixtures/credentials.ts';
import { workspace } from './fixtures/paths.ts';
import { TauriDriver } from './fixtures/tauriDriver.ts';

const runRoot = process.env.PU_E2E_RUN_ROOT;

let fixture: DaemonFixture | null = null;
/** The machines spec's "remote" daemon. */
let remote: DaemonFixture | null = null;
let driver: TauriDriver | null = null;
/** The Point Guard spec's model provider, and the pairing credentials that existed before it. */
let provider: FakeProvider | null = null;
let credentialsBefore: Set<string> | null = null;
let specName = 'spec';
/** The specs that chat with Point Guard through the fixture provider. */
const POINT_GUARD_SPECS: ReadonlySet<string> = new Set(['pointGuard', 'walkthrough']);

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
    await provider?.stop();
    provider = null;
    if (credentialsBefore !== null) removePairingCredentials(credentialsBefore);
    credentialsBefore = null;
    const current = fixture;
    fixture = null;
    if (current !== null) {
      if (existsSync(current.daemonLog)) {
        copyFileSync(current.daemonLog, join(specArtifacts(), 'pu-engine.log'));
      }
      await current.dispose();
    }
    const machine = remote;
    remote = null;
    if (machine !== null) {
      if (existsSync(machine.daemonLog)) {
        copyFileSync(machine.daemonLog, join(specArtifacts(), 'remote-pu-engine.log'));
      }
      await machine.dispose();
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
      process.env.PU_E2E_ROOT = fixture.root;
      process.env.PU_E2E_HOME = fixture.home;
      process.env.PU_E2E_PROJECT_ROOT = fixture.projectRoot;
      let appEnv = fixture.env;
      if (specName === 'machines') {
        remote = await DaemonFixture.create({
          root: join(runRoot, 'machines-remote'),
          projectName: 'remote-project',
        });
        await remote.startDaemon();
        process.env.PU_E2E_REMOTE_ROOT = remote.root;
        appEnv = { ...appEnv, ...writeFakeSsh(join(runRoot, 'fake-ssh'), remote.socket) };
      }
      if (POINT_GUARD_SPECS.has(specName)) {
        provider = await FakeProvider.start();
        provider.writePiAgentConfig(fixture.home);
        credentialsBefore = pairingCredentials();
        const localAppData = join(fixture.root, 'localappdata');
        const appData = join(fixture.root, 'appdata');
        mkdirSync(localAppData, { recursive: true });
        mkdirSync(appData, { recursive: true });
        appEnv = {
          ...appEnv,
          PUREPOINT_POINTGUARD_DEV: '1',
          LOCALAPPDATA: localAppData,
          APPDATA: appData,
        };
      }
      driver = await TauriDriver.start(appEnv, join(specArtifacts(), 'tauri-driver.log'));
      sessionConfig.port = driver.port;
    } catch (error) {
      await teardown();
      throw error;
    }
  },

  afterTest: async (test, _context, result) => {
    // The walkthrough saves its own screenshot per step (NN-step.png); keep only its failures.
    if (specName === 'walkthrough' && result.passed) return;
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
