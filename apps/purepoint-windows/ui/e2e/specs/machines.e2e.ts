// Machines (WIN-8.14): add a machine, see it healthy, open a project on it through the remote
// folder picker, and stream an agent's terminal from it. The "machine" is a second daemon in its
// own temp HOME (wdio.conf.ts), reached through the app's ssh relay with a fake `ssh.cmd` as
// `PU_SSH` (fixtures/fakeSsh.ts) that runs `pu daemon proxy` against that daemon.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { workspace } from '../fixtures/paths.ts';
import { agentRow, agentRowSelector, projectRow } from '../fixtures/sidebar.ts';

const echoScript = join(workspace.appDir, 'ui', 'e2e', 'fixtures', 'echo-bytes.mjs');

interface HookWindow {
  __PU_TERMINAL_STATS__?: boolean;
  __puTerminals: { text(agentId: string): string | null };
}

function env(name: string): string {
  const value = process.env[name];
  if (value === undefined) throw new Error(`${name} is not set`);
  return value;
}

function machineRow(name: string) {
  return $(`[data-testid="sidebar-row-machine-${name}"]`);
}

async function screenText(agentId: string): Promise<string> {
  return (
    (await browser.execute(
      (id) => (window as unknown as HookWindow).__puTerminals.text(id),
      agentId,
    )) ?? ''
  );
}

describe('Machines', () => {
  const local = DaemonFixture.current();
  const remoteRoot = env('PU_E2E_REMOTE_ROOT');
  const remote = DaemonFixture.existing(remoteRoot);
  const remoteHome = join(remoteRoot, 'home');
  const remoteProject = join(remoteHome, 'remote-project');
  const sshLog = join(env('PU_E2E_RUN_ROOT'), 'fake-ssh', 'ssh.log');
  let agentId = '';

  before(async () => {
    await expect(projectRow('e2e-project')).toBeDisplayed({ wait: 30_000 });
    await browser.execute(() => {
      (window as unknown as HookWindow).__PU_TERMINAL_STATS__ = true;
    });
  });

  it('given Add Machine, should save it in the shared registry and show it healthy', async () => {
    await $('button[aria-label="Add Machine"]').click();
    const sheet = $('[data-testid="add-machine-sheet"]');
    await expect(sheet).toBeDisplayed();
    await sheet.$('input[placeholder="studio"]').setValue('local');
    await sheet.$('input[placeholder="alias or user@host[:port]"]').setValue('fakehost');
    await sheet.$('button=Add').click();
    await expect(sheet).not.toBeExisting({ wait: 10_000 });

    // The shared registry `pu machine` reads (pu-core's schema).
    const registry = JSON.parse(readFileSync(join(local.home, '.pu', 'machines.json'), 'utf8')) as {
      version: number;
      machines: { id: string; name: string; ssh_host: string }[];
    };
    expect(registry.version).toBe(1);
    expect(registry.machines.map((m) => [m.name, m.ssh_host])).toEqual([['local', 'fakehost']]);

    const row = machineRow('local');
    await expect(row).toBeDisplayed({ wait: 30_000 });
    // Healthy: the relayed `health` answered with this app's protocol.
    await expect(row).toHaveAttribute('aria-label', 'local (fakehost): Connected', {
      wait: 60_000,
    });
    // Through the relay: the uname probe, then `pu daemon proxy` with connection sharing off.
    const log = readFileSync(sshLog, 'utf8').replace(/"/g, '');
    expect(log).toContain('uname -s');
    expect(log).toMatch(/-o ControlMaster=no -o ControlPath=none fakehost pu daemon proxy/);
  });

  it('given the folder picker, should open a project on the machine', async () => {
    await machineRow('local').$('button[aria-label="Open Remote Project"]').click();
    const picker = $('[data-testid="remote-folder-picker"]');
    await expect(picker).toBeDisplayed();
    // It starts at the machine user's home, listing folders through the machine's daemon.
    await expect($('[data-testid="remote-folder-path"]')).toHaveText(remoteHome, {
      wait: 30_000,
    });
    const folder = $('[data-testid="remote-folder-remote-project"]');
    await expect(folder).toBeDisplayed();
    await folder.click();
    await picker.$('button=Open').click();
    await expect(picker).not.toBeExisting({ wait: 30_000 });

    // The machine's daemon registered it, and it is listed under the machine.
    expect(existsSync(join(remoteProject, '.pu', 'manifest.json'))).toBe(true);
    const row = projectRow('remote-project');
    await expect(row).toBeDisplayed({ wait: 30_000 });
    await expect(machineRow('local')).toHaveAttribute('aria-label', 'local (fakehost): 1 project', {
      wait: 30_000,
    });
  });

  it('given an agent on the machine, should stream its terminal over the relay', async () => {
    const log = join(remoteRoot, 'echo.log');
    const { stdout } = await remote.pu(
      [
        'spawn',
        '--root',
        '--agent',
        'terminal',
        '--command',
        `node ${echoScript} echo ${log}`,
        '--json',
      ],
      remoteProject,
    );
    const match = /"(?:agent_id|id)"\s*:\s*"([^"]+)"/.exec(stdout);
    if (!match?.[1]) throw new Error(`pu spawn printed no agent id: ${stdout}`);
    agentId = match[1];

    // Status streams from the machine's daemon: the agent's workspace appears under its project.
    await expect(agentRow(agentId)).toBeExisting({ wait: 60_000 });
    await browser.execute((selector) => {
      document
        .querySelector(selector)
        ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    }, agentRowSelector(agentId));
    const pane = $(`[data-testid="terminal-pane"][data-agent-id="${agentId}"] .xterm`);
    await expect(pane).toBeDisplayed({ wait: 30_000 });
    await browser.waitUntil(async () => (await screenText(agentId)).includes('ECHO READY'), {
      timeout: 60_000,
      timeoutMsg: 'the remote agent never showed ECHO READY',
    });

    // Input goes back to the machine's agent through the same relay.
    await pane.click();
    await browser.keys(['z']);
    await browser.waitUntil(() => existsSync(log) && readFileSync(log, 'utf8').includes('7a'), {
      timeout: 30_000,
      timeoutMsg: 'the remote agent never read the typed key',
    });
    await browser.waitUntil(async () => (await screenText(agentId)).includes('RX 7a'), {
      timeout: 30_000,
      timeoutMsg: 'the echo never came back',
    });
  });
});
