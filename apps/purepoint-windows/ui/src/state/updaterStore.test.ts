import { describe, expect, it, vi } from 'vitest';
import type { UpdaterState } from '../services/updater.ts';
import {
  checkForUpdates,
  createUpdaterStore,
  downloadUpdate,
  installUpdate,
  type UpdaterCommands,
} from './updaterStore.ts';

function commands(replies: Partial<Record<keyof UpdaterCommands, UpdaterState | Error>>) {
  const reply = (key: keyof UpdaterCommands) => () => {
    const value = replies[key];
    if (value === undefined) return Promise.reject(new Error(`unexpected ${key}`));
    return value instanceof Error ? Promise.reject(value) : Promise.resolve(value);
  };
  return {
    check: vi.fn(reply('check')),
    download: vi.fn(reply('download')),
    install: vi.fn(reply('install')),
  } satisfies UpdaterCommands;
}

const available: UpdaterState = {
  state: 'available',
  version: '0.2.0',
  currentVersion: '0.1.0',
  notes: 'Faster agents.',
  date: null,
};

describe('updater store', () => {
  it('opens the dialog, shows checking, then the reply', async () => {
    const store = createUpdaterStore();
    const backend = commands({ check: available });
    const done = checkForUpdates(store, backend);
    expect(store.getState().dialogOpen).toBe(true);
    expect(store.getState().status).toEqual({ state: 'checking' });
    await done;
    expect(store.getState().status).toEqual(available);
  });

  it('does not start a second check while one runs', async () => {
    const store = createUpdaterStore();
    store.getState().setStatus({ state: 'downloading', version: '0.2.0', downloaded: 0, total: 9 });
    const backend = commands({});
    await checkForUpdates(store, backend);
    expect(backend.check).not.toHaveBeenCalled();
    expect(store.getState().dialogOpen).toBe(true);
  });

  it('keeps "not configured" without flashing checking', async () => {
    const store = createUpdaterStore();
    store.getState().setStatus({ state: 'notConfigured' });
    const backend = commands({ check: { state: 'notConfigured' } });
    const done = checkForUpdates(store, backend);
    expect(store.getState().status).toEqual({ state: 'notConfigured' });
    await done;
    expect(backend.check).toHaveBeenCalledOnce();
    expect(store.getState().status).toEqual({ state: 'notConfigured' });
  });

  it('downloads only an available update and installs only a ready one', async () => {
    const store = createUpdaterStore();
    const backend = commands({
      download: { state: 'readyToRestart', version: '0.2.0' },
      install: { state: 'readyToRestart', version: '0.2.0' },
    });
    await installUpdate(store, backend);
    await downloadUpdate(store, backend);
    expect(backend.download).not.toHaveBeenCalled();
    expect(backend.install).not.toHaveBeenCalled();

    store.getState().setStatus(available);
    const downloading = downloadUpdate(store, backend);
    expect(store.getState().status.state).toBe('downloading');
    await downloading;
    expect(store.getState().status).toEqual({ state: 'readyToRestart', version: '0.2.0' });

    const installing = installUpdate(store, backend);
    expect(store.getState().status).toEqual({ state: 'installing', version: '0.2.0' });
    await installing;
    expect(backend.install).toHaveBeenCalledOnce();
  });

  it('turns a failed command into the error state', async () => {
    const store = createUpdaterStore();
    await checkForUpdates(store, commands({ check: new Error('offline') }));
    expect(store.getState().status).toEqual({ state: 'error', message: 'offline' });
  });
});
