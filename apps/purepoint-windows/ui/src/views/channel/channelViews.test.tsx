// WIN-8.10: ProjectChannelView, ChannelComposer, ChannelHoverRow and ChannelReferenceView over a
// scripted daemon (no backend). The fixture snapshot renders docs/reference/fixtures/
// channel-history.json, the history the Swift wire tests decode, in en-US and UTC.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import fixture from '../../../../../../docs/reference/fixtures/channel-history.json?raw';
import type { Request, Response } from '../../protocol';
import { inTimeZone } from '../../test/timeZone';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => new Promise(() => undefined)) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));
const gitCalls: string[] = [];
vi.mock('../../services/git.ts', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../services/git.ts')>()),
  fetchCommitDiff: (root: string, sha: string) => {
    gitCalls.push(`commit ${root} ${sha}`);
    return Promise.resolve([]);
  },
  fetchPRDiff: (cwd: string, prNumber: number) => {
    gitCalls.push(`pr ${cwd} ${String(prNumber)}`);
    return Promise.resolve({ files: [] });
  },
}));
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView = () => undefined;

const { ProjectChannelView } = await import('./ProjectChannelView.tsx');
const { ChannelComposer } = await import('./ChannelComposer.tsx');
const { ProjectState } = await import('../../state/projectState.ts');
const { ChannelState, memoryChannelStorage } = await import('../../state/channelState.ts');
const { FakeChannelDaemon } = await import('../../state/channelState.fixtures.ts');
const { makeAgentModel, makeWorktreeModel } = await import('../../model/workspaceModel.ts');

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error('missing test value');
  return value;
}

const ROOT = 'C:\\work\\demo';
const history = JSON.parse(fixture) as Response;

function project(send: (request: Request) => Promise<Response>) {
  const channel = new ChannelState({
    projectRoot: ROOT,
    storage: memoryChannelStorage(),
    sendRequest: send,
    sleep: (_ms, signal) =>
      new Promise((resolve) => {
        signal.addEventListener('abort', () => {
          resolve();
        });
      }),
  });
  return new ProjectState({
    projectRoot: ROOT,
    client: { send: () => Promise.resolve({ type: 'ok' }) },
    channel,
  });
}

const agent = (id: string, name: string, agentType: string) =>
  makeAgentModel({ id, name, agentType, status: 'running', prompt: '', startedAt: '' });
const worktree = (id: string, path: string, agents: ReturnType<typeof agent>[] = []) =>
  makeWorktreeModel({ id, name: id, path, branch: `pu/${id}`, status: 'active', agents });

function fixtureProject() {
  const requests: Request[] = [];
  const state = project((request) => {
    requests.push(request);
    if (request.type === 'channel_read') return Promise.resolve(history);
    return Promise.resolve({
      type: 'channel_message',
      message: must((history as Extract<Response, { type: 'channel_history' }>).messages[0]),
      revision: 12,
    });
  });
  return { state, requests };
}

function daemonProject() {
  const daemon = new FakeChannelDaemon();
  return { daemon, state: project(daemon.send) };
}

inTimeZone('UTC', () => {
  describe('ProjectChannelView', () => {
    it('renders channel-history.json like the Mac', async () => {
      const { state } = fixtureProject();
      const { container } = render(
        <ProjectChannelView project={state} locale="en-US" isAppActive={() => false} />,
      );
      await screen.findByText('jono');
      const timeline = screen.getByTestId('channel-timeline');
      // The top-level message only; its reply lives in the thread (3 replies).
      expect(within(timeline).getByText('Load older messages')).toBeInTheDocument();
      expect(within(timeline).getByText('Oct 9, 2026')).toBeInTheDocument();
      expect(within(timeline).getByText('5:00 PM')).toBeInTheDocument();
      expect(within(timeline).getByText('review').tagName).toBe('CODE');
      expect(within(timeline).getByText('edited')).toBeInTheDocument();
      expect(within(timeline).getByText('Review PR')).toBeInTheDocument();
      expect(within(timeline).getByText('👍 2')).toHaveClass('mine');
      expect(within(timeline).getByText('3 replies')).toBeInTheDocument();
      expect(within(timeline).queryByText(/checks passed/)).toBeNull();
      // The agent's reply is the one unread message (my own message never is).
      expect(screen.getByText('1 unread messages ↓')).toBeInTheDocument();
      expect(screen.getByText('A shared place to keep each other up to date')).toBeInTheDocument();
      expect(screen.getByRole('textbox', { name: 'Message channel' })).toHaveAttribute(
        'placeholder',
        'Message #demo',
      );
      expect(container.querySelector('[data-view="ProjectChannelView"]')).toMatchSnapshot();
      state.channel.shutdown();
    });

    it('shows the agent reply with its badge, branch and commit in the thread', async () => {
      const { state, requests } = fixtureProject();
      render(<ProjectChannelView project={state} locale="en-US" isAppActive={() => false} />);
      fireEvent.click(await screen.findByText('3 replies'));
      const thread = await screen.findByTestId('channel-thread');
      await within(thread).findByText('Channel builder');
      expect(requests.some((r) => r.type === 'channel_read' && r.parent_id === 'msg-parent')).toBe(
        true,
      );
      expect(within(thread).getByText('codex')).toHaveClass('channel-agent-badge');
      expect(within(thread).getByText('pu/channel')).toBeInTheDocument();
      expect(within(thread).getByText('abcdef01')).toBeInTheDocument();
      expect(
        within(thread).getByText(
          (_, element) =>
            element?.textContent === 'Yes — checks passed.\nRésumé included.' &&
            element.classList.contains('channel-message-text'),
        ),
      ).toBeInTheDocument();
      expect(within(thread).getByRole('textbox', { name: 'Reply' })).toHaveAttribute(
        'placeholder',
        'Reply to thread…',
      );
      // Reply rows cannot start their own thread.
      expect(within(thread).queryByRole('button', { name: 'Reply in thread' })).toBeNull();
      fireEvent.click(within(thread).getByRole('button', { name: 'Close thread' }));
      expect(screen.queryByTestId('channel-thread')).toBeNull();
      state.channel.shutdown();
    });

    it('compact hides the subtitle', async () => {
      const { state } = fixtureProject();
      render(<ProjectChannelView project={state} compact locale="en-US" />);
      await screen.findByText('jono');
      expect(screen.queryByText('A shared place to keep each other up to date')).toBeNull();
      state.channel.shutdown();
    });

    it('sends with Enter, keeps Shift+Enter for a newline and disables an empty send', async () => {
      const { state, daemon } = daemonProject();
      render(<ProjectChannelView project={state} locale="en-US" />);
      await screen.findByText('The conversation starts here');
      const editor = screen.getByRole('textbox', { name: 'Message channel' });
      const send = screen.getByRole('button', { name: 'Send message' });
      expect(send).toBeDisabled();
      fireEvent.change(editor, { target: { value: 'first line' } });
      fireEvent.keyDown(editor, { key: 'Enter', shiftKey: true });
      expect(daemon.requests.some((r) => r.type === 'channel_send')).toBe(false);
      fireEvent.change(editor, { target: { value: 'first line\nsecond' } });
      expect(send).toBeEnabled();
      await act(async () => {
        fireEvent.keyDown(editor, { key: 'Enter' });
        await Promise.resolve();
      });
      await screen.findByText(
        (_, element) =>
          element?.classList.contains('channel-message-text') === true &&
          element.textContent === 'first line\nsecond',
      );
      expect(daemon.requests.find((r) => r.type === 'channel_send')).toMatchObject({
        text: 'first line\nsecond',
      });
      expect(editor).toHaveValue('');
      state.channel.shutdown();
    });

    it('does not send while an IME composition is in progress', async () => {
      const { state, daemon } = daemonProject();
      render(<ProjectChannelView project={state} locale="en-US" />);
      const editor = await screen.findByRole('textbox', { name: 'Message channel' });
      fireEvent.change(editor, { target: { value: 'にほん' } });
      fireEvent.keyDown(editor, { key: 'Enter', isComposing: true });
      fireEvent.keyDown(editor, { key: 'Process' });
      expect(daemon.requests.some((r) => r.type === 'channel_send')).toBe(false);
      state.channel.shutdown();
    });

    it('shows a retryable failure without replaying the send', async () => {
      const { state, daemon } = daemonProject();
      render(<ProjectChannelView project={state} locale="en-US" />);
      const editor = await screen.findByRole('textbox', { name: 'Message channel' });
      daemon.failNext.channel_send = 'pipe closed';
      fireEvent.change(editor, { target: { value: 'hello' } });
      fireEvent.click(screen.getByRole('button', { name: 'Send message' }));
      expect(await screen.findByRole('alert')).toHaveTextContent(
        'Send was not confirmed. Check the channel before trying again. pipe closed',
      );
      expect(editor).toHaveValue('hello');
      fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
      expect(screen.queryByRole('alert')).toBeNull();
      expect(daemon.requests.filter((r) => r.type === 'channel_send')).toHaveLength(1);
      state.channel.shutdown();
    });

    it('completes @mentions of agents from the keyboard', async () => {
      const { state, daemon } = daemonProject();
      state.store.setState({
        rootAgents: [agent('ag-1', 'Builder', 'codex')],
        worktrees: [worktree('wt-1', 'C:\\work\\demo-wt', [agent('ag-2', 'Bobby', 'claude')])],
      });
      render(<ProjectChannelView project={state} locale="en-US" />);
      const editor = await screen.findByRole('textbox', { name: 'Message channel' });
      fireEvent.change(editor, { target: { value: 'hey @b', selectionStart: 6 } });
      const list = screen.getByRole('listbox', { name: 'Mentions' });
      expect(
        within(list)
          .getAllByRole('option')
          .map((o) => o.textContent),
      ).toEqual(['@Bobby', '@Builder']);
      fireEvent.keyDown(editor, { key: 'ArrowDown' });
      expect(within(list).getByRole('option', { selected: true })).toHaveTextContent('@Builder');
      fireEvent.keyDown(editor, { key: 'Enter' });
      expect(editor).toHaveValue('hey @Builder');
      expect(screen.queryByRole('listbox')).toBeNull();
      expect(daemon.requests.some((r) => r.type === 'channel_send')).toBe(false);
      // Nothing matches: no list.
      fireEvent.change(editor, { target: { value: 'hey @zz', selectionStart: 7 } });
      expect(screen.queryByRole('listbox')).toBeNull();
      // The @ menu appends a mention; the code button inline code.
      fireEvent.click(screen.getByRole('button', { name: 'Mention someone' }));
      fireEvent.click(screen.getByRole('menuitem', { name: '@Bobby' }));
      expect(editor).toHaveValue('hey @zz@Bobby ');
      fireEvent.click(screen.getByRole('button', { name: 'Insert inline code' }));
      expect(editor).toHaveValue('hey @zz@Bobby `code`');
      // The members list shows every agent.
      fireEvent.click(screen.getByRole('button', { name: 'Channel members' }));
      const members = screen.getByRole('menu', { name: 'Project members' });
      expect(within(members).getByText('Bobby')).toBeInTheDocument();
      expect(within(members).getByText('codex')).toBeInTheDocument();
      state.channel.shutdown();
    });

    it('edits my own message and reacts from the hover row', async () => {
      const { state, daemon } = daemonProject();
      daemon.post('typo', { authorId: 'human:me' });
      daemon.post('theirs');
      render(<ProjectChannelView project={state} locale="en-US" />);
      const mine = await screen.findByRole('article', { name: 'me: typo' });
      const theirs = screen.getByRole('article', { name: 'human:other: theirs' });

      fireEvent.click(within(theirs).getByRole('button', { name: 'More actions' }));
      expect(within(theirs).getByRole('menuitem', { name: 'Copy text' })).toBeInTheDocument();
      expect(within(theirs).getByRole('menuitem', { name: 'Mark unread' })).toBeInTheDocument();
      expect(within(theirs).queryByRole('menuitem', { name: 'Edit message' })).toBeNull();

      fireEvent.click(within(mine).getByRole('button', { name: 'More actions' }));
      fireEvent.click(within(mine).getByRole('menuitem', { name: 'Edit message' }));
      const sheet = screen.getByRole('dialog', { name: 'Edit message' });
      const text = within(sheet).getByRole('textbox', { name: 'Message text' });
      expect(text).toHaveValue('typo');
      fireEvent.change(text, { target: { value: '   ' } });
      expect(within(sheet).getByRole('button', { name: 'Save' })).toBeDisabled();
      fireEvent.change(text, { target: { value: 'fixed' } });
      fireEvent.click(within(sheet).getByRole('button', { name: 'Save' }));
      await screen.findByRole('article', { name: 'me: fixed' });
      expect(screen.queryByRole('dialog')).toBeNull();

      fireEvent.click(within(theirs).getByRole('button', { name: 'React 👍' }));
      expect(await within(theirs).findByText('👍 1')).toHaveClass('mine');
      expect(daemon.requests.find((r) => r.type === 'channel_react')).toMatchObject({
        active: true,
      });
      fireEvent.click(within(theirs).getByText('👍 1'));
      await waitFor(() => {
        expect(within(theirs).queryByText('👍 1')).toBeNull();
      });
      state.channel.shutdown();
    });

    it('copies text and marks a message unread from the hover menu', async () => {
      const { state, daemon } = daemonProject();
      daemon.post('one');
      daemon.post('two');
      const copied: string[] = [];
      render(
        <ProjectChannelView
          project={state}
          locale="en-US"
          copyText={(text) => copied.push(text)}
        />,
      );
      const row = await screen.findByRole('article', { name: 'human:other: two' });
      fireEvent.click(within(row).getByRole('button', { name: 'More actions' }));
      fireEvent.click(within(row).getByRole('menuitem', { name: 'Copy text' }));
      expect(copied).toEqual(['two']);
      act(() => {
        state.channel.markRead();
      });
      expect(screen.queryByText(/unread messages/)).toBeNull();
      fireEvent.click(within(row).getByRole('button', { name: 'More actions' }));
      fireEvent.click(within(row).getByRole('menuitem', { name: 'Mark unread' }));
      expect(await screen.findByText('1 unread messages ↓')).toBeInTheDocument();
      expect(screen.getByText('New messages')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Mark as read' }));
      expect(screen.queryByText(/unread messages/)).toBeNull();
      expect(screen.queryByText('New messages')).toBeNull();
      state.channel.shutdown();
    });

    it('searches and clears the search', async () => {
      const { state, daemon } = daemonProject();
      daemon.post('alpha');
      daemon.post('beta');
      render(<ProjectChannelView project={state} locale="en-US" />);
      await screen.findByText('beta');
      fireEvent.click(screen.getByRole('button', { name: 'Search messages' }));
      const field = screen.getByRole('searchbox', { name: 'Search messages' });
      fireEvent.change(field, { target: { value: 'alp' } });
      fireEvent.keyDown(field, { key: 'Enter' });
      await waitFor(() => {
        expect(screen.queryByText('beta')).toBeNull();
      });
      expect(screen.getByText('alpha')).toBeInTheDocument();
      fireEvent.change(field, { target: { value: 'zzz' } });
      fireEvent.keyDown(field, { key: 'Enter' });
      expect(await screen.findByText('No messages found')).toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
      expect(await screen.findByText('beta')).toBeInTheDocument();
      fireEvent.keyDown(screen.getByRole('searchbox', { name: 'Search messages' }), {
        key: 'Escape',
      });
      expect(screen.queryByRole('searchbox')).toBeNull();
      state.channel.shutdown();
    });

    it('opens a reference in its sender worktree', async () => {
      const { state, daemon } = daemonProject();
      state.store.setState({
        worktrees: [worktree('wt-1', 'C:\\work\\demo-wt')],
      });
      const message = daemon.post('see commit');
      daemon.messages[0] = {
        ...message,
        author: { ...message.author, worktree_id: 'wt-1' },
        references: [{ kind: 'commit', value: 'abcdef0123456789', label: null }],
      };
      render(<ProjectChannelView project={state} locale="en-US" />);
      fireEvent.click(await screen.findByRole('button', { name: 'abcdef01' }));
      const sheet = screen.getByRole('dialog', { name: 'abcdef0123456789' });
      await within(sheet).findByText('No file changes');
      expect(gitCalls).toContain('commit C:\\work\\demo-wt abcdef0123456789');
      fireEvent.click(within(sheet).getByRole('button', { name: 'Done' }));
      expect(screen.queryByRole('dialog')).toBeNull();
      state.channel.shutdown();
    });

    it('feeds the sidebar unread count through the project snapshot', async () => {
      const { state, daemon } = daemonProject();
      daemon.post('hello');
      expect(state.snapshot().channelUnread).toBe(0);
      await state.channel.refresh();
      expect(state.snapshot().channelUnread).toBe(1);
      state.channel.markRead();
      expect(state.snapshot().channelUnread).toBe(0);
    });
  });
});

describe('ChannelComposer', () => {
  it('follows the content height between 60 and 180 px', () => {
    const { rerender } = render(
      <ChannelComposer
        text=""
        onTextChange={() => undefined}
        placeholder="Message channel"
        prompt=""
        mentions={[]}
        onSubmit={() => undefined}
      />,
    );
    const editor = screen.getByRole('textbox', { name: 'Message channel' });
    expect(editor.style.height).toBe('60px');
    Object.defineProperty(editor, 'scrollHeight', { configurable: true, value: 2000 });
    rerender(
      <ChannelComposer
        text={'line\n'.repeat(40)}
        onTextChange={() => undefined}
        placeholder="Message channel"
        prompt=""
        mentions={[]}
        onSubmit={() => undefined}
      />,
    );
    expect(editor.style.height).toBe('180px');
  });
});
