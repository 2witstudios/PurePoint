// The e2e hooks are honoured only where `__PU_TEST_HOOKS__` is true (vite.config.ts): a release
// bundle ignores them, so a page script cannot silence the opener or answer the folder picker.
import { afterEach, describe, expect, it, vi } from 'vitest';

const { enabled, invoke, open, openUrl, revealItemInDir } = vi.hoisted(() => ({
  enabled: { value: true },
  invoke: vi.fn(() => Promise.resolve()),
  open: vi.fn(() => Promise.resolve(null)),
  openUrl: vi.fn(() => Promise.resolve()),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));
vi.mock('./testHooks', () => ({ testHooksEnabled: () => enabled.value }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openUrl, revealItemInDir }));

import { statsEnabled } from '../views/terminal/terminalStats';
import { pickFolder } from './dialog';
import { openExternal, openInDefaultApp, revealInExplorer } from './opener';
import { testHooksEnabled as realTestHooksEnabled } from './testHooks';

type Hooks = {
  __PU_OPENER_SPY__?: string[];
  __PU_DIALOG_SPY__?: (string | null)[];
  __PU_TERMINAL_STATS__?: boolean;
};
const hooks = globalThis as Hooks;

afterEach(() => {
  delete hooks.__PU_OPENER_SPY__;
  delete hooks.__PU_DIALOG_SPY__;
  delete hooks.__PU_TERMINAL_STATS__;
  enabled.value = true;
  vi.clearAllMocks();
});

describe('test hooks', () => {
  it('given a build without test hooks, should ignore every spy', async () => {
    enabled.value = false;
    hooks.__PU_OPENER_SPY__ = [];
    hooks.__PU_DIALOG_SPY__ = ['C:\\planted'];
    hooks.__PU_TERMINAL_STATS__ = true;
    openExternal('https://purepoint.dev');
    openInDefaultApp('C:\\repo\\a.txt');
    revealInExplorer('C:\\repo');
    expect(hooks.__PU_OPENER_SPY__).toEqual([]);
    expect(openUrl).toHaveBeenCalledWith('https://purepoint.dev');
    expect(invoke).toHaveBeenCalledWith('open_in_default_app', { path: 'C:\\repo\\a.txt' });
    expect(revealItemInDir).toHaveBeenCalledWith('C:\\repo');
    await expect(pickFolder()).resolves.toBeNull();
    expect(open).toHaveBeenCalled();
    expect(hooks.__PU_DIALOG_SPY__).toEqual(['C:\\planted']);
    expect(statsEnabled()).toBe(false);
  });

  it('given a test build, should honour them', async () => {
    hooks.__PU_OPENER_SPY__ = [];
    hooks.__PU_DIALOG_SPY__ = ['C:\\picked'];
    hooks.__PU_TERMINAL_STATS__ = true;
    openExternal('https://purepoint.dev');
    expect(hooks.__PU_OPENER_SPY__).toEqual(['https://purepoint.dev']);
    await expect(pickFolder()).resolves.toBe('C:\\picked');
    expect(statsEnabled()).toBe(true);
    expect(openUrl).not.toHaveBeenCalled();
  });

  it('is on under Vitest', async () => {
    const actual = await vi.importActual<{ testHooksEnabled: typeof realTestHooksEnabled }>(
      './testHooks',
    );
    expect(actual.testHooksEnabled()).toBe(true);
  });
});
