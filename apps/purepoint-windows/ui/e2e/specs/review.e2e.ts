// WIN-8.3: the review of a worktree. A 5,000-line diff scrolls top to bottom with no main-thread
// long task (`PerformanceObserver('longtask')`, i.e. none over 50 ms) while its pages load and
// virtualized rows come and go; the commit list shows the worktree's commit; and the
// Uncommitted tab picks up a file written to the working tree (DiffState's watcher and polling).

import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { browser, $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

const WORKTREE = 'review-wt';
const BIG_FILE = 'big.ts';
const BIG_LINES = 5000;

interface ScrollResult {
  steps: number;
  longTasks: number[];
  observed: boolean;
  lastLine: string | null;
  reachedEnd: boolean;
}

function byText(text: string, tag = '*', attributes = '') {
  return $(`//${tag}[${attributes ? `${attributes} and ` : ''}normalize-space()="${text}"]`);
}

describe('Review changes', () => {
  const fixture = DaemonFixture.current();
  let worktreePath = '';

  before(async () => {
    await fixture.spawnWorktreeTerminal(WORKTREE);
    const { stdout } = await fixture.git('worktree', 'list', '--porcelain');
    // Blocks of `worktree <path>` … `branch refs/heads/pu/<name>`.
    const path = stdout
      .split(/\r?\n\r?\n/)
      .find((block) => block.includes(`branch refs/heads/pu/${WORKTREE}`))
      ?.match(/^worktree (.+)$/m)?.[1];
    if (path === undefined) throw new Error(`no ${WORKTREE} worktree in:\n${stdout}`);
    worktreePath = path;
    const body = Array.from(
      { length: BIG_LINES },
      (_, i) =>
        `export const value${String(i + 1)} = "line ${String(i + 1)}"; // row ${String(i + 1)}`,
    ).join('\n');
    await writeFile(join(worktreePath, BIG_FILE), `${body}\n`);
    await fixture.gitIn(worktreePath, 'add', BIG_FILE);
    await fixture.gitIn(worktreePath, 'commit', '--quiet', '-m', 'Add a 5,000-line file');
  });

  it('opens the worktree review on its branch changes', async () => {
    // The worktree's agent shares its name, so look only in the project/worktree list.
    const row = $(`//ul[@data-testid="project-list"]//li[normalize-space()="${WORKTREE}"]`);
    await row.waitForDisplayed({ timeout: 30_000 });
    await row.click();
    await expect($('[data-view="ReviewChangesView"]')).toBeDisplayed();
    for (const tab of ['Branch Changes', 'Uncommitted', 'Commits', 'PR Diffs']) {
      await expect(byText(tab, 'button', '@role="tab"')).toBeDisplayed();
    }
    // 5,000 lines: the card starts collapsed, as large files do on the Mac.
    const header = $(`[aria-label="Expand diff for ${BIG_FILE}"]`);
    await header.waitForDisplayed({ timeout: 30_000 });
    await expect($('.diff-list-count')).toHaveText('1 changed files');
    await header.click();
    await browser.waitUntil(async () => (await $$('.diff-line').length) > 20, {
      timeout: 30_000,
      timeoutMsg: 'the first page did not draw',
    });
    // Highlighted: the grammar for .ts loaded and coloured the keywords.
    await browser.waitUntil(async () => (await $$('.diff-line .pp-tok-keyword').length) > 0, {
      timeout: 15_000,
      timeoutMsg: 'no highlighted tokens',
    });
  });

  it('scrolls a 5,000-line diff top to bottom with no long task', async () => {
    await browser.setTimeout({ script: 180_000 });
    const result: ScrollResult = await browser.execute(async (): Promise<ScrollResult> => {
      const scroller = document.querySelector<HTMLElement>('[data-testid="diff-list-scroll"]');
      if (scroller === null) {
        return { steps: 0, longTasks: [], observed: false, lastLine: null, reachedEnd: false };
      }
      const longTasks: number[] = [];
      let observed: boolean;
      try {
        new PerformanceObserver((list) => {
          for (const entry of list.getEntries()) longTasks.push(entry.duration);
        }).observe({ type: 'longtask', buffered: false });
        observed = PerformanceObserver.supportedEntryTypes.includes('longtask');
      } catch {
        observed = false;
      }
      const frame = () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              resolve();
            });
          });
        });
      const lastCode = () => {
        const codes = scroller.querySelectorAll('.diff-code');
        return codes.length === 0 ? null : (codes[codes.length - 1]?.textContent ?? null);
      };
      let steps = 0;
      let idle = 0;
      while (steps < 4000) {
        const atBottom = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2;
        const pending =
          scroller.querySelector('.diff-spinner') !== null ||
          [...scroller.querySelectorAll('button')].some((b) => b.textContent === 'More lines');
        if (atBottom && !pending) {
          idle += 1;
          if (idle > 10) break;
        } else {
          idle = 0;
        }
        scroller.scrollTop += Math.round(scroller.clientHeight * 0.8);
        steps += 1;
        await frame();
      }
      // Let the observer deliver the last entries.
      await new Promise((resolve) => setTimeout(resolve, 200));
      return {
        steps,
        longTasks,
        observed,
        lastLine: lastCode(),
        reachedEnd: scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2,
      };
    });
    console.log(
      `scrolled in ${String(result.steps)} steps; long tasks: ${JSON.stringify(result.longTasks)}`,
    );
    expect(result.observed).toBe(true);
    expect(result.reachedEnd).toBe(true);
    expect(result.lastLine).toBe(
      `export const value${String(BIG_LINES)} = "line ${String(BIG_LINES)}"; // row ${String(BIG_LINES)}`,
    );
    expect(result.longTasks).toEqual([]);
    // Virtualized: only the rows near the screen are in the DOM.
    expect(await $$('.diff-line').length).toBeLessThan(400);
    await expect(byText('Pages 1–25', 'span')).toBeExisting();
  });

  it('lists the worktree commit', async () => {
    await byText('Commits', 'button', '@role="tab"').click();
    await expect(byText('Add a 5,000-line file', 'span')).toBeDisplayed();
  });

  it('shows a file written to the working tree', async () => {
    await byText('Uncommitted', 'button', '@role="tab"').click();
    await expect($('[data-view="DiffListView"]')).toBeDisplayed();
    await writeFile(join(worktreePath, 'live.md'), '# Written while the review is open\n');
    await browser.waitUntil(
      async () =>
        (await $('[aria-label="Expand diff for live.md"]').isExisting()) ||
        (await $('[aria-label="Collapse diff for live.md"]').isExisting()),
      { timeout: 20_000, timeoutMsg: 'the new file did not appear' },
    );
    await expect(byText('Untracked', 'div', '@class="diff-group-title"')).toBeDisplayed();
    // Untracked files start collapsed; expanding one reads its contents.
    const header = $('[aria-label="Expand diff for live.md"]');
    if (await header.isExisting()) await header.click();
    await browser.waitUntil(
      async () =>
        (
          await browser.execute(() =>
            [...document.querySelectorAll('.diff-code')].map((el) => el.textContent),
          )
        ).includes('# Written while the review is open'),
      { timeout: 20_000, timeoutMsg: 'the new file did not open' },
    );
  });
});
