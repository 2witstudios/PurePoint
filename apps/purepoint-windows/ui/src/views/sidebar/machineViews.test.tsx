// The machine views (WIN-8.14): Add Machine, a machine row's actions, and the remote folder picker.
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Request, Response } from '../../protocol';
import { makeMachine } from '../../model/sidebarFixtures.ts';
import type { MachineState } from '../../state/appState.ts';
import { AppStateContext } from '../shell/appContext.ts';
import { makeTestApp } from '../shell/testApp.ts';
import { AddMachineSheet } from './AddMachineSheet.tsx';
import { RemoteFolderPicker } from './RemoteFolderPicker.tsx';
import { SidebarOutlineViewController } from './SidebarOutlineViewController.tsx';

describe('AddMachineSheet', () => {
  function renderSheet() {
    const app = makeTestApp();
    const added: [string, string][] = [];
    vi.spyOn(app, 'addMachine').mockImplementation((name, sshHost) => {
      added.push([name, sshHost]);
      return Promise.resolve(null);
    });
    const onDismiss = vi.fn();
    const testConnection = vi.fn(() => Promise.resolve('Connected. The machine runs Windows.'));
    render(
      <AppStateContext.Provider value={app}>
        <AddMachineSheet
          onDismiss={onDismiss}
          loadAliases={() => Promise.resolve(['studio-mac', 'build'])}
          testConnection={testConnection}
        />
      </AppStateContext.Provider>,
    );
    return { added, onDismiss, testConnection };
  }

  it('given a name and host, should add the machine and close', async () => {
    const { added, onDismiss } = renderSheet();
    const add = screen.getByRole('button', { name: 'Add' });
    expect(add).toBeDisabled();
    fireEvent.change(screen.getByPlaceholderText('studio'), { target: { value: ' studio ' } });
    fireEvent.change(screen.getByPlaceholderText('alias or user@host[:port]'), {
      target: { value: 'me@box:2222' },
    });
    fireEvent.click(add);
    await waitFor(() => {
      expect(onDismiss).toHaveBeenCalled();
    });
    expect(added).toEqual([['studio', 'me@box:2222']]);
  });

  it('given an invalid name, should show the Mac message and add nothing', () => {
    const { added } = renderSheet();
    fireEvent.change(screen.getByPlaceholderText('studio'), { target: { value: 'bad name' } });
    fireEvent.change(screen.getByPlaceholderText('alias or user@host[:port]'), {
      target: { value: 'box' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Add' }));
    expect(screen.getByRole('alert')).toHaveTextContent(
      "Use 1–64 letters, digits, '.', '_' or '-', starting with a letter or digit.",
    );
    expect(added).toEqual([]);
  });

  it('given a host from the ssh config, should suggest its name; Test Connection reports', async () => {
    const { testConnection } = renderSheet();
    await act(async () => {
      await Promise.resolve();
    });
    fireEvent.change(screen.getByPlaceholderText('alias or user@host[:port]'), {
      target: { value: 'studio-mac' },
    });
    expect(screen.getByPlaceholderText('studio')).toHaveValue('studio-mac');
    fireEvent.click(screen.getByRole('button', { name: 'Test Connection' }));
    expect(testConnection).toHaveBeenCalledWith('studio-mac');
    expect(await screen.findByText('Connected. The machine runs Windows.')).toBeInTheDocument();
  });
});

describe('machine rows', () => {
  it('given a stopped daemon, should offer Start Daemon; a running one, Open Remote Project', () => {
    const onStartDaemon = vi.fn();
    const onOpenRemoteProject = vi.fn();
    const stopped = makeMachine('studio', {
      state: { kind: 'connected', daemon: { kind: 'stopped' } },
      statusText: 'Daemon stopped',
    });
    const running = makeMachine('build', {
      state: { kind: 'connected', daemon: { kind: 'running' } },
      statusText: 'Connected',
    });
    render(
      <SidebarOutlineViewController
        projects={[]}
        machines={[stopped, running]}
        onStartDaemon={onStartDaemon}
        onOpenRemoteProject={onOpenRemoteProject}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Start Daemon' }));
    expect(onStartDaemon).toHaveBeenCalledWith(stopped.id);
    const open = screen.getAllByRole('button', { name: 'Open Remote Project' });
    expect(open).toHaveLength(1);
    fireEvent.click(open[0] as HTMLElement);
    expect(onOpenRemoteProject).toHaveBeenCalledWith(running.id);
  });
});

describe('RemoteFolderPicker', () => {
  it('given a machine, should list its home folders and open the one chosen', async () => {
    const requests: Request[] = [];
    const send = (request: Request): Promise<Response> => {
      requests.push(request);
      return Promise.resolve({
        type: 'fs_list_result',
        path: '/home/me',
        truncated: false,
        entries: ['zeta', 'app'].map((name) => ({
          name,
          path: `/home/me/${name}`,
          is_dir: true,
          is_symlink: false,
          size: 0,
          modified_ms: 0,
          ignored: false,
        })),
      });
    };
    const openProject = vi.fn(() => Promise.resolve());
    const onDismiss = vi.fn();
    const machine = {
      machine: { id: 'm1', name: 'studio', sshHost: 'studio' },
      client: { send },
      openProject,
    } as unknown as MachineState;
    render(<RemoteFolderPicker machine={machine} onDismiss={onDismiss} />);
    expect(await screen.findByTestId('remote-folder-path')).toHaveTextContent('/home/me');
    expect(requests[0]).toMatchObject({ type: 'fs_list', path: '~', directories_only: true });
    const rows = screen.getAllByRole('option');
    expect(rows.map((row) => row.textContent)).toEqual(['app', 'zeta']);
    fireEvent.click(screen.getByTestId('remote-folder-app'));
    fireEvent.click(screen.getByRole('button', { name: 'Open' }));
    await waitFor(() => {
      expect(onDismiss).toHaveBeenCalled();
    });
    expect(openProject).toHaveBeenCalledWith('/home/me/app');
  });
});
