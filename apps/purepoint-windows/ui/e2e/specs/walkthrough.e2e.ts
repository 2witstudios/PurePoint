// The automated parity walkthrough (WIN-11.1): the iPad/Mac walkthrough adapted to Windows as ONE
// continuous user journey in one app session, broad and shallow. Each step checks the outcome a
// user would see and saves `ui/e2e/artifacts/walkthrough/NN-step.png`; the per-feature specs in
// this folder stay the deep checks, and this spec reuses their fixtures and patterns.
//
// The journey: open a project, the sidebar tree and its context menus, spawn a terminal in a
// worktree, type, resize, scroll, select and copy in the terminal, split panes and drag a tab, the
// file tree, edit and save with highlighting, the Markdown preview, review the changes and their
// diffs, the command palette, hotkeys and the menu bar, Schedule, the Agents hub, Triggers, the
// project channel, Settings, notifications and the taskbar badge, and a Point Guard chat answered
// by the loopback fixture provider (wdio.conf.ts gives this spec the Point Guard spec's runtime).
//
// Native surfaces WebDriver cannot see (the folder picker, toasts, the taskbar overlay) are
// driven through the same hooks the deep specs use, or checked by their closest observable effect.
// Only `terminal` agents run and every URL goes to the opener spy, so nothing calls a real AI or
// opens a browser.

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { workspace } from '../fixtures/paths.ts';
import { agentRow, agentRowSelector, projectRow, worktreeRow } from '../fixtures/sidebar.ts';

interface Probe {
  agentId: string;
  cols: number | null;
  rows: number | null;
}

interface HookWindow {
  __PU_DIALOG_SPY__?: (string | null)[];
  __PU_OPENER_SPY__?: string[];
  __PU_TERMINAL_STATS__?: boolean;
  __puTerminals: {
    list(): Probe[];
    text(agentId: string): string | null;
    locate(agentId: string, needle: string): { x: number; y: number } | null;
  };
  __puTabDrag?: DataTransfer;
  __TAURI_INTERNALS__: { invoke(cmd: string, args?: unknown): Promise<unknown> };
}

const WORKTREE = 'walk';
const TYPED = 'walkthroughtyped';
const APP_SOURCE = 'export const answer = 1;\n';
const GUIDE = '# Walkthrough guide\n\nSee [the site](https://example.com/walk) for more.\n';
const SHOTS = join(workspace.artifactsDir, 'walkthrough');
const TOMORROW = (() => {
  const date = new Date();
  date.setDate(date.getDate() + 1);
  return dayKey(date);
})();

/** The running step's number, the first two characters of its title. */
let stepNumber = '00';

/** Save this step's screenshot as `NN-<step>.png`. */
async function shot(step: string): Promise<void> {
  mkdirSync(SHOTS, { recursive: true });
  await browser.saveScreenshot(join(SHOTS, `${stepNumber}-${step}.png`));
}

function byText(text: string, tag = '*', attributes = '') {
  return $(`//${tag}[${attributes ? `${attributes} and ` : ''}normalize-space()="${text}"]`);
}

const sidebarMenu = () => $('[data-testid="sidebar-context-menu"]');
const palette = () => $('[data-testid="command-palette"]');
const treeRow = (name: string) => $(`[data-testid="file-tree-row"][aria-label="${name}"]`);
const barSelector = (leafId: number) =>
  `[data-testid="pane-tab-bar"][data-leaf-id="${String(leafId)}"]`;

async function menuTitles(): Promise<string[]> {
  await expect(sidebarMenu()).toBeDisplayed({ wait: 10_000 });
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll(
        '[data-testid="sidebar-context-menu"] [role="menuitem"], [data-testid="sidebar-context-menu"] [role="separator"]',
      ),
      (node) => (node.getAttribute('role') === 'separator' ? '---' : node.textContent),
    ),
  );
}

async function closeSidebarMenu(): Promise<void> {
  await browser.keys(['Escape']);
  await expect(sidebarMenu()).not.toBeExisting({ wait: 5000 });
}

async function agentRowIds(): Promise<string[]> {
  return browser.execute(() =>
    Array.from(
      document.querySelectorAll<HTMLElement>('[data-testid^="sidebar-row-agent-"][data-agent-id]'),
      (node) => node.dataset.agentId ?? '',
    ),
  );
}

/** Select a workspace row as the outline does (on mouse down). */
async function selectWorkspaceOf(agentId: string): Promise<void> {
  await expect(agentRow(agentId)).toBeExisting({ wait: 30_000 });
  await browser.execute((selector: string) => {
    document
      .querySelector(selector)
      ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
  }, agentRowSelector(agentId));
  await expect($('[data-testid="pane-grid"]')).toBeDisplayed();
}

async function probe(agentId: string): Promise<Probe | undefined> {
  const rows = await browser.execute(() => (window as unknown as HookWindow).__puTerminals.list());
  return rows.find((row) => row.agentId === agentId);
}

async function screenText(agentId: string): Promise<string> {
  return (
    (await browser.execute(
      (id) => (window as unknown as HookWindow).__puTerminals.text(id),
      agentId,
    )) ?? ''
  );
}

async function locate(agentId: string, needle: string): Promise<{ x: number; y: number } | null> {
  return browser.execute(
    (id, text) => (window as unknown as HookWindow).__puTerminals.locate(id, text),
    agentId,
    needle,
  );
}

async function doubleClickAt(point: { x: number; y: number }): Promise<void> {
  await browser.performActions([
    {
      type: 'pointer',
      id: 'mouse',
      parameters: { pointerType: 'mouse' },
      actions: [
        { type: 'pointerMove', duration: 0, origin: 'viewport', x: point.x, y: point.y },
        { type: 'pointerDown', button: 0 },
        { type: 'pointerUp', button: 0 },
        { type: 'pointerDown', button: 0 },
        { type: 'pointerUp', button: 0 },
      ],
    },
  ]);
  await browser.releaseActions();
}

async function wheelAt(point: { x: number; y: number }, deltaY: number): Promise<void> {
  await browser.performActions([
    {
      type: 'wheel',
      id: 'wheel',
      actions: [{ type: 'scroll', x: point.x, y: point.y, deltaX: 0, deltaY, duration: 100 }],
    },
  ] as never);
  await browser.releaseActions();
}

/** Tab IDs per pane, as the app shows them. */
async function shownTabs(): Promise<Record<number, number[]>> {
  return browser.execute(() => {
    const out: Record<number, number[]> = {};
    for (const bar of document.querySelectorAll<HTMLElement>('[data-testid="pane-tab-bar"]')) {
      out[Number(bar.dataset.leafId)] = Array.from(
        bar.querySelectorAll<HTMLElement>('[data-testid="pane-tab"]'),
        (tab) => Number(tab.dataset.surfaceId),
      );
    }
    return out;
  });
}

async function paneCount(): Promise<number> {
  return (await $$('[data-testid="pane-cell"]')).length;
}

/** A split or New Tab opens the palette for its empty tab (paneGrid.e2e.ts): close it. */
async function dismissEmptyTabPalette(): Promise<void> {
  await expect(palette()).toBeDisplayed({ wait: 15_000 });
  await browser.keys(['Escape']);
  await expect($('[data-testid="palette-backdrop"]')).not.toBeExisting({ wait: 10_000 });
}

/**
 * Drag a tab onto another pane's tab strip with synthesized HTML5 drag events: Chromium does not
 * start an HTML5 drag from WebDriver input (see paneGrid.e2e.ts).
 */
async function dragTabToStrip(surfaceId: number, leafId: number): Promise<string> {
  return browser.execute(
    (sourceSel: string, targetSel: string) => {
      const source = document.querySelector(sourceSel);
      const target = document.querySelector(targetSel);
      if (source === null || target === null) return 'no tab or strip';
      const dataTransfer = new DataTransfer();
      source.dispatchEvent(
        new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer }),
      );
      const rect = target.getBoundingClientRect();
      const init = {
        bubbles: true,
        cancelable: true,
        dataTransfer,
        clientX: rect.right - 2,
        clientY: rect.top + rect.height / 2,
      };
      target.dispatchEvent(new DragEvent('dragenter', init));
      const over = new DragEvent('dragover', init);
      target.dispatchEvent(over);
      if (!over.defaultPrevented) return 'the strip refused the tab';
      target.dispatchEvent(new DragEvent('drop', init));
      source.dispatchEvent(new DragEvent('dragend', init));
      return 'dropped';
    },
    `[data-testid="pane-tab"][data-surface-id="${String(surfaceId)}"]`,
    `${barSelector(leafId)} [role="tablist"]`,
  );
}

async function editorText(): Promise<string> {
  return browser.execute(() => document.querySelector('.cm-content')?.textContent ?? '');
}

async function focusMain(): Promise<void> {
  await browser.execute(() => {
    document.querySelector<HTMLElement>('main.detail-column')?.focus();
  });
}

async function navCurrent(item: 'dashboard' | 'agents' | 'schedule'): Promise<boolean> {
  return (await $(`[data-testid="sidebar-nav-${item}"]`).getAttribute('aria-current')) === 'page';
}

function dayKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

describe('Parity walkthrough', () => {
  const fixture = DaemonFixture.existing();
  let worktreePath = '';
  let shell = '';
  let fullCols = 0;

  before(async () => {
    await browser.execute(() => {
      const w = window as unknown as HookWindow;
      w.__PU_OPENER_SPY__ = [];
      w.__PU_TERMINAL_STATS__ = true;
    });
    // A weekly schedule from tomorrow (it never fires during the run), made before the Schedule
    // screen first loads.
    await fixture.pu([
      'schedule',
      'create',
      'walk-weekly',
      '--recurrence',
      'weekly',
      '--start-at',
      `${TOMORROW}T10:00:00`,
      '--trigger',
      'inline-prompt',
      '--trigger-prompt',
      'echo purepoint-walkthrough',
      '--agent',
      'terminal',
      '--root',
    ]);
  });

  beforeEach(function () {
    stepNumber = this.currentTest?.title.slice(0, 2) ?? '00';
  });

  it('01 opens a project', async () => {
    await expect(projectRow('e2e-project')).toBeDisplayed({ wait: 30_000 });
    // The native folder picker takes the queued answer (services/dialog.ts).
    const picked = join(fixture.home, 'walk-repo');
    mkdirSync(picked, { recursive: true });
    await fixture.gitIn(picked, 'init', '--quiet', '--initial-branch=main');
    await browser.execute((path: string) => {
      (window as unknown as HookWindow).__PU_DIALOG_SPY__ = [path];
    }, picked);
    await $('[aria-label="Sidebar"]').click();
    await browser.keys(['Control', 'Shift', 'o']);
    await expect(projectRow('walk-repo')).toBeDisplayed({ wait: 30_000 });
    await shot('open-project');
  });

  it('02 shows the sidebar tree and its context menus', async () => {
    // A worktree with a terminal agent, as `pu spawn` from any client makes it.
    const first = await fixture.spawnWorktreeTerminal(WORKTREE);
    await expect(worktreeRow(WORKTREE)).toBeDisplayed({ wait: 30_000 });
    await expect(agentRow(first)).toBeExisting({ wait: 30_000 });
    await expect($('[role="tree"][aria-label="Projects"]')).toBeDisplayed();

    await projectRow('e2e-project').click({ button: 'right' });
    expect((await menuTitles()).at(-1)).toBe('Remove Project…');
    await shot('sidebar-project-menu');
    await closeSidebarMenu();

    await agentRow(first).click({ button: 'right' });
    expect(await menuTitles()).toEqual(['Rename…', '---', 'Kill Agent']);
    await closeSidebarMenu();
  });

  it('03 spawns a terminal in a worktree', async () => {
    const { stdout } = await fixture.git('worktree', 'list', '--porcelain');
    worktreePath =
      stdout
        .split(/\r?\n\r?\n/)
        .find((block) => block.includes(`branch refs/heads/pu/${WORKTREE}`))
        ?.match(/^worktree (.+)$/m)?.[1] ?? '';
    expect(worktreePath).not.toBe('');

    const before = await agentRowIds();
    await worktreeRow(WORKTREE).moveTo();
    await worktreeRow(WORKTREE).$('button[aria-label="Add"]').click();
    expect(await menuTitles()).toEqual(['New Agent', 'New Terminal']);
    await $(
      '//*[@data-testid="sidebar-context-menu"]//*[@role="menuitem"][normalize-space()="New Terminal"]',
    ).click();
    await browser.waitUntil(async () => (await agentRowIds()).some((id) => !before.includes(id)), {
      timeout: 30_000,
      timeoutMsg: 'New Terminal never added an agent row',
    });
    shell = (await agentRowIds()).find((id) => !before.includes(id)) ?? '';
    await selectWorkspaceOf(shell);
    await expect($(`[data-testid="terminal-pane"][data-agent-id="${shell}"] .xterm`)).toBeDisplayed(
      { wait: 30_000 },
    );
    await shot('spawn-in-worktree');
  });

  it('04 types into the terminal', async () => {
    await browser.waitUntil(async () => (await screenText(shell)).trim() !== '', {
      timeout: 60_000,
      timeoutMsg: 'the shell never printed its prompt',
    });
    await $(`[data-testid="terminal-pane"][data-agent-id="${shell}"]`).click();
    await browser.keys(`echo ${TYPED}`.split(''));
    await browser.keys(['Enter']);
    await browser.waitUntil(async () => (await screenText(shell)).split(TYPED).length - 1 >= 2, {
      timeout: 30_000,
      timeoutMsg: `the shell never echoed ${TYPED}`,
    });
    await shot('terminal-type');
  });

  it('05 resizes the terminal with its pane', async () => {
    fullCols = (await probe(shell))?.cols ?? 0;
    expect(fullCols).toBeGreaterThan(0);
    // Hiding the sidebar widens the pane; xterm.js refits to more columns.
    await browser.keys(['Control', 'Shift', '\\']);
    await expect($('[aria-label="Sidebar"]')).not.toBeExisting({ wait: 5000 });
    await browser.waitUntil(async () => ((await probe(shell))?.cols ?? 0) > fullCols, {
      timeout: 10_000,
      timeoutMsg: 'the terminal did not widen with its pane',
    });
    await shot('terminal-resize');
    await browser.keys(['Control', 'Shift', '\\']);
    await expect($('[aria-label="Sidebar"]')).toBeDisplayed({ wait: 5000 });
    await browser.waitUntil(async () => (await probe(shell))?.cols === fullCols, {
      timeout: 10_000,
      timeoutMsg: 'the terminal did not narrow back',
    });
  });

  it('06 scrolls the terminal back', async () => {
    // PowerShell (the daemon's Windows shell) prints enough lines to push the echo off screen.
    await browser.keys('1..80 | % { "row$_" }'.split(''));
    await browser.keys(['Enter']);
    await browser.waitUntil(async () => (await screenText(shell)).includes('row80'), {
      timeout: 30_000,
      timeoutMsg: 'the shell never printed row80',
    });
    await browser.waitUntil(async () => (await locate(shell, TYPED)) === null, {
      timeout: 10_000,
      timeoutMsg: `${TYPED} stayed on screen`,
    });
    const screen = $(`[data-testid="terminal-pane"][data-agent-id="${shell}"] .xterm-screen`);
    const at = await screen.getLocation();
    const size = await screen.getSize();
    const centre = {
      x: Math.round(at.x + size.width / 2),
      y: Math.round(at.y + size.height / 2),
    };
    for (let i = 0; i < 60 && (await locate(shell, TYPED)) === null; i++) {
      await wheelAt(centre, -600);
    }
    expect(await locate(shell, TYPED)).not.toBeNull();
    await shot('terminal-scroll');
  });

  it('07 selects and copies terminal text', async () => {
    await browser.execute(() => navigator.clipboard.writeText('before'));
    const point = await locate(shell, TYPED);
    if (point === null) throw new Error(`${TYPED} is not on screen`);
    await doubleClickAt(point);
    await browser.keys(['Control', 'Shift', 'c']);
    await browser.waitUntil(
      async () => (await browser.execute(() => navigator.clipboard.readText())) === TYPED,
      { timeout: 5000, timeoutMsg: 'Ctrl+Shift+C did not copy the selection' },
    );
    await shot('terminal-copy');
  });

  it('08 splits panes and drags a tab between them', async () => {
    await $(`${barSelector(0)} button[aria-label="Split Right"]`).click();
    await browser.waitUntil(async () => (await paneCount()) === 2, {
      timeout: 15_000,
      timeoutMsg: 'Split Right did not make a second pane',
    });
    await dismissEmptyTabPalette();
    await $(`${barSelector(1)} button[aria-label="New Tab"]`).click();
    await dismissEmptyTabPalette();
    const tabs = await shownTabs();
    expect(tabs[1]?.length).toBe(2);
    const moved = tabs[1]?.at(-1) ?? -1;
    expect(await dragTabToStrip(moved, 0)).toBe('dropped');
    await browser.waitUntil(
      async () => {
        const now = await shownTabs();
        return now[0]?.at(-1) === moved && now[1]?.length === 1;
      },
      { timeout: 15_000, timeoutMsg: `the tab never moved: ${JSON.stringify(await shownTabs())}` },
    );
    await shot('pane-split-tab-drag');
  });

  it('09 browses the file tree', async () => {
    mkdirSync(join(worktreePath, 'src'), { recursive: true });
    mkdirSync(join(worktreePath, 'docs'), { recursive: true });
    writeFileSync(join(worktreePath, 'src', 'app.ts'), APP_SOURCE);
    writeFileSync(join(worktreePath, 'docs', 'guide.md'), GUIDE);
    await fixture.gitIn(worktreePath, 'add', 'src', 'docs');
    await fixture.gitIn(worktreePath, 'commit', '--quiet', '-m', 'Add walkthrough files');

    // Room for the tree and the editor side by side: close the split's pane (its tab is empty,
    // so it closes without asking) and hide the workspace sidebar.
    await $(`${barSelector(1)} button[aria-label="Close Pane"]`).click();
    await browser.waitUntil(async () => (await paneCount()) === 1, {
      timeout: 15_000,
      timeoutMsg: 'Close Pane did not close the empty pane',
    });
    const hide = $('button[aria-label="Hide Workspace Sidebar"]');
    if (await hide.isExisting()) await hide.click();
    await $(`${barSelector(0)} button[aria-label="New Tab"]`).click();
    await expect(palette()).toBeDisplayed({ wait: 15_000 });
    await browser.keys('Files'.split(''));
    await browser.waitUntil(
      async () =>
        (await $(
          '[data-testid="palette-row"][data-highlighted="true"] .palette-row-name',
        ).getText()) === 'Files',
      { timeout: 15_000, timeoutMsg: 'the palette never offered Files first' },
    );
    await browser.keys(['Enter']);
    await expect($('[data-testid="file-pane"]')).toBeDisplayed({ wait: 15_000 });
    for (const name of ['docs', 'src', 'README.md']) {
      await expect(treeRow(name)).toBeDisplayed({ wait: 20_000 });
    }
    await treeRow('src').click();
    await expect(treeRow('app.ts')).toBeDisplayed({ wait: 15_000 });
    await shot('file-tree');
  });

  it('10 edits and saves a highlighted file', async () => {
    await treeRow('app.ts').click();
    await expect($('[data-testid="file-pane-name"]')).toHaveText('app.ts', { wait: 15_000 });
    await browser.waitUntil(async () => (await editorText()).includes('export const answer'), {
      timeout: 20_000,
      timeoutMsg: 'app.ts never opened',
    });
    await browser.waitUntil(
      () =>
        browser.execute(() => document.querySelectorAll('.cm-content .pp-tok-keyword').length > 0),
      { timeout: 10_000, timeoutMsg: 'app.ts was never highlighted' },
    );
    await $('.cm-content').click();
    await browser.keys(['Control', 'End']);
    await browser.keys('// walkthrough'.split(''));
    await expect($('[data-testid="file-pane-dirty"]')).toBeDisplayed({ wait: 10_000 });
    await browser.keys(['Control', 's']);
    const file = join(worktreePath, 'src', 'app.ts');
    await browser.waitUntil(() => readFileSync(file, 'utf8').includes('// walkthrough'), {
      timeout: 15_000,
      timeoutMsg: `app.ts on disk is ${JSON.stringify(readFileSync(file, 'utf8'))}`,
    });
    await expect($('[data-testid="file-pane-dirty"]')).not.toBeExisting({ wait: 10_000 });
    await shot('edit-save');
  });

  it('11 previews Markdown', async () => {
    await treeRow('docs').click();
    await expect(treeRow('guide.md')).toBeDisplayed({ wait: 15_000 });
    await treeRow('guide.md').click();
    const frame = $('[data-testid="markdown-preview"]');
    await expect(frame).toBeDisplayed({ wait: 15_000 });
    await browser.switchFrame(await frame);
    try {
      await expect($('h1=Walkthrough guide')).toBeDisplayed({ wait: 10_000 });
      await expect($('a=the site')).toBeDisplayed();
    } finally {
      await browser.switchFrame(null);
    }
    await shot('markdown-preview');
  });

  it('12 reviews the changes and their diffs', async () => {
    await worktreeRow(WORKTREE).click();
    await expect($('[data-view="ReviewChangesView"]')).toBeDisplayed({ wait: 15_000 });
    for (const tab of ['Branch Changes', 'Uncommitted', 'Commits', 'PR Diffs']) {
      await expect(byText(tab, 'button', '@role="tab"')).toBeDisplayed();
    }
    await byText('Uncommitted', 'button', '@role="tab"').click();
    // The file's card header (the list also has a "Show … diff for" row per file).
    const expand = $('[aria-label="Expand diff for src/app.ts"]');
    const collapse = $('[aria-label="Collapse diff for src/app.ts"]');
    await browser.waitUntil(async () => (await expand.isExisting()) || collapse.isExisting(), {
      timeout: 30_000,
      timeoutMsg: 'the saved edit never showed',
    });
    if (await expand.isExisting()) await expand.click();
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() =>
            [...document.querySelectorAll('.diff-code')].map((el) => el.textContent),
          )
        ).some((line) => line.includes('// walkthrough')),
      { timeout: 20_000, timeoutMsg: 'the diff never showed the saved line' },
    );
    await shot('review-diff');
    await byText('Commits', 'button', '@role="tab"').click();
    await expect(byText('Add walkthrough files', 'span')).toBeDisplayed({ wait: 15_000 });
  });

  it('13 opens the command palette', async () => {
    await focusMain();
    await browser.keys(['Control', 'Shift', 'n']);
    await expect(palette()).toBeDisplayed({ wait: 15_000 });
    const names = await browser.execute(() =>
      Array.from(
        document.querySelectorAll('[data-testid="palette-row"] .palette-row-name'),
        (node) => node.textContent,
      ),
    );
    expect(names.slice(0, 5)).toEqual(['Claude', 'Codex', 'OpenCode', 'Terminal', 'Worktree']);
    await browser.keys('term'.split(''));
    await browser.waitUntil(async () => (await $$('[data-testid="palette-row"]').length) === 1, {
      timeout: 10_000,
      timeoutMsg: 'the filter did not narrow to Terminal',
    });
    await shot('command-palette');
    await browser.keys(['Escape']);
    await expect(palette()).not.toBeExisting({ wait: 10_000 });
  });

  it('14 runs hotkeys and the menu bar', async () => {
    await focusMain();
    await browser.keys(['Alt', 'Shift', '2']);
    await browser.waitUntil(() => navCurrent('agents'), { timeoutMsg: 'Alt+Shift+2' });
    await browser.keys(['Alt', 'Shift', '3']);
    await browser.waitUntil(() => navCurrent('schedule'), { timeoutMsg: 'Alt+Shift+3' });

    await $('[role="menubar"] button[data-menu="view"]').click();
    const menu = $('[role="menu"][aria-label="View"]');
    await expect(menu).toBeDisplayed({ wait: 5000 });
    await shot('menu-bar');
    await menu.$('//button[@role="menuitem"][.//span[normalize-space()="Toggle Sidebar"]]').click();
    await expect($('[aria-label="Sidebar"]')).not.toBeExisting({ wait: 5000 });
    await browser.keys(['Control', 'Shift', '\\']);
    await expect($('[aria-label="Sidebar"]')).toBeDisplayed({ wait: 5000 });
  });

  it('15 shows a schedule on the Schedule screen', async () => {
    const day = TOMORROW;
    await $('[data-testid="sidebar-nav-schedule"]').click();
    await expect($('[data-view="ScheduleView"]')).toBeDisplayed();
    await expect($('[data-view="WeekCalendarView"]')).toBeDisplayed();
    // Tomorrow is next week when today is the last day of the week.
    if (!(await $(`.schedule-week-day[data-date="${day}"]`).isExisting())) {
      await $('[aria-label="Next"]').click();
    }
    await expect(
      $(`[data-testid="schedule-event"][data-name="walk-weekly"][data-date="${day}"]`),
    ).toBeExisting({ wait: 15_000 });
    await shot('schedule');
  });

  it('16 creates a prompt in the Agents hub', async () => {
    await $('[data-testid="sidebar-nav-agents"]').click();
    await expect($('[data-testid="agents-hub"]')).toBeDisplayed();
    await $('button[role="tab"]=Prompts').click();
    await $('button[aria-label="New prompt"]').click();
    const dialog = $('[role="dialog"]');
    await dialog.$('[aria-label="Name"]').setValue('walk-prompt');
    await dialog.$('select[aria-label="Agent type"]').selectByAttribute('value', 'terminal');
    await dialog.$('[aria-label="Prompt body"]').setValue('echo from-walkthrough');
    await dialog.$('button=Create').click();
    await dialog.waitForExist({ reverse: true });
    await expect($('[data-testid="hub-row"][data-id$=":walk-prompt"]')).toBeDisplayed({
      wait: 15_000,
    });
    await shot('agents-hub');
  });

  it('17 creates a trigger', async () => {
    await $('button[role="tab"]=Triggers').click();
    await $('button[aria-label="New trigger"]').click();
    const dialog = $('[role="dialog"]');
    await dialog.$('[aria-label="Name"]').setValue('walk-idle');
    await dialog.$('button[role="radio"]=Agent Idle').click();
    await dialog.$('[aria-label="Inject text 1"]').setValue('echo walk-nudge');
    await expect($('[data-testid="trigger-preview"]')).toHaveText(
      expect.stringContaining('On Agent Idle →'),
    );
    await dialog.$('button=Create').click();
    await dialog.waitForExist({ reverse: true });
    await expect($('[data-testid="hub-row"][data-id$=":walk-idle"]')).toBeDisplayed({
      wait: 15_000,
    });
    const { stdout } = await fixture.pu(['trigger', 'list', '--json']);
    expect(stdout).toContain('walk-idle');
    await shot('triggers');
  });

  it('18 talks in the project channel', async () => {
    await projectRow('e2e-project').click();
    await expect($('[data-view="ProjectDetailView"]')).toBeDisplayed();
    await byText('Channel', 'button', '@role="tab"').click();
    await expect($('[data-view="ProjectChannelView"]')).toBeDisplayed();
    const text = `Walkthrough hello ${String(Date.now())}`;
    const editor = $('textarea[aria-label="Message channel"]');
    await editor.click();
    await editor.setValue(text);
    await browser.keys('Enter');
    await expect(byText(text, 'span')).toBeDisplayed({ wait: 30_000 });
    const { stdout } = await fixture.pu(['channel', 'read', '--json']);
    expect(stdout).toContain(text);
    await shot('channel');
  });

  it('19 changes Settings', async () => {
    await browser.keys(['Control', ',']);
    const settings = $('[data-testid="settings-view"]');
    await expect(settings).toBeDisplayed({ wait: 10_000 });
    await $('[data-testid="settings-section-display"]').click();
    const original =
      (await $('[role="radio"][aria-checked="true"]').getAttribute('data-value')) ?? 'system';
    await $('[role="radio"][data-value="dark"]').click();
    await browser.waitUntil(
      async () => (await browser.execute(() => document.documentElement.dataset.theme)) === 'dark',
      { timeoutMsg: 'the app never switched to dark' },
    );
    await shot('settings-dark');
    await $(`[role="radio"][data-value="${original}"]`).click();
    await $('[data-testid="settings-section-general"]').click();
    await expect(byText('Agent notifications')).toBeDisplayed({ wait: 10_000 });
  });

  it('20 shows the notification settings and sets the taskbar badge', async () => {
    // Toasts and the taskbar overlay are native surfaces WebDriver cannot read (WIN-11.3 checks
    // them by hand). Here: the notification toggles are on screen, and the badge command the
    // notifications service calls (`taskbar_set_badge`) succeeds on this window and clears.
    await expect(
      $('//label[contains(normalize-space(), "Completion notifications")]//input[@role="switch"]'),
    ).toBeExisting();
    await shot('notifications');
    const results = await browser.execute(async () => {
      const tauri = (window as unknown as HookWindow).__TAURI_INTERNALS__;
      const out: string[] = [];
      for (const count of [3, 0]) {
        try {
          await tauri.invoke('taskbar_set_badge', { count });
          out.push('ok');
        } catch (error) {
          out.push(String(error));
        }
      }
      return out;
    });
    expect(results).toEqual(['ok', 'ok']);
    await browser.keys('Escape');
    await $('[data-testid="settings-view"]').waitForExist({ reverse: true });
  });

  it('21 chats with Point Guard', async function () {
    this.timeout(300_000);
    await $('[data-testid="sidebar-nav-dashboard"]').click();
    await expect($('[data-testid="pg-chat"]')).toBeDisplayed({ wait: 30_000 });
    await $('.pg-connection-dot[data-connected="true"]').waitForExist({
      timeout: 240_000,
      timeoutMsg: 'Point Guard never connected (is the dev runtime built?)',
    });
    const composer = $('[data-testid="pg-composer-input"]');
    await composer.click();
    await composer.setValue('Hello fixture');
    await browser.keys('Enter');
    await expect($('[data-testid="pg-message-user"]')).toHaveText(
      expect.stringContaining('Hello fixture'),
      { wait: 30_000 },
    );
    await browser.waitUntil(
      async () => {
        const replies = await $$('[data-testid="pg-message-assistant"]');
        const last = replies.at(-1);
        return (
          last !== undefined &&
          (await last.getText()).includes('streamed from the loopback provider.')
        );
      },
      { timeout: 60_000, timeoutMsg: 'the fixture reply never arrived' },
    );
    await shot('point-guard');
  });

  after(async () => {
    // Nothing in the journey opened a URL.
    const opened = await browser.execute(
      () => (window as unknown as HookWindow).__PU_OPENER_SPY__ ?? [],
    );
    expect(opened).toEqual([]);
  });
});
