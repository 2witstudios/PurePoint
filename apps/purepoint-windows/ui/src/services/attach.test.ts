// Command wiring of the attach service; the session itself is tested in Rust.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn<(command: string, args?: Record<string, unknown>) => Promise<unknown>>();
const channels: { onmessage: (message: unknown) => void }[] = [];
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: Record<string, unknown>) => invoke(command, args),
  Channel: class {
    onmessage: (message: unknown) => void;
    constructor(onmessage: (message: unknown) => void) {
      this.onmessage = onmessage;
      channels.push(this);
    }
  },
}));

const { openAttach } = await import('./attach');

async function open(options: Parameters<typeof openAttach>[1] = { cols: 80, rows: 24 }) {
  const output: Uint8Array[] = [];
  const events: unknown[] = [];
  invoke.mockResolvedValueOnce(7);
  const session = await openAttach('ag-1', options, {
    onOutput: (bytes) => output.push(bytes),
    onEvent: (event) => events.push(event),
  });
  const channel = channels.at(-1);
  if (!channel) throw new Error('no channel');
  return { session, output, events, channel };
}

describe('attach service', () => {
  beforeEach(() => {
    invoke.mockReset();
    channels.length = 0;
  });

  it('opens a session over a channel', async () => {
    const { session, channel } = await open({ cols: 120, rows: 40, focused: true });
    expect(session.id).toBe(7);
    expect(invoke).toHaveBeenCalledWith('attach_open', {
      agentId: 'ag-1',
      cols: 120,
      rows: 40,
      onEvent: channel,
      focused: true,
      resetBeforeReplay: false,
      machineId: null,
    });
  });

  it('routes raw output and json events in order', async () => {
    const { session, output, events, channel } = await open();
    channel.onmessage({ type: 'attach_ready', bufferedBytes: 2 });
    channel.onmessage(new Uint8Array([104, 105]).buffer);
    channel.onmessage({ type: 'live' });
    channel.onmessage({ type: 'exited' });
    expect(output).toEqual([new Uint8Array([104, 105])]);
    expect(events).toEqual([
      { type: 'attach_ready', bufferedBytes: 2 },
      { type: 'live' },
      { type: 'exited' },
    ]);
    expect(session.ended).toEqual({ type: 'exited' });
  });

  it('sends input, size and focus on the session', async () => {
    const { session } = await open();
    invoke.mockResolvedValue(true);
    await session.input('ls', { submit: true });
    expect(invoke).toHaveBeenLastCalledWith('attach_input', {
      sessionId: 7,
      data: 'ls',
      submit: true,
      deliver: false,
    });
    await expect(session.deliver(new Uint8Array([27, 91, 65]))).resolves.toBe(true);
    expect(invoke).toHaveBeenLastCalledWith('attach_input', {
      sessionId: 7,
      data: [27, 91, 65],
      submit: false,
      deliver: true,
    });
    await session.resize(100, 30);
    expect(invoke).toHaveBeenLastCalledWith('attach_resize', { sessionId: 7, cols: 100, rows: 30 });
    await session.claimSize();
    expect(invoke).toHaveBeenLastCalledWith('attach_claim_size', { sessionId: 7 });
    await session.blur();
    expect(invoke).toHaveBeenLastCalledWith('attach_blur', { sessionId: 7 });
    await session.close();
    expect(invoke).toHaveBeenLastCalledWith('attach_close', { sessionId: 7 });
  });
});
