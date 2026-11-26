// WIN-8.10: the project channel against the real daemon. A message sent from the app's composer
// (Enter) is what `pu channel read --json` returns, and a message the CLI sends appears in the
// app's timeline on its next poll.

import { basename } from 'node:path';
import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';
import { projectRow } from '../fixtures/sidebar.ts';

interface ChannelReadJson {
  type: string;
  messages: { id: string; text: string; author: { id: string; kind: string } }[];
}

/** An element by its tag (or `*`) and exact, whitespace-normalized text. */
function byText(text: string, tag = '*', attributes = '') {
  return $(`//${tag}[${attributes ? `${attributes} and ` : ''}normalize-space()="${text}"]`);
}

describe('Project channel', () => {
  const fixture = DaemonFixture.current();
  const projectName = basename(fixture.projectRoot);
  const fromApp = `Hello from the app ${String(Date.now())}`;
  const fromCli = `Hello from the CLI ${String(Date.now())}`;

  async function readChannel(): Promise<ChannelReadJson> {
    const { stdout } = await fixture.pu(['channel', 'read', '--json']);
    return JSON.parse(stdout) as ChannelReadJson;
  }

  it('opens the channel from the project detail', async () => {
    const project = projectRow(projectName);
    await project.waitForDisplayed();
    await project.click();
    await expect($('[data-view="ProjectDetailView"]')).toBeDisplayed();
    await byText('Channel', 'button', '@role="tab"').click();
    await expect($('[data-view="ProjectChannelView"]')).toBeDisplayed();
    await expect(byText('The conversation starts here', 'span')).toBeDisplayed();
  });

  it('sends a message from the composer that the CLI reads', async () => {
    const editor = $('textarea[aria-label="Message channel"]');
    await editor.click();
    await editor.setValue(fromApp);
    await browser.keys('Enter');
    await browser.waitUntil(
      async () => (await readChannel()).messages.some((message) => message.text === fromApp),
      { timeout: 30_000, timeoutMsg: 'the CLI never read the message sent from the app' },
    );
    const sent = (await readChannel()).messages.find((message) => message.text === fromApp);
    expect(sent?.author.kind).toBe('human');
    // Sent and confirmed: the draft clears and the timeline shows it.
    await expect(editor).toHaveValue('');
    await expect(byText(fromApp, 'span')).toBeDisplayed();
  });

  it('shows a message the CLI sends', async () => {
    await fixture.pu(['channel', 'send', fromCli]);
    await byText(fromCli, 'span').waitForDisplayed({
      timeout: 30_000,
      timeoutMsg: 'the CLI message never appeared in the app',
    });
    // Both sides agree on one author identity for the local human.
    const history = await readChannel();
    const authors = new Set(
      history.messages
        .filter((message) => message.text === fromApp || message.text === fromCli)
        .map((message) => message.author.id),
    );
    expect(authors.size).toBe(1);
  });
});
