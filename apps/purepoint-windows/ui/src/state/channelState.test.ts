// WIN-8.10: ChannelState against an in-memory channel store with the engine's cursor rules.
import { describe, expect, it } from 'vitest';
import { FakeChannelDaemon } from './channelState.fixtures';
import {
  BACKGROUND_POLL_MS,
  ChannelState,
  memoryChannelStorage,
  VISIBLE_POLL_MS,
  type ChannelStorage,
} from './channelState';

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error('missing test value');
  return value;
}

const ROOT = 'C:\\repo';

/** A sleep that only ends when polling is cancelled, recording each requested interval. */
function manualSleep() {
  const intervals: number[] = [];
  const wakers: (() => void)[] = [];
  return {
    intervals,
    wake: () => {
      wakers.splice(0).forEach((wake) => {
        wake();
      });
    },
    sleep: (ms: number, signal: AbortSignal) =>
      new Promise<void>((resolve) => {
        intervals.push(ms);
        wakers.push(resolve);
        signal.addEventListener('abort', () => {
          resolve();
        });
      }),
  };
}

function make(
  daemon = new FakeChannelDaemon(),
  storage: ChannelStorage = memoryChannelStorage(),
  sleep = manualSleep(),
) {
  const channel = new ChannelState({
    projectRoot: ROOT,
    storage,
    sendRequest: daemon.send,
    sleep: sleep.sleep,
  });
  return { channel, daemon, storage, sleep };
}

async function settle(): Promise<void> {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}

describe('ChannelState reads', () => {
  it('given a project channel, should load the latest window and its cursors', async () => {
    const { channel, daemon } = make();
    for (let i = 1; i <= 120; i++) daemon.post(`m${String(i)}`);
    await channel.refresh();
    expect(channel.messages).toHaveLength(100);
    expect(channel.messages[0]?.text).toBe('m21');
    expect(channel.hasMore).toBe(true);
    expect(channel.selfAuthorId).toBe('human:me');
    expect(channel.latestSequence).toBe(120);
    const read = daemon.reads()[0];
    expect(read).toMatchObject({ project_root: ROOT, query: null, known_revision: null });
  });

  it('given an unchanged revision, should skip the reload and keep the messages', async () => {
    const { channel, daemon } = make();
    daemon.post('hello');
    await channel.refresh();
    await channel.refresh();
    const reads = daemon.reads();
    expect(reads.at(-1)?.known_revision).toBe(daemon.revision);
    expect(channel.messages.map((m) => m.text)).toEqual(['hello']);
  });

  it('given older pages, should load them, merge without duplicates and keep them on refresh', async () => {
    const { channel, daemon } = make();
    for (let i = 1; i <= 250; i++) daemon.post(`m${String(i)}`);
    await channel.refresh();
    await channel.loadOlder();
    expect(channel.messages).toHaveLength(200);
    expect(channel.messages[0]?.sequence).toBe(51);
    expect(channel.hasMore).toBe(true);
    const sequences = channel.messages.map((m) => m.sequence);
    expect(sequences).toEqual([...sequences].sort((a, b) => a - b));

    // An edit far back changes the revision: every loaded page refreshes.
    daemon.messages[60] = { ...must(daemon.messages[60]), text: 'edited far back' };
    daemon.revision += 1;
    await channel.refresh();
    expect(channel.messages).toHaveLength(200);
    expect(channel.messages.find((m) => m.sequence === 61)?.text).toBe('edited far back');
    await channel.loadOlder();
    expect(channel.messages).toHaveLength(250);
    expect(channel.hasMore).toBe(false);
  });

  it('given a read error, should report it rather than an empty success', async () => {
    const { channel, daemon } = make();
    daemon.failNext.channel_read = 'unsupported channel store version';
    await channel.refresh();
    expect(channel.error).toBe('unsupported channel store version');
    expect(channel.store.getState().isLoading).toBe(false);
    await channel.refresh();
    expect(channel.error).toBeNull();
  });

  it('given a rejected transport, should surface its message', async () => {
    const channel = new ChannelState({
      projectRoot: ROOT,
      storage: memoryChannelStorage(),
      sendRequest: () => Promise.reject(new Error('daemon is not running')),
    });
    await channel.refresh();
    expect(channel.error).toBe('daemon is not running');
  });

  it('given a search, should send the query on latest and older reads only', async () => {
    const { channel, daemon } = make();
    daemon.post('alpha');
    daemon.post('beta');
    await channel.refresh();
    channel.setQuery('alp');
    await channel.search();
    expect(channel.messages.map((m) => m.text)).toEqual(['alpha']);
    const reads = daemon.reads();
    expect(reads.at(-1)).toMatchObject({ query: 'alp', known_revision: null });
    expect(reads.filter((r) => r.after != null).every((r) => r.query === null)).toBe(true);
  });

  it('given a thread, should load its replies, older replies and refresh it with the channel', async () => {
    const { channel, daemon } = make();
    const parent = daemon.post('parent');
    for (let i = 1; i <= 130; i++) daemon.post(`r${String(i)}`, { parentId: parent.id });
    await channel.refresh();
    expect(channel.store.getState().replyCounts[parent.id]).toBe(130);
    await channel.openThread(parent.id);
    expect(channel.threadMessages).toHaveLength(100);
    expect(channel.threadHasMore).toBe(true);
    await channel.loadOlderReplies();
    expect(channel.threadMessages).toHaveLength(130);
    expect(channel.threadMessages.every((m) => m.parent_id === parent.id)).toBe(true);
    daemon.post('r131', { parentId: parent.id });
    await channel.refresh();
    expect(channel.threadMessages).toHaveLength(131);
    channel.closeThread();
    expect(channel.threadId).toBeNull();
  });
});

describe('ChannelState unread tracking', () => {
  it("given others' messages, should count them unread and never my own", async () => {
    const { channel, daemon } = make();
    daemon.post('one');
    daemon.post('mine', { authorId: 'human:me' });
    daemon.post('two', { authorId: 'ag-1', agentType: 'codex' });
    await channel.refresh();
    expect(channel.unreadCount).toBe(2);
    expect(channel.firstUnread()?.text).toBe('one');
  });

  it('given visible messages, should advance the saved read cursor contiguously', async () => {
    const storage = memoryChannelStorage();
    const { channel, daemon } = make(new FakeChannelDaemon(), storage);
    const [a, b, c] = [daemon.post('a'), daemon.post('b'), daemon.post('c')];
    await channel.refresh();
    channel.markVisible(c);
    expect(channel.readSequence).toBe(0);
    expect(channel.unreadCount).toBe(2);
    channel.markVisible(a);
    expect(channel.readSequence).toBe(1);
    channel.markVisible(b);
    expect(channel.readSequence).toBe(3);
    expect(channel.unreadCount).toBe(0);
    expect(storage.getItem(`channel.${ROOT}.read`)).toBe('3');

    // Another instance (an app restart) resumes from the saved cursor.
    const restarted = make(daemon, storage).channel;
    await restarted.refresh();
    expect(restarted.unreadCount).toBe(0);
    daemon.post('d');
    await restarted.refresh();
    expect(restarted.unreadCount).toBe(1);
  });

  it('given a seen message out of order, should remember it across restarts', async () => {
    const storage = memoryChannelStorage();
    const { channel, daemon } = make(new FakeChannelDaemon(), storage);
    daemon.post('a');
    const b = daemon.post('b');
    await channel.refresh();
    channel.markVisible(b);
    expect(JSON.parse(storage.getItem(`channel.${ROOT}.seen`) ?? '[]')).toEqual(['2']);
    const restarted = make(daemon, storage).channel;
    await restarted.refresh();
    expect(restarted.unreadCount).toBe(1);
  });

  it('given unread messages beyond the loaded window, should count all of them', async () => {
    const { channel, daemon } = make();
    for (let i = 1; i <= 250; i++) daemon.post(`m${String(i)}`);
    await channel.refresh();
    expect(channel.messages).toHaveLength(100);
    expect(channel.unreadCount).toBe(250);
  });

  it('given an older timeline opened, should not mark anything read without visibility', async () => {
    const { channel, daemon } = make();
    for (let i = 1; i <= 150; i++) daemon.post(`m${String(i)}`);
    await channel.refresh();
    await channel.loadOlder();
    expect(channel.readSequence).toBe(0);
    expect(channel.unreadCount).toBe(150);
  });

  it('given a search, should not mark messages read', async () => {
    const { channel, daemon } = make();
    const a = daemon.post('alpha');
    await channel.refresh();
    channel.setQuery('alpha');
    await channel.search();
    channel.markVisible(a);
    channel.markRead();
    expect(channel.unreadCount).toBe(1);
  });

  it('given mark read and mark unread, should move the cursor and recount', async () => {
    const { channel, daemon } = make();
    daemon.post('a');
    const b = daemon.post('b');
    daemon.post('c');
    await channel.refresh();
    channel.markRead();
    expect(channel.unreadCount).toBe(0);
    expect(channel.readSequence).toBe(3);
    channel.markUnread(b);
    expect(channel.readSequence).toBe(1);
    await settle();
    await channel.refresh();
    expect(channel.unreadCount).toBe(2);
  });

  it('given the first unread in an older page or a thread, should reveal it', async () => {
    const { channel, daemon } = make();
    const parent = daemon.post('parent', { authorId: 'human:me' });
    const reply = daemon.post('reply', { parentId: parent.id });
    for (let i = 1; i <= 150; i++) daemon.post(`m${String(i)}`, { authorId: 'human:me' });
    await channel.refresh();
    expect(channel.unreadCount).toBe(1);
    expect(await channel.revealFirstUnread()).toBe(reply.id);
    expect(channel.threadId).toBe(parent.id);
    expect(channel.unreadReplyTarget).toBe(reply.id);
    expect(channel.messages.some((m) => m.id === parent.id)).toBe(true);
  });
});

describe('ChannelState writes', () => {
  it('given a draft, should send it, clear it and refresh', async () => {
    const storage = memoryChannelStorage();
    const { channel, daemon } = make(new FakeChannelDaemon(), storage);
    channel.setDraft('hello @team');
    expect(storage.getItem(`channel.${ROOT}.draft`)).toBe('hello @team');
    await channel.send();
    expect(daemon.requests.find((r) => r.type === 'channel_send')).toMatchObject({
      text: 'hello @team',
      parent_id: null,
    });
    expect(channel.draft).toBe('');
    expect(storage.getItem(`channel.${ROOT}.draft`)).toBe('');
    expect(channel.messages.map((m) => m.text)).toEqual(['hello @team']);
    expect(channel.unreadCount).toBe(0);
  });

  it('given a saved draft, should restore it', () => {
    const storage = memoryChannelStorage();
    storage.setItem(`channel.${ROOT}.draft`, 'half written');
    expect(make(new FakeChannelDaemon(), storage).channel.draft).toBe('half written');
  });

  it('given a blank draft, should not send', async () => {
    const { channel, daemon } = make();
    channel.setDraft('  \n ');
    await channel.send();
    expect(daemon.requests.some((r) => r.type === 'channel_send')).toBe(false);
  });

  it('given an unconfirmed send, should keep the draft and not retry', async () => {
    const { channel, daemon } = make();
    channel.setDraft('important');
    daemon.failNext.channel_send = 'connection reset';
    await channel.send();
    expect(channel.draft).toBe('important');
    expect(channel.error).toBe(
      'Send was not confirmed. Check the channel before trying again. connection reset',
    );
    expect(daemon.requests.filter((r) => r.type === 'channel_send')).toHaveLength(1);
    channel.dismissMutationError();
    expect(channel.error).toBeNull();
  });

  it('given a thread reply, should send it to the parent and keep drafts per thread', async () => {
    const storage = memoryChannelStorage();
    const { channel, daemon } = make(new FakeChannelDaemon(), storage);
    const first = daemon.post('first');
    const second = daemon.post('second');
    await channel.refresh();
    await channel.openThread(first.id);
    channel.setReplyDraft('draft for first');
    await channel.openThread(second.id);
    expect(channel.replyDraft).toBe('');
    await channel.openThread(first.id);
    expect(channel.replyDraft).toBe('draft for first');
    await channel.send(true);
    expect(daemon.requests.find((r) => r.type === 'channel_send')).toMatchObject({
      text: 'draft for first',
      parent_id: first.id,
    });
    expect(channel.replyDraft).toBe('');
    expect(channel.threadMessages.map((m) => m.text)).toEqual(['draft for first']);
    expect(channel.store.getState().replyCounts[first.id]).toBe(1);
  });

  it('given my message, should edit it; given another sender, should report the rejection', async () => {
    const { channel, daemon } = make();
    const mine = daemon.post('typo', { authorId: 'human:me' });
    const theirs = daemon.post('theirs');
    await channel.refresh();
    await channel.edit(mine, 'fixed');
    expect(channel.messages.find((m) => m.id === mine.id)).toMatchObject({ text: 'fixed' });
    expect(channel.messages.find((m) => m.id === mine.id)?.edited_at).not.toBeNull();
    await channel.edit(theirs, 'hijack');
    expect(channel.error).toBe('only the sender can edit a message');
    expect(channel.messages.find((m) => m.id === theirs.id)?.text).toBe('theirs');
  });

  it('given a reaction, should toggle my thumbs-up', async () => {
    const { channel, daemon } = make();
    const message = daemon.post('nice');
    await channel.refresh();
    await channel.react(must(channel.messages[0]));
    expect(daemon.requests.filter((r) => r.type === 'channel_react').at(-1)).toMatchObject({
      message_id: message.id,
      active: true,
    });
    expect(channel.messages[0]?.reactions).toEqual([{ emoji: '👍', author_ids: ['human:me'] }]);
    await channel.react(must(channel.messages[0]));
    expect(daemon.requests.filter((r) => r.type === 'channel_react').at(-1)).toMatchObject({
      active: false,
    });
    expect(channel.messages[0]?.reactions).toEqual([]);
  });
});

describe('ChannelState polling', () => {
  it('given a visible view, should poll every 3 s; in the background every 15 s', async () => {
    const sleep = manualSleep();
    const { channel, daemon } = make(new FakeChannelDaemon(), memoryChannelStorage(), sleep);
    channel.startBackground();
    await settle();
    expect(daemon.reads()).toHaveLength(1);
    expect(sleep.intervals).toEqual([BACKGROUND_POLL_MS]);
    channel.start();
    sleep.wake();
    await settle();
    expect(daemon.reads()).toHaveLength(2);
    expect(sleep.intervals.at(-1)).toBe(VISIBLE_POLL_MS);
    channel.stop();
    sleep.wake();
    await settle();
    expect(sleep.intervals.at(-1)).toBe(BACKGROUND_POLL_MS);
    channel.shutdown();
    sleep.wake();
    await settle();
    expect(daemon.reads()).toHaveLength(3);
  });

  it('given the last view closes without background polling, should stop', async () => {
    const sleep = manualSleep();
    const { channel, daemon } = make(new FakeChannelDaemon(), memoryChannelStorage(), sleep);
    channel.start();
    await settle();
    channel.stop();
    sleep.wake();
    await settle();
    expect(daemon.reads()).toHaveLength(1);
  });

  it('given a shutdown mid-read, should drop the late result', async () => {
    const daemon = new FakeChannelDaemon();
    daemon.post('late');
    let release: () => void = () => undefined;
    const channel = new ChannelState({
      projectRoot: ROOT,
      storage: memoryChannelStorage(),
      sendRequest: (request) =>
        new Promise((resolve) => {
          release = () => {
            void daemon.send(request).then(resolve);
          };
        }),
    });
    const pending = channel.refresh();
    await settle();
    channel.shutdown();
    release();
    await pending;
    expect(channel.messages).toEqual([]);
  });
});
