// WIN-8.15: Open Project through the native folder picker, and local files handed to Windows.
import { afterEach, describe, expect, it, vi } from 'vitest';

const { invoke, open, openPath, openUrl, revealItemInDir } = vi.hoisted(() => ({
  invoke: vi.fn(() => Promise.resolve()),
  open: vi.fn<(options: unknown) => Promise<string | string[] | null>>(),
  openPath: vi.fn(() => Promise.resolve()),
  openUrl: vi.fn(() => Promise.resolve()),
  revealItemInDir: vi.fn(() => Promise.resolve()),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open }));
vi.mock('@tauri-apps/plugin-opener', () => ({ openPath, openUrl, revealItemInDir }));
vi.mock('@tauri-apps/api/core', () => ({ invoke }));

import { appLifecycleHooks, openProjectWithPicker } from './appLifecycle.ts';
import { pickFolder } from './dialog.ts';
import {
  isBlockedFromDefaultApp,
  openExternal,
  openInDefaultApp,
  revealInExplorer,
} from './opener.ts';

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
  it('should hand URLs and reveals to the opener plugin and files to the checked command', () => {
    openExternal('https://purepoint.dev');
    expect(openInDefaultApp('C:\\repo\\README.md')).toBe('opened');
    revealInExplorer('C:\\repo\\src');
    expect(openUrl).toHaveBeenCalledWith('https://purepoint.dev');
    expect(invoke).toHaveBeenCalledWith('open_in_default_app', { path: 'C:\\repo\\README.md' });
    expect(openPath).not.toHaveBeenCalled();
    expect(revealItemInDir).toHaveBeenCalledWith('C:\\repo\\src');
  });

  it('given a program or script, should refuse to open it in its default app', () => {
    const blocked = [
      'run.js',
      'x.vbs',
      'x.wsf',
      'x.hta',
      'x.bat',
      'x.CMD',
      'x.lnk',
      'x.url',
      'x.scr',
      'x.msi',
      'x.exe',
      'x.cpl',
      'x.jar',
      'x.appref-ms',
      'x.settingcontent-ms',
      'x.ps1',
      'x.com',
      'x.pif',
      'x.bat.',
      'x.exe . ',
      'a.txt:evil.exe',
      'x.{20D04FE0-3AEA-1069-A2D8-08002B30309D}',
    ];
    for (const name of blocked) {
      expect(isBlockedFromDefaultApp(`C:\\repo\\${name}`), name).toBe(true);
      expect(openInDefaultApp(`C:\\repo\\${name}`), name).toBe('blocked');
    }
    for (const name of ['README.md', 'a.txt', 'image.png', 'Makefile', '.env', 'js']) {
      expect(isBlockedFromDefaultApp(`C:\\repo\\${name}`), name).toBe(false);
    }
    expect(isBlockedFromDefaultApp('/srv/app/x.sh.js')).toBe(true);
    expect(invoke).not.toHaveBeenCalled();
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
