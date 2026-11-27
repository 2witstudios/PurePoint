import { describe, expect, it, vi } from 'vitest';
import type { UpdaterState } from '../services/updater.ts';
import {
  AUTOMATIC_CHECK_DELAY_MS,
  AUTOMATIC_CHECK_INTERVAL_MS,
  checkForUpdates,
  checkForUpdatesInBackground,
  createUpdaterStore,
  startAutomaticUpdateChecks,
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

describe('automatic update checks', () => {
  it('opens the dialog only when a background check finds an update', async () => {
    const quiet = createUpdaterStore();
    await checkForUpdatesInBackground(
      quiet,
      commands({ check: { state: 'upToDate', currentVersion: '0.1.0' } }),
    );
    expect(quiet.getState().dialogOpen).toBe(false);
    expect(quiet.getState().status).toEqual({ state: 'upToDate', currentVersion: '0.1.0' });

    const found = createUpdaterStore();
    await checkForUpdatesInBackground(found, commands({ check: available }));
    expect(found.getState().dialogOpen).toBe(true);
    expect(found.getState().status).toEqual(available);
  });

  it('stays quiet on failure and skips when busy, found, unconfigured or open', async () => {
    const failing = createUpdaterStore();
    await checkForUpdatesInBackground(failing, commands({ check: new Error('offline') }));
    expect(failing.getState().dialogOpen).toBe(false);
    expect(failing.getState().status).toEqual({ state: 'idle' });

    const skipped: UpdaterState[] = [
      available,
      { state: 'readyToRestart', version: '0.2.0' },
      { state: 'notConfigured' },
      { state: 'downloading', version: '0.2.0', downloaded: 0, total: null },
    ];
    for (const status of skipped) {
      const store = createUpdaterStore();
      store.getState().setStatus(status);
      const backend = commands({ check: available });
      await checkForUpdatesInBackground(store, backend);
      expect(backend.check).not.toHaveBeenCalled();
    }
    const open = createUpdaterStore();
    open.getState().openDialog();
    const backend = commands({ check: available });
    await checkForUpdatesInBackground(open, backend);
    expect(backend.check).not.toHaveBeenCalled();
  });

  it('checks after the launch delay, then daily, until stopped', () => {
    vi.useFakeTimers();
    try {
      const store = createUpdaterStore();
      const backend = commands({ check: { state: 'upToDate', currentVersion: '0.1.0' } });
      const stop = startAutomaticUpdateChecks(store, backend);
      vi.advanceTimersByTime(AUTOMATIC_CHECK_DELAY_MS - 1);
      expect(backend.check).not.toHaveBeenCalled();
      vi.advanceTimersByTime(1);
      expect(backend.check).toHaveBeenCalledTimes(1);
      store.getState().setStatus({ state: 'idle' });
      vi.advanceTimersByTime(AUTOMATIC_CHECK_INTERVAL_MS);
      expect(backend.check).toHaveBeenCalledTimes(2);
      stop();
      vi.advanceTimersByTime(AUTOMATIC_CHECK_INTERVAL_MS * 3);
      expect(backend.check).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
