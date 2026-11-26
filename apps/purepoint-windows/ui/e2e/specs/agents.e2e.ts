// Agents hub (WIN-8.7) against the fixture daemon: create a prompt and spawn from it, create a
// `terminal` agent definition, and run a two-agent swarm of it. Only `terminal` agents run, so
// nothing calls a real AI. Each step is checked in the UI and in `pu.exe --json` listings.

import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

interface SpyWindow {
  __PU_OPENER_SPY__?: string[];
}

const fixture = DaemonFixture.existing();

async function puJson(args: readonly string[]): Promise<unknown> {
  const { stdout } = await fixture.pu([...args, '--json']);
  return JSON.parse(stdout) as unknown;
}

function names(list: unknown, key: string): string[] {
  const items = (list as Record<string, unknown>)[key];
  return Array.isArray(items) ? items.map((item) => (item as { name: string }).name) : [];
}

const dialog = () => $('[role="dialog"]');

async function fill(label: string, value: string): Promise<void> {
  const field = dialog().$(`[aria-label="${label}"]`);
  await field.waitForDisplayed();
  await field.setValue(value);
}

async function clickButton(scope: ReturnType<typeof $>, name: string): Promise<void> {
  const button = scope.$(`button=${name}`);
  await button.waitForClickable();
  await button.click();
}

/** Click the hub's Spawn / Run button and return the agent ids it reports starting. */
async function launch(detailTestId: string, title: string): Promise<string[]> {
  const detail = $(`[data-testid="${detailTestId}"]`);
  await clickButton(detail, title);
  const spawned = detail.$('[data-testid="hub-spawned"]');
  await spawned.waitForDisplayed({ timeout: 60_000 });
  const text = await spawned.getText();
  return text
    .replace(/^Started\s+/, '')
    .split(',')
    .map((id) => id.trim())
    .filter((id) => id !== '');
}

async function statusText(): Promise<string> {
  return JSON.stringify(await puJson(['status']));
}

describe('Agents hub', () => {
  before(async () => {
    // Nothing here should open a URL; the spy keeps any that does inside the test.
    await browser.execute(() => {
      (window as unknown as SpyWindow).__PU_OPENER_SPY__ = [];
    });
    await $('button.sidebar-nav-row=Agents').click();
    await expect($('[data-testid="agents-hub"]')).toBeDisplayed();
  });

  it('creates a prompt and spawns a terminal agent from it', async () => {
    await $('button[role="tab"]=Prompts').click();
    await $('button[aria-label="New prompt"]').click();
    await fill('Name', 'e2e-prompt');
    await fill('Description', 'Echo from a prompt');
    await clickButton(dialog(), 'Project');
    await dialog().$('select[aria-label="Agent type"]').selectByAttribute('value', 'terminal');
    await fill('Prompt body', 'echo from-e2e-prompt');
    await clickButton(dialog(), 'Create');
    await dialog().waitForExist({ reverse: true });

    const row = $('[data-testid="hub-row"][data-id="local:e2e-prompt"]');
    await expect(row).toBeDisplayed();
    await expect(row).toHaveText(expect.stringContaining('e2e-project'));
    await expect($('[data-testid="prompt-detail"] textarea')).toHaveValue('echo from-e2e-prompt');

    const listed = await puJson(['prompt', 'list']);
    expect(names(listed, 'templates')).toContain('e2e-prompt');

    const [agentId] = await launch('prompt-detail', 'Spawn');
    expect(agentId).toMatch(/\S+/);
    await browser.waitUntil(async () => (await statusText()).includes(String(agentId)), {
      timeout: 30_000,
      timeoutMsg: `pu status never listed ${String(agentId)}`,
    });
    await expect($(`[data-testid="agent-list"] [data-agent-id="${String(agentId)}"]`)).toBeExisting(
      { wait: 30_000 },
    );
  });

  it('creates a terminal agent definition', async () => {
    await $('button[role="tab"]=Agents').click();
    await $('button[aria-label="New agent"]').click();
    await fill('Name', 'e2e-term');
    await dialog().$('select[aria-label="Agent type"]').selectByAttribute('value', 'terminal');
    await expect(dialog().$('[aria-label="Prompt source"]')).not.toBeExisting();
    await fill('Tags', 'e2e');
    await clickButton(dialog(), 'Create');
    await dialog().waitForExist({ reverse: true });

    await expect($('[data-testid="hub-row"][data-id="global:e2e-term"]')).toBeDisplayed();
    await expect($('[data-testid="agent-detail"]')).toHaveText(
      expect.stringContaining('terminal agent. Scope: global.'),
    );
    const listed = await puJson(['agent', 'list']);
    expect(names(listed, 'agent_defs')).toContain('e2e-term');
  });

  it('runs a two-agent swarm of terminal agents', async () => {
    await $('button[role="tab"]=Swarms').click();
    await $('button[aria-label="New swarm"]').click();
    await fill('Name', 'e2e-pair');
    await dialog().$('button[aria-label="Decrease Worktrees"]').click(); // 2 -> 1
    await expect(dialog().$('[aria-label="Worktrees"]')).toHaveValue('1');
    await clickButton(dialog(), 'Add Agent');
    await fill('Agent def 1', 'e2e-term');
    await fill('Role 1', 'shell');
    await dialog().$('button[aria-label="Increase Quantity 1"]').click();
    await expect(dialog().$('[aria-label="Quantity 1"]')).toHaveValue('2');
    // Just the roster: no extra terminal per worktree.
    await dialog().$('label=Include terminal').click();
    await expect(dialog().$('input[role="switch"]')).not.toBeSelected();
    await clickButton(dialog(), 'Create');
    await dialog().waitForExist({ reverse: true });

    const detail = $('[data-testid="swarm-detail"]');
    await expect(detail).toHaveText(expect.stringContaining('1 worktrees · 2 agents total'));
    await expect(detail).toHaveText(expect.stringContaining('No terminal'));
    const listed = await puJson(['swarm', 'list']);
    expect(names(listed, 'swarm_defs')).toContain('e2e-pair');

    const started = await launch('swarm-detail', 'Run');
    expect(started).toHaveLength(2);
    await browser.waitUntil(
      async () => {
        const status = await statusText();
        return started.every((id) => status.includes(id));
      },
      { timeout: 30_000, timeoutMsg: `pu status never listed ${started.join(', ')}` },
    );
    const status = await puJson(['status']);
    const worktree = (
      (status as { worktrees?: { name: string; agents: object }[] }).worktrees ?? []
    )
      .filter((wt) => started.some((id) => Object.hasOwn(wt.agents, id)))
      .map((wt) => wt.name);
    expect(worktree).toEqual(['e2e-pair-0']);
  });

  after(async () => {
    const opened = await browser.execute(
      () => (window as unknown as SpyWindow).__PU_OPENER_SPY__ ?? [],
    );
    expect(opened).toEqual([]);
  });
});
