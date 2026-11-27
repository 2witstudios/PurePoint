// Sidebar menus and project drag (WIN-7.8) against the real daemon: the worktree add menu's New
// Terminal spawns an agent; a workspace row's context menu renames and kills it; a project row's
// context menu offers the Mac's entries; and dragging one project row onto another reorders the
// projects (`reorder_projects`).
//
// Right-clicks are real WebDriver clicks. The project drag is synthesized DOM drag events
// (`dragstart`, `dragenter`, `dragover`, `drop`, `dragend` with one script-made `DataTransfer`),
// as in paneGrid.e2e.ts: Chromium does not start an HTML5 drag from WebDriver input.

import { existsSync, mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { agentRow, agentRowSelector, projectRow, worktreeRow } from '../fixtures/sidebar.ts';

interface SpyWindow {
  __PU_DIALOG_SPY__?: (string | null)[];
}

interface ManifestAgent {
  readonly name?: string;
  readonly status?: string;
}

const menu = () => $('[data-testid="sidebar-context-menu"]');

async function menuTitles(): Promise<string[]> {
  await expect(menu()).toBeDisplayed({ wait: 10_000 });
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll(
        '[data-testid="sidebar-context-menu"] [role="menuitem"], [data-testid="sidebar-context-menu"] [role="separator"]',
      ),
      (node) =>
        node.getAttribute('role') === 'separator'
          ? '---'
          : `${node.textContent}${(node as HTMLButtonElement).disabled ? ' (off)' : ''}`,
    ),
  );
}

async function choose(title: string): Promise<void> {
  const item = await $(
    `//*[@data-testid="sidebar-context-menu"]//*[@role="menuitem"][normalize-space()="${title}"]`,
  );
  await item.click();
  await expect(menu()).not.toBeExisting({ wait: 5000 });
}

/** Agent ids of the workspace rows on screen. */
async function agentRowIds(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid^="sidebar-row-agent-"][data-agent-id]'),
      (node) => node.dataset.agentId ?? '',
    ),
  );
}

/** Project rows' names, top to bottom. */
async function projectOrder(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(document.querySelectorAll<HTMLElement>('[data-kind="project"]'))
      .sort((a, b) => a.offsetTop - b.offsetTop)
      .map((node) => (node.dataset.testid ?? '').replace('sidebar-row-project-', '')),
  );
}

describe('Sidebar menus', () => {
  const fixture = DaemonFixture.existing();
  const manifest = join(fixture.projectRoot, '.pu', 'manifest.json');
  let spawned = '';

  /** The agent's manifest entry, wherever it is (project root or a worktree). */
  function manifestAgent(agentId: string): ManifestAgent | null {
    if (!existsSync(manifest)) return null;
    const doc = JSON.parse(readFileSync(manifest, 'utf8')) as {
      agents?: Record<string, ManifestAgent>;
      worktrees?: Record<string, { agents?: Record<string, ManifestAgent> }>;
    };
    if (doc.agents?.[agentId] !== undefined) return doc.agents[agentId];
    for (const worktree of Object.values(doc.worktrees ?? {})) {
      const agent = worktree.agents?.[agentId];
      if (agent !== undefined) return agent;
    }
    return null;
  }

  before(async () => {
    await expect(projectRow('e2e-project')).toBeDisplayed({ wait: 30_000 });
    const first = await fixture.spawnWorktreeTerminal('menus');
    await expect(agentRow(first)).toBeExisting({ wait: 30_000 });
    await expect(worktreeRow('menus')).toBeDisplayed({ wait: 30_000 });
  });

  it('spawns a terminal from the worktree add menu', async () => {
    const before = await agentRowIds();
    await worktreeRow('menus').moveTo();
    await worktreeRow('menus').$('button[aria-label="Add"]').click();
    expect(await menuTitles()).toEqual(['New Agent', 'New Terminal']);
    await choose('New Terminal');
    await browser.waitUntil(async () => (await agentRowIds()).some((id) => !before.includes(id)), {
      timeout: 30_000,
      timeoutMsg: 'New Terminal never added an agent row',
    });
    spawned = (await agentRowIds()).find((id) => !before.includes(id)) ?? '';
    await browser.waitUntil(() => manifestAgent(spawned) !== null, {
      timeout: 15_000,
      timeoutMsg: `the daemon never recorded ${spawned}`,
    });
  });

  it('renames an agent from its context menu', async () => {
    await agentRow(spawned).click({ button: 'right' });
    expect(await menuTitles()).toEqual(['Rename…', '---', 'Kill Agent']);
    await choose('Rename…');
    const field = $('input.sidebar-rename');
    await expect(field).toBeFocused({ wait: 5000 });
    await browser.keys(['Control', 'a']);
    await browser.keys('menu-renamed'.split(''));
    await browser.keys(['Enter']);
    await expect($(`[data-testid="sidebar-row-agent-menu-renamed"]`)).toBeExisting({
      wait: 15_000,
    });
    await browser.waitUntil(() => manifestAgent(spawned)?.name === 'menu-renamed', {
      timeout: 15_000,
      timeoutMsg: `the daemon never renamed ${spawned}: ${JSON.stringify(manifestAgent(spawned))}`,
    });
  });

  it('kills an agent from its context menu', async () => {
    await agentRow(spawned).click({ button: 'right' });
    await menuTitles();
    await choose('Kill Agent');
    await browser.waitUntil(async () => !(await $(agentRowSelector(spawned)).isExisting()), {
      timeout: 15_000,
      timeoutMsg: 'the killed agent kept its row',
    });
    await browser.waitUntil(
      () => {
        const agent = manifestAgent(spawned);
        return agent === null || !['running', 'waiting', 'spawning'].includes(agent.status ?? '');
      },
      {
        timeout: 15_000,
        timeoutMsg: `the daemon never killed ${spawned}: ${JSON.stringify(manifestAgent(spawned))}`,
      },
    );
  });

  it('reorders two projects by dragging one onto the other', async () => {
    const second = join(fixture.home, 'second-project');
    mkdirSync(second, { recursive: true });
    await fixture.gitIn(second, 'init', '--quiet', '--initial-branch=main');
    await browser.execute((path: string) => {
      (window as unknown as SpyWindow).__PU_DIALOG_SPY__ = [path];
    }, second);
    await $('[aria-label="Sidebar"]').click();
    await browser.keys(['Control', 'Shift', 'o']);
    await expect(projectRow('second-project')).toBeDisplayed({ wait: 30_000 });
    expect(await projectOrder()).toEqual(['e2e-project', 'second-project']);

    // The menu offers the Mac's order entries: the top project cannot move up.
    await projectRow('e2e-project').click({ button: 'right' });
    const titles = await menuTitles();
    expect(titles.slice(-4)).toEqual(['Move Up (off)', 'Move Down', '---', 'Remove Project…']);
    await browser.keys(['Escape']);
    await expect(menu()).not.toBeExisting({ wait: 5000 });

    const outcome = await browser.execute(
      (sourceSel: string, targetSel: string) => {
        const source = document.querySelector(sourceSel);
        const target = document.querySelector(targetSel);
        if (source === null || target === null) return 'no rows';
        const dataTransfer = new DataTransfer();
        source.dispatchEvent(
          new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer }),
        );
        const rect = target.getBoundingClientRect();
        const init = {
          bubbles: true,
          cancelable: true,
          dataTransfer,
          clientX: rect.left + rect.width / 2,
          clientY: rect.top + rect.height * 0.25,
        };
        target.dispatchEvent(new DragEvent('dragenter', init));
        const over = new DragEvent('dragover', init);
        target.dispatchEvent(over);
        if (!over.defaultPrevented) return 'target refused the project';
        target.dispatchEvent(new DragEvent('drop', init));
        source.dispatchEvent(new DragEvent('dragend', init));
        return 'dropped';
      },
      '[data-testid="sidebar-row-project-second-project"]',
      '[data-testid="sidebar-row-project-e2e-project"]',
    );
    expect(outcome).toBe('dropped');
    await browser.waitUntil(
      async () => (await projectOrder()).join() === 'second-project,e2e-project',
      { timeout: 10_000, timeoutMsg: `the drop did not reorder: ${(await projectOrder()).join()}` },
    );
    // The daemon kept the order: nothing snaps back once it has answered and pushed it.
    await delay(3000);
    expect(await projectOrder()).toEqual(['second-project', 'e2e-project']);
  });
});
