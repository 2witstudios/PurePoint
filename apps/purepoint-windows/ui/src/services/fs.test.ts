// Command wiring of the layout file IO (WIN-6.0); the commands are tested in Rust
// (`src-tauri/src/files/text.rs`).
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

const { daemonLayoutHostIO, localLayoutHostIO } = await import('./fs');
const { LayoutWriteConflictError } = await import('../model/workspacePersistence');
type Request = import('../protocol').Request;
type Response = import('../protocol').Response;

describe('localLayoutHostIO', () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it('stats, reads and writes through the fs commands', async () => {
    invoke.mockResolvedValueOnce(42);
    await expect(localLayoutHostIO.stat('C:\\p\\.pu\\workspaces.json')).resolves.toEqual({
      modifiedMs: 42,
    });
    expect(invoke).toHaveBeenLastCalledWith('fs_stat_text', {
      path: 'C:\\p\\.pu\\workspaces.json',
    });
    invoke.mockResolvedValueOnce(null);
    await expect(localLayoutHostIO.stat('x')).resolves.toBeNull();

    invoke.mockResolvedValueOnce({ text: '{}', modifiedMs: 1, truncated: false });
    await expect(localLayoutHostIO.read('x', 10)).resolves.toEqual({
      text: '{}',
      modifiedMs: 1,
      truncated: false,
    });
    expect(invoke).toHaveBeenLastCalledWith('fs_read_text', { path: 'x', maxBytes: 10 });

    invoke.mockResolvedValueOnce(7);
    await expect(localLayoutHostIO.write('x', 't', null)).resolves.toBe(7);
    expect(invoke).toHaveBeenLastCalledWith('fs_write_text_if_unmodified', {
      path: 'x',
      text: 't',
      expectedModifiedMs: null,
    });
  });

  it('turns a conflict rejection into a layout write conflict', async () => {
    invoke.mockRejectedValueOnce({ kind: 'conflict', message: 'changed' });
    await expect(localLayoutHostIO.write('x', 't', 1)).rejects.toBeInstanceOf(
      LayoutWriteConflictError,
    );
    invoke.mockRejectedValueOnce({ kind: 'io', message: 'denied' });
    await expect(localLayoutHostIO.write('x', 't', 1)).rejects.toEqual({
      kind: 'io',
      message: 'denied',
    });
  });
});

describe('daemonLayoutHostIO', () => {
  function host(answer: (request: Request) => Response) {
    const requests: Request[] = [];
    const io = daemonLayoutHostIO({
      send: (request) => {
        requests.push(request);
        return Promise.resolve(answer(request));
      },
    });
    return { io, requests };
  }

  it('reads, stats and writes through fs requests with base64 bodies', async () => {
    const { io, requests } = host((request) => {
      switch (request.type) {
        case 'fs_stat':
          return {
            type: 'fs_stat_result',
            path: request.path,
            exists: false,
            is_dir: false,
            size: 0,
            modified_ms: 0,
          };
        case 'fs_read':
          return {
            type: 'fs_read_result',
            path: request.path,
            data_base64: btoa('{"version":4}'),
            size: 13,
            modified_ms: 9,
            truncated: false,
          };
        default:
          return { type: 'fs_write_result', path: '/p', size: 2, modified_ms: 10 };
      }
    });
    await expect(io.stat('/p')).resolves.toBeNull();
    await expect(io.read('/p', 100)).resolves.toEqual({
      text: '{"version":4}',
      modifiedMs: 9,
      truncated: false,
    });
    await expect(io.write('/p', 'é', 9)).resolves.toBe(10);
    expect(requests[2]).toEqual({
      type: 'fs_write',
      path: '/p',
      data_base64: 'w6k=',
      expected_modified_ms: 9,
      create_dirs: true,
    });
  });

  it('maps not_found to a missing file and conflict to a write conflict', async () => {
    const { io } = host((request) =>
      request.type === 'fs_write'
        ? { type: 'error', code: 'conflict', message: 'changed' }
        : { type: 'error', code: 'not_found', message: 'gone' },
    );
    await expect(io.read('/p', 1)).resolves.toBeNull();
    await expect(io.stat('/p')).resolves.toBeNull();
    await expect(io.write('/p', 'x', 1)).rejects.toBeInstanceOf(LayoutWriteConflictError);
  });

  it('rejects other replies', async () => {
    const { io } = host(() => ({ type: 'error', code: 'io_error', message: 'denied' }));
    await expect(io.read('/p', 1)).rejects.toThrow('denied');
  });
});
