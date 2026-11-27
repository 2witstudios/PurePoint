// Command and event wiring of the watcher wrappers; the watching itself is tested in Rust.
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

type Listener = (event: { payload: unknown }) => void;
const listeners = new Map<string, Listener>();
vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, handler: Listener) => {
    listeners.set(name, handler);
    return Promise.resolve(() => listeners.delete(name));
  },
}));

const watchers = await import('./watchers');

describe('watcher service', () => {
  beforeEach(() => {
    invoke.mockReset();
    listeners.clear();
  });

  it('starts and stops watchers with camelCase arguments', async () => {
    invoke.mockResolvedValue(1);
    await watchers.watchManifest('C:\\p');
    expect(invoke).toHaveBeenLastCalledWith('watch_manifest', { projectRoot: 'C:\\p' });
    await watchers.watchWorktree('C:\\p\\wt');
    expect(invoke).toHaveBeenLastCalledWith('watch_worktree', { worktreePath: 'C:\\p\\wt' });
    await watchers.watchFileTree('C:\\p');
    expect(invoke).toHaveBeenLastCalledWith('watch_file_tree', { root: 'C:\\p' });
    await watchers.watchMachines();
    expect(invoke).toHaveBeenLastCalledWith('watch_machines', undefined);
    await watchers.unwatch(3);
    expect(invoke).toHaveBeenLastCalledWith('unwatch', { id: 3 });
  });

  it('delivers event payloads to listeners', async () => {
    const manifest = vi.fn();
    const tree = vi.fn();
    const machines = vi.fn();
    const worktrees = vi.fn();
    const unlisten = await watchers.onManifestChanged(manifest);
    await watchers.onFileTreeChanged(tree);
    await watchers.onMachinesChanged(machines);
    await watchers.onWorktreesChanged(worktrees);
    listeners.get('manifest-changed')?.({ payload: { projectRoot: 'C:\\p' } });
    listeners.get('file-tree-changed')?.({ payload: { root: 'C:\\p', paths: ['C:\\p\\a'] } });
    listeners.get('machines-changed')?.({ payload: {} });
    listeners.get('worktrees-changed')?.({ payload: { worktreePath: 'C:\\p\\wt' } });
    expect(manifest).toHaveBeenCalledWith({ projectRoot: 'C:\\p' });
    expect(tree).toHaveBeenCalledWith({ root: 'C:\\p', paths: ['C:\\p\\a'] });
    expect(machines).toHaveBeenCalledOnce();
    expect(worktrees).toHaveBeenCalledWith({ worktreePath: 'C:\\p\\wt' });
    unlisten();
    expect(listeners.has('manifest-changed')).toBe(false);
  });

  it('releases before the request and discards the release on success', async () => {
    invoke.mockImplementation((command) =>
      Promise.resolve(command === 'watchers_release' ? { token: 7, released: 2 } : true),
    );
    const send = vi.fn(() => {
      expect(invoke).toHaveBeenLastCalledWith('watchers_release', {
        path: 'C:\\p\\.pu\\worktrees\\wt-1',
      });
      return Promise.resolve('ok');
    });
    const request = {
      type: 'delete_worktree',
      project_root: 'C:\\p',
      worktree_id: 'wt-1',
    } as const;
    await expect(watchers.sendReleasingWatchers(request, send)).resolves.toBe('ok');
    expect(send).toHaveBeenCalledWith(request);
    expect(invoke).toHaveBeenLastCalledWith('watchers_discard', { token: 7 });
  });

  it('restores the watchers when the request fails', async () => {
    invoke.mockImplementation((command) =>
      Promise.resolve(command === 'watchers_release' ? { token: 9, released: 1 } : 1),
    );
    const failure = new Error('locked');
    await expect(
      watchers.withWatchersReleased('C:\\p\\wt', () => Promise.reject(failure)),
    ).rejects.toBe(failure);
    expect(invoke).toHaveBeenLastCalledWith('watchers_restore', { token: 9 });
  });

  it('releases only for requests that remove a worktree', () => {
    expect(
      watchers.releasePathFor({
        type: 'kill',
        project_root: 'C:\\p\\',
        target: { worktree: 'wt-2' },
      }),
    ).toBe('C:\\p\\.pu\\worktrees\\wt-2');
    expect(
      watchers.releasePathFor({ type: 'kill', project_root: '/home/u/p', target: 'all_worktrees' }),
    ).toBe('/home/u/p/.pu/worktrees');
    expect(
      watchers.releasePathFor({ type: 'kill', project_root: 'C:\\p', target: { agent: 'ag-1' } }),
    ).toBeNull();
    expect(watchers.releasePathFor({ type: 'status', project_root: 'C:\\p' })).toBeNull();
  });

  it('sends other requests without releasing', async () => {
    const send = vi.fn(() => Promise.resolve(1));
    await watchers.sendReleasingWatchers({ type: 'status', project_root: 'C:\\p' }, send);
    expect(invoke).not.toHaveBeenCalled();
  });
});
