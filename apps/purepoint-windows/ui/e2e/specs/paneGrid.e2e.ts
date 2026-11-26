// The pane grid (WIN-7.4) against real `terminal` agents: a workspace row shows its grid; the tab
// strip splits and closes panes; the divider drags; tabs reorder, move and split off by HTML5 drag
// and drop; the layout saves to `.pu/workspaces.json` (a valid version 4 document) and comes back
// after a relaunch; and `pu grid` run from the CLI changes the app's layout.
//
// Tab drags are synthesized DOM drag events (`dragstart`, `dragenter`, `dragover`, `drop`,
// `dragend`, each carrying one script-made `DataTransfer`), dispatched in the page: msedgedriver's
// WebDriver actions move the real pointer, but Chromium does not start an HTML5 drag from
// synthetic WebDriver input, so a pointer-action drag never reaches `dragstart`. The divider drag,
// which is plain pointer events, uses real WebDriver pointer actions.

import { readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { agentRow, agentRowSelector } from '../fixtures/sidebar.ts';
import {
  CURRENT_DOCUMENT_VERSION,
  decodeWorkspaceDocument,
  workspacesFromText,
} from '../../src/model/workspacePersistence.ts';
import { allLeafIds, type PaneSplitNode } from '../../src/model/paneSplit.ts';
import type { Workspace } from '../../src/model/workspace.ts';

type Zone = 'left' | 'right' | 'top' | 'bottom' | 'center';

/** Where a synthesized drop lands: a tab, a pane's tab strip, or a zone of a pane's content. */
type DropTarget =
  | { readonly kind: 'tab'; readonly surfaceId: number }
  | { readonly kind: 'strip'; readonly leafId: number }
  | { readonly kind: 'pane'; readonly leafId: number; readonly zone: Zone };

interface DragWindow {
  __puTabDrag?: DataTransfer;
}

const cells = () => $$('[data-testid="pane-cell"]');
const tabSelector = (surfaceId: number) =>
  `[data-testid="pane-tab"][data-surface-id="${String(surfaceId)}"]`;
const barSelector = (leafId: number) =>
  `[data-testid="pane-tab-bar"][data-leaf-id="${String(leafId)}"]`;

async function paneCount(): Promise<number> {
  return (await cells()).length;
}

async function waitForPaneCount(count: number, message: string): Promise<void> {
  await browser.waitUntil(async () => (await paneCount()) === count, {
    timeout: 15_000,
    timeoutMsg: `${message}: expected ${String(count)} panes, saw ${String(await paneCount())}`,
  });
}

/** Tab IDs per pane, in tree order, as the app shows them. */
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

async function waitForTabs(expected: Record<number, number[]>, message: string): Promise<void> {
  await browser.waitUntil(
    async () => JSON.stringify(await shownTabs()) === JSON.stringify(expected),
    {
      timeout: 15_000,
      timeoutMsg: `${message}: expected ${JSON.stringify(expected)}, saw ${JSON.stringify(await shownTabs())}`,
    },
  );
}

/** Drag a tab to a target with synthesized HTML5 drag events (see the header). */
async function dragTab(surfaceId: number, target: DropTarget): Promise<void> {
  await browser.execute((selector: string) => {
    const source = document.querySelector(selector);
    if (source === null) throw new Error(`no tab ${selector}`);
    const dataTransfer = new DataTransfer();
    (window as unknown as DragWindow).__puTabDrag = dataTransfer;
    source.dispatchEvent(
      new DragEvent('dragstart', { bubbles: true, cancelable: true, dataTransfer }),
    );
  }, tabSelector(surfaceId));
  // The panes cover their content with drop overlays once React has seen the drag start.
  if (target.kind === 'pane') {
    await $(
      `[data-testid="pane-drop-overlay"][data-leaf-id="${String(target.leafId)}"]`,
    ).waitForExist({
      timeout: 5000,
    });
  }
  const selector =
    target.kind === 'tab'
      ? tabSelector(target.surfaceId)
      : target.kind === 'strip'
        ? `${barSelector(target.leafId)} [role="tablist"]`
        : `[data-testid="pane-drop-overlay"][data-leaf-id="${String(target.leafId)}"]`;
  const zone = target.kind === 'pane' ? target.zone : 'center';
  const sourceSelector = tabSelector(surfaceId);
  const outcome = await browser.execute(
    (targetSelector: string, where: Zone, sourceSel: string) => {
      const element = document.querySelector(targetSelector);
      const dataTransfer = (window as unknown as DragWindow).__puTabDrag;
      if (element === null || dataTransfer === undefined) return 'no target';
      const rect = element.getBoundingClientRect();
      // Inside a strip, aim past the last tab; inside a pane, 5% in from the zone's edge.
      const x =
        where === 'left'
          ? rect.left + rect.width * 0.05
          : where === 'right'
            ? rect.right - rect.width * 0.05
            : targetSelector.includes('tablist')
              ? rect.right - 2
              : rect.left + rect.width / 2;
      const y =
        where === 'top'
          ? rect.top + rect.height * 0.05
          : where === 'bottom'
            ? rect.bottom - rect.height * 0.05
            : rect.top + rect.height / 2;
      const init = { bubbles: true, cancelable: true, dataTransfer, clientX: x, clientY: y };
      element.dispatchEvent(new DragEvent('dragenter', init));
      const over = new DragEvent('dragover', init);
      element.dispatchEvent(over);
      if (!over.defaultPrevented) return 'target refused the tab';
      element.dispatchEvent(new DragEvent('drop', init));
      document.querySelector(sourceSel)?.dispatchEvent(new DragEvent('dragend', init));
      delete (window as unknown as DragWindow).__puTabDrag;
      return 'dropped';
    },
    selector,
    zone,
    sourceSelector,
  );
  expect(outcome).toBe('dropped');
}

async function clickBarButton(leafId: number, label: string): Promise<void> {
  await $(`${barSelector(leafId)} button[aria-label="${label}"]`).click();
}

describe('Pane grid', () => {
  const fixture = DaemonFixture.existing();
  const layoutFile = join(fixture.projectRoot, '.pu', 'workspaces.json');
  let first = '';
  let second = '';
  let workspaceId = '';

  /** The layout document on disk: checked to be a valid version 4 document, then decoded. */
  function layoutOnDisk(): Workspace | null {
    if (!existsSync(layoutFile)) return null;
    const text = readFileSync(layoutFile, 'utf8');
    const document = decodeWorkspaceDocument(text);
    if (document === null) throw new Error(`.pu/workspaces.json does not decode: ${text}`);
    expect(document.version).toBe(CURRENT_DOCUMENT_VERSION);
    return workspacesFromText(text, fixture.projectRoot).find((w) => w.id === workspaceId) ?? null;
  }

  async function waitForSavedLeaves(count: number): Promise<Workspace> {
    let saved: Workspace | null = null;
    await browser.waitUntil(
      () => {
        saved = layoutOnDisk();
        return saved !== null && allLeafIds(saved.root).length === count;
      },
      { timeout: 15_000, timeoutMsg: `the saved layout never had ${String(count)} panes` },
    );
    return saved as unknown as Workspace;
  }

  async function selectWorkspaceOf(agentId: string): Promise<void> {
    await expect(agentRow(agentId)).toBeExisting({ wait: 30_000 });
    await browser.execute((selector: string) => {
      document
        .querySelector(selector)
        ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    }, agentRowSelector(agentId));
    await expect($('[data-testid="pane-grid"]')).toBeDisplayed();
    workspaceId = (await $('[data-testid="pane-grid"]').getAttribute('data-workspace-id')) ?? '';
  }

  before(async () => {
    first = await fixture.spawnTerminal();
    second = await fixture.spawnTerminal();
  });

  it("shows a selected workspace row's pane grid with its terminal", async () => {
    await selectWorkspaceOf(first);
    expect(await paneCount()).toBe(1);
    await expect(
      $(`[data-testid="terminal-pane"][data-agent-id="${first}"] .xterm`),
    ).toBeDisplayed();
    // The strip is always there, even for one tab.
    expect((await shownTabs())[0]).toEqual([0]);
  });

  it('splits from the tab strip and drags the divider', async () => {
    await clickBarButton(0, 'Split Right');
    await waitForPaneCount(2, 'Split Right');
    await expect($('[data-testid="pane-empty-tab"]')).toBeDisplayed();

    const divider = await $('[data-testid="pane-divider"]');
    expect(await divider.getAttribute('aria-valuenow')).toBe('50');
    const location = await divider.getLocation();
    const size = await divider.getSize();
    const x = Math.round(location.x + size.width / 2);
    const y = Math.round(location.y + size.height / 2);
    await browser.performActions([
      {
        type: 'pointer',
        id: 'mouse',
        parameters: { pointerType: 'mouse' },
        actions: [
          { type: 'pointerMove', duration: 0, origin: 'viewport', x, y },
          { type: 'pointerDown', button: 0 },
          { type: 'pointerMove', duration: 200, origin: 'viewport', x: x - 120, y },
          { type: 'pointerUp', button: 0 },
        ],
      },
    ]);
    await browser.releaseActions();
    await browser.waitUntil(async () => Number(await divider.getAttribute('aria-valuenow')) < 50, {
      timeout: 5000,
      timeoutMsg: 'dragging the divider left did not shrink the first pane',
    });
    const saved = await waitForSavedLeaves(2);
    const root = saved.root as PaneSplitNode & { type: 'split' };
    await browser.waitUntil(
      () => {
        const current = layoutOnDisk()?.root;
        return current?.type === 'split' && current.ratio < 0.5;
      },
      { timeout: 15_000, timeoutMsg: 'the dragged ratio was not saved' },
    );
    expect(root.axis).toBe('vertical');
  });

  it('moves tabs by drag and drop: reorder, to another pane, and onto an edge', async () => {
    // `pu grid assign` puts the second agent in the focused (new) pane.
    await fixture.pu(['grid', 'assign', second]);
    await expect($(`[data-testid="terminal-pane"][data-agent-id="${second}"]`)).toBeExisting({
      wait: 15_000,
    });
    await waitForTabs({ 0: [0], 1: [1] }, 'assign');
    await clickBarButton(1, 'New Tab');
    await waitForTabs({ 0: [0], 1: [1, 2] }, 'New Tab');

    await dragTab(2, { kind: 'tab', surfaceId: 1 });
    await waitForTabs({ 0: [0], 1: [2, 1] }, 'reorder within a pane');

    await dragTab(1, { kind: 'strip', leafId: 0 });
    await waitForTabs({ 0: [0, 1], 1: [2] }, 'move to another pane');

    await dragTab(1, { kind: 'pane', leafId: 1, zone: 'bottom' });
    await waitForPaneCount(3, 'drop on the bottom edge');
    await waitForTabs({ 0: [0], 1: [2], 2: [1] }, 'split off on an edge');
    const saved = await waitForSavedLeaves(3);
    expect(saved.panes[2]?.tabs.map((tab) => tab.id)).toEqual([1]);
  });

  it('asks before closing a pane with two running agents', async () => {
    await dragTab(1, { kind: 'pane', leafId: 0, zone: 'center' });
    await waitForTabs({ 0: [0, 1], 1: [2] }, 'drop in the centre');
    await clickBarButton(0, 'Close Pane');
    const dialog = await $('[data-testid="close-confirmation"]');
    await expect(dialog).toBeDisplayed();
    await expect(dialog).toHaveText(expect.stringContaining('2 running agents will be stopped.'));
    await dialog.$('button=Cancel').click();
    await expect(dialog).not.toBeExisting();
    expect(await paneCount()).toBe(2);
  });

  it('restores the saved layout on relaunch', async () => {
    const saved = await waitForSavedLeaves(2);
    await browser.reloadSession();
    await selectWorkspaceOf(first);
    await waitForPaneCount(2, 'after relaunch');
    await waitForTabs({ 0: [0, 1], 1: [2] }, 'after relaunch');
    const restored = layoutOnDisk();
    expect(restored?.root).toEqual(saved.root);
    // Pane 0 shows its active tab, the second agent (the last tab moved there).
    await expect(
      $(
        `[data-testid="pane-cell"][data-leaf-id="0"] [data-testid="terminal-pane"][data-agent-id="${second}"] .xterm`,
      ),
    ).toBeDisplayed();
  });

  it('follows pu grid show, split, focus, assign and close from the CLI', async () => {
    const shown = await fixture.pu(['grid', 'show']);
    expect(shown.stdout).toContain(workspaceId);

    await fixture.pu(['grid', 'split', '--axis', 'h', '--leaf', '1']);
    await waitForPaneCount(3, 'pu grid split');
    const newLeaf = await browser.execute(
      () =>
        document.querySelector<HTMLElement>('[data-testid="pane-cell"][data-focused="true"]')
          ?.dataset.leafId,
    );
    expect(newLeaf).toBe('3');

    await fixture.pu(['grid', 'focus', '--leaf', '0']);
    await expect(
      $('[data-testid="pane-cell"][data-leaf-id="0"][data-focused="true"]'),
    ).toBeExisting({
      wait: 10_000,
    });

    // Assign the second agent to the new pane: it leaves pane 0 (one tab per agent).
    await fixture.pu(['grid', 'assign', second, '--leaf', '3']);
    await waitForTabs({ 0: [0], 1: [2], 3: [3] }, 'pu grid assign');
    await expect(
      $(
        `[data-testid="pane-cell"][data-leaf-id="3"] [data-testid="terminal-pane"][data-agent-id="${second}"]`,
      ),
    ).toBeExisting({ wait: 10_000 });

    await fixture.pu(['grid', 'close', '--leaf', '1']);
    await waitForPaneCount(2, 'pu grid close');
    await browser.waitUntil(
      () => {
        const saved = layoutOnDisk();
        return saved !== null && allLeafIds(saved.root).join() === '0,3';
      },
      { timeout: 15_000, timeoutMsg: 'the saved layout never became panes 0 and 3' },
    );
    const json = JSON.parse((await fixture.pu(['grid', 'show', '--json'])).stdout) as {
      version: number;
    };
    expect(json.version).toBe(CURRENT_DOCUMENT_VERSION);
  });
});
