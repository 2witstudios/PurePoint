// WIN-8.2: open a project detail and a worktree detail from the sidebar and check that every
// section title and action label of the Swift views is on screen.
//
// The lists below are extracted from apps/purepoint-macos/purepoint-macos/Views/Detail/
// {ProjectDetailView,RootCheckoutDetailView,WorktreeDetailView}.swift: `Text`, `Button` and
// `Label` titles and `.help` strings (the Windows views carry the help strings as aria-label and
// title). Dynamic text (names, counts) is checked separately. Views that are later leaves
// (ProjectChannelView, FileTreeSidebarView, the editor) contribute only the labels this leaf draws.

import { basename } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

const WORKTREE = 'e2e-wt';

/** ProjectDetailView.swift: the tab picker, the section, the root row, and the root back button. */
const PROJECT_OVERVIEW_LABELS = ['Overview', 'Channel', 'WORKTREES', 'Root checkout'];
const PROJECT_ROOT_CHECKOUT_LABELS = ['Back to overview'];
/** RootCheckoutDetailView.swift: header actions (the file tree starts shown). */
const ROOT_CHECKOUT_LABELS = ['Toggle changes view', 'Refresh', 'Hide file tree'];
/** WorktreeDetailView.swift: header actions, and the editor's empty state. */
const WORKTREE_LABELS = ['Show file tree', 'Files', 'Channel', 'Toggle changes view', 'Refresh'];
const WORKTREE_EDITOR_LABELS = ['Select a file to edit'];

/** Every text node and accessible label in the detail column. */
async function detailLabels(): Promise<string[]> {
  return browser.execute(() => {
    const root = document.querySelector('.detail-column');
    if (root === null) return [];
    const labels: string[] = [];
    for (const element of root.querySelectorAll('*')) {
      const label = element.getAttribute('aria-label');
      if (label !== null) labels.push(label);
      for (const node of element.childNodes) {
        if (node.nodeType === Node.TEXT_NODE) {
          const text = node.textContent?.trim();
          if (text) labels.push(text);
        }
      }
    }
    return labels;
  });
}

/** An element by its tag (or `*`) and exact, whitespace-normalized text. */
function byText(text: string, tag = '*', attributes = '') {
  return $(`//${tag}[${attributes ? `${attributes} and ` : ''}normalize-space()="${text}"]`);
}

async function expectLabels(expected: readonly string[]): Promise<void> {
  await browser.waitUntil(
    async () => {
      const labels = await detailLabels();
      return expected.every((label) => labels.includes(label));
    },
    { timeout: 15_000, timeoutMsg: `missing one of ${JSON.stringify(expected)}` },
  );
  const labels = await detailLabels();
  for (const label of expected) expect(labels).toContain(label);
}

describe('Project and worktree detail', () => {
  const fixture = DaemonFixture.current();
  const projectName = basename(fixture.projectRoot);

  before(async () => {
    await fixture.spawnWorktreeTerminal(WORKTREE);
  });

  it('opens the project detail from the sidebar', async () => {
    const project = $('[data-testid="project-list"]').$(`li=${projectName}`);
    await project.waitForDisplayed();
    await project.click();
    await expect($('[data-view="ProjectDetailView"]')).toBeDisplayed();
    await expectLabels([projectName, '1 worktrees', ...PROJECT_OVERVIEW_LABELS, WORKTREE]);
    // The review summaries load from git (the root has no commits beyond its base).
    await browser.waitUntil(
      async () => (await detailLabels()).some((label) => / commits · \d+ local files$/.test(label)),
      { timeout: 30_000, timeoutMsg: 'no review summary appeared' },
    );
  });

  it('switches to the channel and back', async () => {
    await byText('Channel', 'button', '@role="tab"').click();
    await expect($('[data-view="ProjectChannelView"]')).toBeDisplayed();
    await byText('Overview', 'button', '@role="tab"').click();
    await expect(byText('WORKTREES', 'span')).toBeDisplayed();
  });

  it('opens the root checkout and returns to the overview', async () => {
    await byText('Root checkout', 'span').click();
    await expect($('[data-view="RootCheckoutDetailView"]')).toBeDisplayed();
    await expectLabels([...PROJECT_ROOT_CHECKOUT_LABELS, ...ROOT_CHECKOUT_LABELS]);
    await byText('Back to overview', 'button').click();
    await expect(byText('WORKTREES', 'span')).toBeDisplayed();
  });

  it('opens the worktree detail from the project overview', async () => {
    await byText(WORKTREE, 'span', '@class="project-row-name"').click();
    await expect($('[data-view="WorktreeDetailView"]')).toBeDisplayed();
    await expect($('.detail-title')).toHaveText(WORKTREE);
  });

  it('opens the worktree detail from the sidebar', async () => {
    await $('[data-testid="project-list"]').$(`li=${projectName}`).click();
    await expect($('[data-view="ProjectDetailView"]')).toBeDisplayed();
    const row = $('[data-testid="worktree-row"]');
    await row.waitForDisplayed();
    await row.click();
    await expect($('[data-view="WorktreeDetailView"]')).toBeDisplayed();
    const path = (await row.getAttribute('title')) ?? fixture.projectRoot;
    const branch = (await fixture.gitIn(path, 'branch', '--show-current')).stdout.trim();
    await expectLabels([WORKTREE, branch, ...WORKTREE_LABELS]);
    await $('[aria-label="Toggle changes view"]').click();
    await expectLabels(WORKTREE_EDITOR_LABELS);
    await byText('Files', 'button').click();
    await expect($('[aria-label="Hide file tree"]')).toBeDisplayed();
  });
});
