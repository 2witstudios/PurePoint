// Files (WIN-8.4 – 8.6) against the fixture repository: the palette's Files row opens a file tab
// with the tree; the tree expands a folder and opens a file in the editor; an edit saved with
// Ctrl+S lands on disk; a write from outside reloads the clean file; a dirty file changed on disk
// shows the "changed on disk" banner and a save then shows the conflict banner; a Markdown file
// previews, and a link click in the preview reaches the opener spy (never a real browser).

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { agentRow, agentRowSelector } from '../fixtures/sidebar.ts';

interface SpyWindow {
  __PU_OPENER_SPY__?: string[];
}

const APP_SOURCE = 'export const answer = 1;\n';
const GUIDE = '# Guide\n\nSee [the site](https://example.com/docs) for more.\n';

const treeRow = (name: string) => $(`[data-testid="file-tree-row"][aria-label="${name}"]`);

async function editorText(): Promise<string> {
  return browser.execute(() => document.querySelector('.cm-content')?.textContent ?? '');
}

async function waitForEditorText(predicate: (text: string) => boolean, message: string) {
  await browser.waitUntil(async () => predicate(await editorText()), {
    timeout: 20_000,
    timeoutMsg: `${message}; the editor shows ${JSON.stringify(await editorText())}`,
  });
}

/** Puts the caret at the end of the document, ready to type. */
async function focusEditorEnd(): Promise<void> {
  await $('.cm-content').click();
  await browser.keys(['Control', 'End']);
}

async function save(): Promise<void> {
  await browser.keys(['Control', 's']);
}

describe('Files', () => {
  const fixture = DaemonFixture.existing();
  const appFile = join(fixture.projectRoot, 'src', 'app.ts');
  const guideFile = join(fixture.projectRoot, 'docs', 'guide.md');

  before(async () => {
    mkdirSync(join(fixture.projectRoot, 'src'), { recursive: true });
    mkdirSync(join(fixture.projectRoot, 'docs'), { recursive: true });
    writeFileSync(appFile, APP_SOURCE);
    writeFileSync(guideFile, GUIDE);
    const agent = await fixture.spawnTerminal();
    await expect(agentRow(agent)).toBeExisting({ wait: 30_000 });
    await browser.execute((selector: string) => {
      document
        .querySelector(selector)
        ?.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 }));
    }, agentRowSelector(agent));
    await expect($('[data-testid="pane-grid"]')).toBeDisplayed();
    await browser.execute(() => {
      (window as SpyWindow).__PU_OPENER_SPY__ = [];
    });
  });

  it("browses the project's tree in a file tab and opens a file", async () => {
    await $('[data-testid="pane-tab-bar"] button[aria-label="New Tab"]').click();
    await expect($('[data-testid="command-palette"]')).toBeDisplayed({ wait: 15_000 });
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

    // Folders first, then files; .git is never listed (.pu, a dot folder, is).
    let names: string[] = [];
    await browser.waitUntil(
      async () => {
        names = await browser.execute(() =>
          Array.from(
            document.querySelectorAll('[data-testid="file-tree-row"]'),
            (row) => row.getAttribute('aria-label') ?? '',
          ),
        );
        return ['docs', 'src', 'README.md'].every((name) => names.includes(name));
      },
      { timeout: 20_000, timeoutMsg: `the tree listed ${JSON.stringify(names)}` },
    );
    expect(names).not.toContain('.git');
    expect(names.indexOf('src')).toBeLessThan(names.indexOf('README.md'));
    await treeRow('src').click();
    await expect(treeRow('app.ts')).toBeDisplayed({ wait: 15_000 });
    await treeRow('app.ts').click();
    await expect($('[data-testid="file-pane-name"]')).toHaveText('app.ts', { wait: 15_000 });
    await waitForEditorText((text) => text.includes('export const answer = 1;'), 'app.ts opened');
    // Line numbers and highlighting.
    await expect($('.cm-lineNumbers')).toBeDisplayed();
    await browser.waitUntil(
      () =>
        browser.execute(() => document.querySelectorAll('.cm-content .pp-tok-keyword').length > 0),
      { timeout: 10_000, timeoutMsg: 'app.ts was never highlighted' },
    );
  });

  it('saves an edit to disk with Ctrl+S', async () => {
    await focusEditorEnd();
    await browser.keys('// saved'.split(''));
    await expect($('[data-testid="file-pane-dirty"]')).toBeDisplayed({ wait: 10_000 });
    await save();
    await browser.waitUntil(() => readFileSync(appFile, 'utf8').includes('// saved'), {
      timeout: 15_000,
      timeoutMsg: `app.ts on disk is ${JSON.stringify(readFileSync(appFile, 'utf8'))}`,
    });
    expect(readFileSync(appFile, 'utf8')).toBe(`${APP_SOURCE}// saved`);
    await expect($('[data-testid="file-pane-dirty"]')).not.toBeExisting({ wait: 10_000 });
  });

  it('reloads a clean file written outside the app', async () => {
    writeFileSync(appFile, 'export const answer = 2;\n');
    await waitForEditorText(
      (text) => text.includes('answer = 2') && !text.includes('// saved'),
      'the external write never reloaded',
    );
    await expect($('[data-testid="file-pane-dirty"]')).not.toBeExisting();
  });

  it('warns about a dirty file changed on disk, and refuses to save over it', async () => {
    await focusEditorEnd();
    await browser.keys('// mine'.split(''));
    await expect($('[data-testid="file-pane-dirty"]')).toBeDisplayed({ wait: 10_000 });
    writeFileSync(appFile, 'export const answer = 3;\n');
    await expect($('[data-testid="editor-external-change"]')).toBeDisplayed({ wait: 20_000 });
    await expect($('[data-testid="editor-external-change"]')).toHaveText(
      expect.stringContaining('File changed on disk.'),
    );
    // The edits are kept.
    expect(await editorText()).toContain('// mine');

    await $('.cm-content').click();
    await save();
    await expect($('[data-testid="editor-save-conflict"]')).toBeDisplayed({ wait: 15_000 });
    await expect($('[data-testid="editor-save-conflict"]')).toHaveText(
      expect.stringContaining('app.ts changed on disk since it was opened.'),
    );
    expect(readFileSync(appFile, 'utf8')).toBe('export const answer = 3;\n');

    await $('[data-testid="editor-save-conflict"]').$('button=Reload').click();
    await waitForEditorText(
      (text) => text.includes('answer = 3') && !text.includes('// mine'),
      'Reload never showed the file on disk',
    );
    await expect($('[data-testid="editor-save-conflict"]')).not.toBeExisting({ wait: 10_000 });
  });

  it('previews Markdown and hands a link click to the opener', async () => {
    await treeRow('docs').click();
    await expect(treeRow('guide.md')).toBeDisplayed({ wait: 15_000 });
    await treeRow('guide.md').click();
    const frame = $('[data-testid="markdown-preview"]');
    await expect(frame).toBeDisplayed({ wait: 15_000 });
    await expect($('[role="radio"][aria-checked="true"]')).toHaveText('Preview');
    expect(await frame.getAttribute('sandbox')).toBe('allow-same-origin');

    await browser.switchFrame(await frame);
    try {
      const link = $('a=the site');
      await expect(link).toBeDisplayed({ wait: 10_000 });
      await link.click();
    } finally {
      await browser.switchFrame(null);
    }
    await browser.waitUntil(
      async () =>
        (await browser.execute(() => (window as SpyWindow).__PU_OPENER_SPY__ ?? [])).includes(
          'https://example.com/docs',
        ),
      { timeout: 10_000, timeoutMsg: 'the link click never reached the opener' },
    );
    // The frame stayed on the preview.
    await expect($('[data-testid="markdown-preview"]')).toBeDisplayed();

    await $('[role="radio"]=Code').click();
    await waitForEditorText((text) => text.includes('# Guide'), 'Code never showed the source');
  });
});
