// Settings (WIN-8.11): each pane edits its preference and the store that applies it.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { keyBinding } from '../../model/keyBinding.ts';
import type { Request, Response } from '../../protocol';
import { AgentConfigState } from '../../state/agentConfigState.ts';
import { createKeyBindingStore } from '../../state/keyBindingStore.ts';
import { PointGuardServiceState } from '../../state/pointGuardService.ts';
import { createSettingsStore } from '../../state/settingsState.ts';
import { useThemeStore } from '../../theme/themeStore.ts';
import { createTerminalFontStore } from '../terminal/terminalFontStore.ts';
import { makeTestApp, memoryStorage } from '../shell/testApp.ts';
import { SettingsAboutView } from './SettingsAboutView.tsx';
import { SettingsAgentsView } from './SettingsAgentsView.tsx';
import { SettingsDisplayView } from './SettingsDisplayView.tsx';
import { SettingsGeneralView } from './SettingsGeneralView.tsx';
import { SettingsHotkeysView } from './SettingsHotkeysView.tsx';
import {
  SettingsPointGuardView,
  skipsPermissions,
  withSkipPermissions,
} from './SettingsPointGuardView.tsx';

afterEach(() => {
  useThemeStore.getState().setPreference('system');
});

function press(code: string, mods: { ctrl?: boolean; alt?: boolean; shift?: boolean } = {}) {
  act(() => {
    window.dispatchEvent(
      new KeyboardEvent('keydown', {
        code,
        ctrlKey: mods.ctrl ?? false,
        altKey: mods.alt ?? false,
        shiftKey: mods.shift ?? false,
        bubbles: true,
        cancelable: true,
      }),
    );
  });
}

describe('Display', () => {
  test('given an appearance and a font size, should apply both through their stores', () => {
    const fontStore = createTerminalFontStore(null);
    const store = createSettingsStore({
      storage: null,
      setTerminalFontSize: (size) => {
        fontStore.getState().setFontSize(size);
      },
    });
    render(<SettingsDisplayView store={store} fontStore={fontStore} />);
    fireEvent.click(screen.getByRole('radio', { name: 'Dark' }));
    expect(useThemeStore.getState().preference).toBe('dark');
    expect(screen.getByRole('radio', { name: 'Dark' })).toHaveAttribute('aria-checked', 'true');

    fireEvent.change(screen.getByLabelText('Terminal font size'), { target: { value: '17' } });
    expect(fontStore.getState().fontSize).toBe(17);
    expect(screen.getByTestId('terminal-font-size-value')).toHaveTextContent('17pt');

    fireEvent.change(screen.getByLabelText('Grid gap'), { target: { value: '5' } });
    expect(store.getState().gridGap).toBe(5);
  });
});

describe('General', () => {
  test('given the toggles and the palette order, should edit the settings', () => {
    const app = makeTestApp();
    const store = createSettingsStore({ storage: memoryStorage() });
    render(<SettingsGeneralView app={app} store={store} />);
    fireEvent.click(screen.getByRole('switch', { name: 'Use point guard names' }));
    expect(store.getState().usePointGuardNames).toBe(false);
    fireEvent.click(screen.getByRole('switch', { name: 'Completion notifications' }));
    expect(store.getState().signalNotifications.completion).toBe(true);
    fireEvent.click(screen.getByRole('switch', { name: 'Launch at login' }));
    expect(store.getState().launchAtLogin).toBe(true);

    const list = screen.getByRole('list', { name: 'Command palette order' });
    const first = within(list).getAllByRole('listitem')[0];
    const firstId = first?.getAttribute('data-id') ?? '';
    const name = first?.textContent ?? '';
    fireEvent.click(within(list).getByRole('button', { name: `Move ${name} down` }));
    expect(within(list).getAllByRole('listitem')[1]).toHaveAttribute('data-id', firstId);
    fireEvent.click(screen.getByRole('button', { name: 'Reset' }));
    expect(store.getState().commandPaletteOrder).toEqual([]);
  });
});

describe('Hotkeys', () => {
  test('given a recorded free chord, should save it and stop recording', () => {
    const store = createKeyBindingStore(null);
    render(<SettingsHotkeysView store={store} />);
    fireEvent.click(screen.getByTestId('hotkey-row-navAgents'));
    expect(store.getState().recordingAction).toBe('navAgents');
    expect(screen.getByText('Press shortcut…')).toBeInTheDocument();
    press('ShiftLeft', { shift: true });
    expect(store.getState().recordingAction).toBe('navAgents');
    press('KeyY', { ctrl: true, shift: true });
    expect(store.getState().binding('navAgents')).toEqual(keyBinding('KeyY', 'ctrl', 'shift'));
    expect(store.getState().recordingAction).toBeNull();
    expect(screen.getByTestId('hotkey-keys-navAgents')).toHaveTextContent('CtrlShiftY');

    fireEvent.click(screen.getByRole('button', { name: 'Reset Agents to default' }));
    expect(store.getState().isCustomized('navAgents')).toBe(false);
  });

  test('given a reserved terminal chord, should refuse it and keep recording', () => {
    const store = createKeyBindingStore(null);
    render(<SettingsHotkeysView store={store} />);
    fireEvent.click(screen.getByTestId('hotkey-row-newAgent'));
    press('KeyC', { ctrl: true });
    expect(screen.getByRole('status')).toHaveTextContent('Ctrl+C is kept for the terminal');
    expect(store.getState().isCustomized('newAgent')).toBe(false);
    expect(store.getState().recordingAction).toBe('newAgent');
    press('Escape');
    expect(store.getState().recordingAction).toBeNull();
  });

  test('given a duplicate chord, should offer Override, which swaps the two', () => {
    const store = createKeyBindingStore(null);
    render(<SettingsHotkeysView store={store} />);
    fireEvent.click(screen.getByTestId('hotkey-row-newAgent'));
    press('KeyT', { ctrl: true, shift: true });
    expect(screen.getByText('Already used by New Tab')).toBeInTheDocument();
    expect(store.getState().isCustomized('newAgent')).toBe(false);
    fireEvent.click(screen.getByRole('button', { name: 'Override' }));
    expect(store.getState().binding('newAgent')).toEqual(keyBinding('KeyT', 'ctrl', 'shift'));
    expect(store.getState().binding('newTab')).toEqual(keyBinding('KeyN', 'ctrl', 'shift'));

    fireEvent.click(screen.getByRole('button', { name: 'Reset All' }));
    expect(store.getState().overrides).toEqual({});
  });

  test('given a recording, should keep the keys from other listeners', () => {
    const store = createKeyBindingStore(null);
    const other = vi.fn();
    window.addEventListener('keydown', other);
    render(<SettingsHotkeysView store={store} />);
    fireEvent.click(screen.getByTestId('hotkey-row-settings'));
    press('KeyB', { alt: true });
    window.removeEventListener('keydown', other);
    expect(other).not.toHaveBeenCalled();
    expect(store.getState().binding('settings')).toEqual(keyBinding('KeyB', 'alt'));
  });
});

function fakeDaemon() {
  const sent: Request[] = [];
  let launchArgs: Record<string, string> = {};
  let commands: Record<string, string> = {};
  const sendRequest = (request: Request): Promise<Response> => {
    sent.push(request);
    if (request.type === 'update_global_agent_settings') {
      launchArgs = { ...launchArgs, ...(request.launch_args ?? {}) };
      commands = { ...commands, ...(request.commands ?? {}) };
    }
    return Promise.resolve({
      type: 'global_agent_settings_report',
      codex_yolo: false,
      agent_status_tracking: true,
      launch_args: launchArgs,
      commands,
    } as Response);
  };
  const config = new AgentConfigState({ ensureDaemon: () => Promise.resolve(), sendRequest });
  return { config, sent };
}

describe('Agents', () => {
  test('given an All Projects launch arg, should save it through the daemon', async () => {
    const { config, sent } = fakeDaemon();
    render(<SettingsAgentsView app={makeTestApp()} config={config} />);
    const field = await screen.findByRole('textbox', { name: 'Arguments claude' });
    await waitFor(() => {
      expect(field.closest('fieldset')).not.toBeDisabled();
    });
    fireEvent.change(field, { target: { value: '--model opus' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    await waitFor(() => {
      expect(config.machineLaunchArgs.claude).toBe('--model opus');
    });
    const update = sent.find((r) => r.type === 'update_global_agent_settings');
    expect(update).toMatchObject({ launch_args: { claude: '--model opus' } });
    expect(screen.getByText('No Project Open')).toBeInTheDocument();
  });
});

describe('Point Guard', () => {
  test('given launch args, should add and remove the skip-permissions flag as a word', () => {
    expect(withSkipPermissions('--model opus', true)).toBe(
      '--dangerously-skip-permissions --model opus',
    );
    expect(withSkipPermissions('--dangerously-skip-permissions --model opus', false)).toBe(
      '--model opus',
    );
    expect(skipsPermissions('--model opus')).toBe(false);
    expect(skipsPermissions('a --dangerously-skip-permissions')).toBe(true);
  });

  test('given the pane, should show status, import a pairing and toggle skip permissions', async () => {
    const { config } = fakeDaemon();
    const service = new PointGuardServiceState({
      backend: {
        status: () => Promise.reject(new Error('unused')),
        start: () => Promise.resolve(),
        stop: () => Promise.resolve(),
        restart: () => Promise.resolve(),
        request: () => Promise.resolve(null),
        chatConnection: () => Promise.reject(new Error('unused')),
        validateFolder: (path) => Promise.resolve(path),
        onState: () => Promise.resolve(() => undefined),
      },
    });
    const onImport = vi
      .fn()
      .mockResolvedValueOnce('ws://100.100.1.2:8787/v1')
      .mockRejectedValueOnce(new Error('This is not a Point Guard pairing code.'));
    render(<SettingsPointGuardView config={config} service={service} onImport={onImport} />);
    expect(screen.getByTestId('point-guard-status')).toHaveTextContent('Not running');
    act(() => {
      service.apply({
        phase: 'Point Guard ready',
        error: null,
        ready: true,
        running: true,
        instanceId: 'run-1',
        remoteEndpoint: null,
        layout: 'bundled',
        restarts: 0,
      });
    });
    expect(screen.getByTestId('point-guard-status')).toHaveTextContent('Connected');

    fireEvent.click(screen.getByRole('radio', { name: 'Address and secret' }));
    fireEvent.change(screen.getByLabelText('Point Guard address'), {
      target: { value: 'ws://100.100.1.2:8787/v1' },
    });
    fireEvent.change(screen.getByLabelText('Pairing secret'), {
      target: { value: 'x'.repeat(32) },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    expect(await screen.findByText('Saved pairing for ws://100.100.1.2:8787/v1.')).toBeVisible();
    expect(onImport).toHaveBeenCalledWith({
      endpoint: 'ws://100.100.1.2:8787/v1',
      secret: 'x'.repeat(32),
    });

    fireEvent.click(screen.getByRole('radio', { name: 'Pairing code' }));
    fireEvent.change(screen.getByLabelText('Pairing code'), { target: { value: 'nope' } });
    fireEvent.click(screen.getByRole('button', { name: 'Import' }));
    expect(await screen.findByText('This is not a Point Guard pairing code.')).toBeVisible();

    const toggle = screen.getByRole('switch', { name: 'Skip permission prompts' });
    await waitFor(() => {
      expect(toggle).not.toBeDisabled();
    });
    fireEvent.click(toggle);
    await waitFor(() => {
      expect(config.machineLaunchArgs.claude).toBe('--dangerously-skip-permissions');
    });
  });
});

describe('About', () => {
  test('given a version, should show it, the build and Check for Updates', () => {
    render(<SettingsAboutView version="1.2.3" />);
    expect(screen.getByTestId('about-version')).toHaveTextContent('1.2.3');
    expect(screen.getByTestId('about-build').textContent).not.toBe('');
    expect(screen.getByRole('img', { name: 'PurePoint logo' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Check for Updates…' })).toBeInTheDocument();
  });
});
