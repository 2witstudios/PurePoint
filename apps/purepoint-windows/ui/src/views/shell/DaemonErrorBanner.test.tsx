import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../state/appState.ts';
import { AppStateContext } from './appContext.ts';
import {
  DaemonErrorBanner,
  GHUnavailableView,
  InlineErrorBanner,
  OverlayButton,
} from './DaemonErrorBanner.tsx';
import { createInstallerBannerStore, ShellBanners } from './ShellBanners.tsx';
import { makeTestApp } from './testApp.ts';

const invoke = vi.fn<(command: string, args?: unknown) => Promise<unknown>>();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: unknown) => invoke(command, args),
}));

const report = {
  binDir: 'C:\\Users\\me\\.pu\\bin',
  binaries: [],
  staleRemoved: 0,
  staleKept: 0,
  pathAdded: false,
  plugin: 'upToDate' as const,
  claude: 'notNeeded' as const,
  restartDaemon: false,
  daemonRestarted: false,
  errors: [],
};

function renderBanners(app: AppState, store = createInstallerBannerStore()) {
  render(
    <AppStateContext.Provider value={app}>
      <ShellBanners installerStore={store} />
    </AppStateContext.Provider>,
  );
  return store;
}

describe('DaemonErrorBanner', () => {
  it('shows the message and dismisses', () => {
    const onDismiss = vi.fn();
    render(<DaemonErrorBanner message="Daemon not running" onDismiss={onDismiss} />);
    expect(screen.getByRole('alert')).toHaveTextContent('Daemon not running');
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });

  it('offers an extra action', () => {
    const onClick = vi.fn();
    render(
      <DaemonErrorBanner message="m" onDismiss={vi.fn()} action={{ label: 'Retry', onClick }} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('ports the inline banner, the gh hint and the overlay button', () => {
    const onDismiss = vi.fn();
    const onClick = vi.fn();
    render(
      <>
        <InlineErrorBanner message="Push failed" onDismiss={onDismiss} />
        <GHUnavailableView />
        <OverlayButton icon="xmark" tooltip="Close panel" onClick={onClick} />
      </>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close panel' }));
    expect(onDismiss).toHaveBeenCalledOnce();
    expect(onClick).toHaveBeenCalledOnce();
    expect(screen.getByText('Install GitHub CLI for PR diffs')).toBeInTheDocument();
    expect(screen.getByText(/winget install GitHub\.cli/)).toBeInTheDocument();
  });
});

describe('ShellBanners', () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it('shows nothing while all is well', () => {
    const app = makeTestApp();
    renderBanners(app);
    act(() => {
      app.handleDaemonState({ state: 'starting' });
      app.handleDaemonState({ state: 'connected', pid: 42, launched: true });
    });
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('shows the daemon error and clears it on dismiss', () => {
    const app = makeTestApp();
    renderBanners(app);
    act(() => {
      app.setDaemonError('Lost the connection to pu-engine');
    });
    expect(screen.getByRole('alert')).toHaveTextContent('Lost the connection to pu-engine');
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(app.daemonError).toBeNull();
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('follows the daemon state: failed and incompatible daemons raise the banner', () => {
    const app = makeTestApp();
    renderBanners(app);
    act(() => {
      app.handleDaemonState({
        state: 'failed',
        kind: 'binary_not_found',
        message: 'pu-engine.exe not found',
      });
    });
    expect(screen.getByRole('alert')).toHaveTextContent('pu-engine.exe not found');
    act(() => {
      app.handleDaemonState({
        state: 'incompatible',
        daemonVersion: 13,
        appVersion: 14,
        message: 'Daemon protocol 13 does not match the app (14)',
      });
    });
    expect(screen.getByRole('alert')).toHaveTextContent('Daemon protocol 13');
  });

  it('shows an installer failure with Retry, and hides it on dismiss until the next state', async () => {
    const app = makeTestApp();
    const store = renderBanners(app);
    act(() => {
      store.getState().setInstaller({ state: 'running' });
    });
    expect(screen.queryByRole('alert')).toBeNull();
    act(() => {
      store.getState().setInstaller({ state: 'failed', message: 'Access denied', report: null });
    });
    expect(screen.getByRole('alert')).toHaveTextContent(
      "Couldn't install the command-line tools: Access denied",
    );

    invoke.mockResolvedValueOnce({ state: 'installed', report });
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(invoke).toHaveBeenCalledWith('installer_run', undefined);
    await waitFor(() => {
      expect(store.getState().installer.state).toBe('installed');
    });
    expect(screen.queryByRole('alert')).toBeNull();

    act(() => {
      store.getState().setInstaller({ state: 'failed', message: 'Disk full', report });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByRole('alert')).toBeNull();
    act(() => {
      store.getState().setInstaller({ state: 'failed', message: 'Disk still full', report });
    });
    expect(screen.getByRole('alert')).toHaveTextContent('Disk still full');
  });

  it('reports a Retry that itself fails', async () => {
    const app = makeTestApp();
    const store = renderBanners(app);
    act(() => {
      store.getState().setInstaller({ state: 'failed', message: 'first', report: null });
    });
    invoke.mockRejectedValueOnce({ message: 'backend gone' });
    fireEvent.click(screen.getByRole('button', { name: 'Retry' }));
    expect(await screen.findByText(/backend gone/)).toBeInTheDocument();
  });

  it('stacks the installer and daemon banners', () => {
    const app = makeTestApp();
    const store = renderBanners(app);
    act(() => {
      app.setDaemonError('daemon down');
      store.getState().setInstaller({ state: 'failed', message: 'no PATH', report: null });
    });
    expect(screen.getAllByRole('alert')).toHaveLength(2);
  });
});
