import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { UpdaterState } from '../../services/updater.ts';
import { createUpdaterStore } from '../../state/updaterStore.ts';
import { AboutUpdateStatus, UpdateDialog } from './UpdateDialog.tsx';

function fakeCommands() {
  return {
    check: vi.fn(() =>
      Promise.resolve<UpdaterState>({
        state: 'available',
        version: '0.2.0',
        currentVersion: '0.1.0',
        notes: 'Faster agents.',
        date: null,
      }),
    ),
    download: vi.fn(() =>
      Promise.resolve<UpdaterState>({ state: 'readyToRestart', version: '0.2.0' }),
    ),
    install: vi.fn(() => Promise.resolve<UpdaterState>({ state: 'installing', version: '0.2.0' })),
  };
}

describe('UpdateDialog', () => {
  it('walks check, available, download, ready and install', async () => {
    const store = createUpdaterStore();
    const commands = fakeCommands();
    render(
      <>
        <AboutUpdateStatus store={store} commands={commands} />
        <UpdateDialog store={store} commands={commands} />
      </>,
    );
    expect(screen.queryByRole('dialog')).toBeNull();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Check for Updates…' }));
      await Promise.resolve();
    });
    expect(screen.getByRole('dialog', { name: 'Software Update' })).toBeTruthy();
    expect(screen.getByText('A new version of PurePoint is available!')).toBeTruthy();
    expect(screen.getByText('Faster agents.')).toBeTruthy();
    expect(screen.getByRole('status').textContent).toBe('PurePoint 0.2.0 is available');

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Install Update' }));
      await Promise.resolve();
    });
    expect(commands.download).toHaveBeenCalledOnce();
    expect(screen.getByText('Ready to Install')).toBeTruthy();

    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Install and Relaunch' }));
      await Promise.resolve();
    });
    expect(commands.install).toHaveBeenCalledOnce();
    expect(screen.getByText('Installing update…', { selector: 'h2' })).toBeTruthy();
    // Nothing else may start while installing.
    const check = screen.getByRole('button', { name: 'Check for Updates…' });
    expect((check as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows "Updates not configured" and closes on OK and Escape', () => {
    const store = createUpdaterStore();
    store.getState().setStatus({ state: 'notConfigured' });
    store.getState().openDialog();
    render(<UpdateDialog store={store} commands={fakeCommands()} />);
    expect(screen.getByText('Updates not configured')).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    expect(screen.queryByRole('dialog')).toBeNull();

    act(() => {
      store.getState().openDialog();
    });
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('shows an error with its message', () => {
    const store = createUpdaterStore();
    store.getState().setStatus({ state: 'error', message: 'The update is improperly signed.' });
    store.getState().openDialog();
    render(<UpdateDialog store={store} commands={fakeCommands()} />);
    expect(screen.getByText('Update Error!')).toBeTruthy();
    expect(screen.getByText('The update is improperly signed.')).toBeTruthy();
  });
});
