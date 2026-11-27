// Triggers (WIN-8.9) against the fixture daemon: create an `agent_idle` trigger in the Agents
// hub, see it in `pu.exe trigger list --json` and in the hub's detail, assign it to the fixture's
// `terminal` agent (checked in the project manifest), then delete it.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

interface SpyWindow {
  __PU_OPENER_SPY__?: string[];
}

interface ListedTrigger {
  name: string;
  on: string;
  scope: string;
  description: string | null;
  sequence: { inject?: string | null; gate?: { run: string } | null }[];
}

const fixture = DaemonFixture.existing();
const dialog = () => $('[role="dialog"]');

async function listedTriggers(): Promise<ListedTrigger[]> {
  const { stdout } = await fixture.pu(['trigger', 'list', '--json']);
  const parsed = JSON.parse(stdout) as { triggers?: ListedTrigger[] };
  return parsed.triggers ?? [];
}

async function fill(label: string, value: string): Promise<void> {
  const field = dialog().$(`[aria-label="${label}"]`);
  await field.waitForDisplayed();
  await field.setValue(value);
}

/** The trigger the manifest records for an agent, wherever the agent sits in it. */
function manifestTrigger(agentId: string): string | null {
  const manifest = JSON.parse(
    readFileSync(join(fixture.projectRoot, '.pu', 'manifest.json'), 'utf8'),
  ) as unknown;
  let found: string | null = null;
  const visit = (value: unknown): void => {
    if (found !== null || typeof value !== 'object' || value === null) return;
    const record = value as Record<string, unknown>;
    if (record.id === agentId && typeof record.triggerName === 'string') {
      found = record.triggerName;
      return;
    }
    Object.values(record).forEach(visit);
  };
  visit(manifest);
  return found;
}

describe('Triggers', () => {
  before(async () => {
    await browser.execute(() => {
      (window as unknown as SpyWindow).__PU_OPENER_SPY__ = [];
    });
    await $('[data-testid="sidebar-nav-agents"]').click();
    await $('button[role="tab"]=Triggers').click();
  });

  it('creates an agent_idle trigger and lists it', async () => {
    await $('button[aria-label="New trigger"]').click();
    await fill('Name', 'e2e-idle');
    await fill('Description', 'Nudge an idle agent');
    await dialog().$('button[role="radio"]=Agent Idle').click();
    await dialog().$('select[aria-label="Scope"]').selectByAttribute('value', 'local');
    await fill('Inject text 1', 'echo e2e-nudge');
    await dialog().$('button=Add Step').click();
    await fill('Gate command 2', 'git status');
    await fill('Exit 2', '0');
    await expect($('[data-testid="trigger-preview"]')).toHaveText(
      'On Agent Idle → 2 steps (local)',
    );
    await dialog().$('button=Create').click();
    await dialog().waitForExist({ reverse: true });

    const row = $('[data-testid="hub-row"][data-id="local:e2e-idle"]');
    await expect(row).toBeDisplayed();
    await expect(row).toHaveText(expect.stringContaining('Agent Idle · 2 steps'));

    const listed = await listedTriggers();
    const trigger = listed.find((t) => t.name === 'e2e-idle');
    expect(trigger).toMatchObject({
      on: 'agent_idle',
      scope: 'local',
      description: 'Nudge an idle agent',
    });
    expect(trigger?.sequence).toHaveLength(2);
    expect(trigger?.sequence[0]?.inject).toBe('echo e2e-nudge');
    expect(trigger?.sequence[1]?.gate?.run).toBe('git status');
  });

  it('shows the trigger and assigns it to an agent', async () => {
    await $('[data-testid="hub-row"][data-id="local:e2e-idle"]').click();
    const detail = $('[data-testid="trigger-detail"]');
    await expect(detail).toHaveText(expect.stringContaining('Fires on Agent Idle'));
    await expect(detail.$$('[data-testid="trigger-step-row"]')).toBeElementsArrayOfSize(2);

    const picker = detail.$('select[aria-label="Agent"]');
    await picker.waitForExist({ timeout: 30_000 });
    const agentId = await picker.getValue();
    expect(agentId).toMatch(/\S+/);
    await detail.$('button=Assign').click();
    await expect(detail.$('[data-testid="trigger-assigned"]')).toHaveText(
      `Assigned to ${agentId} (2 steps)`,
    );
    await browser.waitUntil(() => manifestTrigger(agentId) === 'e2e-idle', {
      timeout: 15_000,
      timeoutMsg: `the manifest never recorded e2e-idle on ${agentId}`,
    });
  });

  it('deletes the trigger', async () => {
    await $('[data-testid="trigger-detail"]').$('button=Delete Trigger').click();
    await $('[data-testid="hub-row"][data-id="local:e2e-idle"]').waitForExist({ reverse: true });
    expect((await listedTriggers()).map((t) => t.name)).not.toContain('e2e-idle');
  });

  after(async () => {
    const opened = await browser.execute(
      () => (window as unknown as SpyWindow).__PU_OPENER_SPY__ ?? [],
    );
    expect(opened).toEqual([]);
  });
});
