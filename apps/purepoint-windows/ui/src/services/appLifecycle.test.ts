// WIN-8.15: Open Project through the native folder picker, and local files handed to Windows.
import { afterEach, describe, expect, it, vi } from 'vitest';

const { open, openPath, openUrl, revealItemInDir } = vi.hoisted(() => ({
  open: vi.fn<(options: unknown) => Promise<string | string[] | null>>(),
  openPath: vi.fn(() => Promise.resolve()),
  openUrl: vi.fn(() => Promise.resolve()),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openPath, openUrl, revealItemInDir }));

import { appLifecycleHooks, openProjectWithPicker } from './appLifecycle.ts';
import { pickFolder } from './dialog.ts';
import { openExternal, openInDefaultApp, revealInExplorer } from './opener.ts';

type SpyGlobal = { __PU_DIALOG_SPY__?: (string | null)[]; __PU_OPENER_SPY__?: string[] };
const spies = globalThis as SpyGlobal;

afterEach(() => {
  delete spies.__PU_DIALOG_SPY__;
  delete spies.__PU_OPENER_SPY__;
  vi.clearAllMocks();
});

describe('pickFolder', () => {
  it('given a pick, should ask the dialog plugin for one folder and resolve its path', async () => {
    open.mockResolvedValueOnce('C:\\Users\\me\\app');
    await expect(pickFolder({ title: 'Choose a project directory' })).resolves.toBe(
      'C:\\Users\\me\\app',
    );
    expect(open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: 'Choose a project directory',
    });
  });

  it('given Cancel, should resolve null', async () => {
    open.mockResolvedValueOnce(null);
    await expect(pickFolder()).resolves.toBeNull();
  });

  it('given the test spy, should take queued paths instead of showing the dialog', async () => {
    spies.__PU_DIALOG_SPY__ = ['C:\\one', null];
    await expect(pickFolder()).resolves.toBe('C:\\one');
    await expect(pickFolder()).resolves.toBeNull();
    await expect(pickFolder()).resolves.toBeNull();
    expect(open).not.toHaveBeenCalled();
  });
});

describe('openProjectWithPicker', () => {
  it('given a folder, should open it as a project', async () => {
    const app = { openProject: vi.fn() };
    const pick = vi.fn(() => Promise.resolve('D:\\code\\site'));
    await expect(openProjectWithPicker(app, pick)).resolves.toBe('D:\\code\\site');
    expect(pick).toHaveBeenCalledWith({ title: 'Choose a project directory' });
    expect(app.openProject).toHaveBeenCalledWith('D:\\code\\site');
  });

  it('given Cancel, should open nothing', async () => {
    const app = { openProject: vi.fn() };
    await expect(openProjectWithPicker(app, () => Promise.resolve(null))).resolves.toBeNull();
    expect(app.openProject).not.toHaveBeenCalled();
  });

  it('given the default hook, should run the folder picker', async () => {
    spies.__PU_DIALOG_SPY__ = ['C:\\picked'];
    const app = { openProject: vi.fn() };
    appLifecycleHooks.openProject(app);
    await vi.waitFor(() => {
      expect(app.openProject).toHaveBeenCalledWith('C:\\picked');
    });
  });
});

describe('opener', () => {
  it('should hand URLs, files and reveals to the opener plugin', () => {
    openExternal('https://purepoint.dev');
    openInDefaultApp('C:\\repo\\README.md');
    revealInExplorer('C:\\repo\\src');
    expect(openUrl).toHaveBeenCalledWith('https://purepoint.dev');
    expect(openPath).toHaveBeenCalledWith('C:\\repo\\README.md');
    expect(revealItemInDir).toHaveBeenCalledWith('C:\\repo\\src');
  });

  it('given the test spy, should record instead of opening anything', () => {
    spies.__PU_OPENER_SPY__ = [];
    openExternal('https://purepoint.dev');
    openInDefaultApp('C:\\a.txt');
    revealInExplorer('C:\\b');
    expect(spies.__PU_OPENER_SPY__).toEqual([
      'https://purepoint.dev',
      'open-path:C:\\a.txt',
      'reveal:C:\\b',
    ]);
    expect(openUrl).not.toHaveBeenCalled();
    expect(openPath).not.toHaveBeenCalled();
    expect(revealItemInDir).not.toHaveBeenCalled();
  });
});
