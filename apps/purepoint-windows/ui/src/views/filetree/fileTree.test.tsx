// WIN-8.4: the file tree views (FileTreeSidebarView, FileTreeOutlineView(Controller)), their row
// model and path helpers, over an in-memory host.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { pathName, parentPath, relativePath, samePath, joinPath } from '../../model/fileNode';
import { memoryFileHost } from '../../services/fileHost.fixtures';
import { stubScrollViewLayout } from '../../test/virtualLayout';
import { FileTreeState } from '../../state/fileTreeState';
import { FileTreeSidebarView } from './FileTreeSidebarView';
import { validFileName } from './FileTreeDialogs';
import { fileTreeMenuItems, type FileTreeActions } from './FileTreeOutlineViewController';
import { fileTreeRows } from './fileTreeRows';

const ROOT = 'C:\\repo';

// The outline is 240 × 2000 px, so the virtualizer draws every row.
stubScrollViewLayout('file-tree-outline', 240, 2000);

interface SpyGlobal {
  __PU_OPENER_SPY__?: string[];
}

afterEach(() => {
  delete (globalThis as SpyGlobal).__PU_OPENER_SPY__;
});

async function mounted(isLocal = true) {
  const host = memoryFileHost(
    {
      [`${ROOT}\\README.md`]: '# hi',
      [`${ROOT}\\src\\main.ts`]: 'let a = 1;',
      [`${ROOT}\\src\\util\\x.rs`]: 'fn x() {}',
    },
    { folders: [ROOT], isLocal },
  );
  const state = new FileTreeState();
  await state.load(ROOT, host);
  const onFileSelected = vi.fn();
  const onHide = vi.fn();
  render(
    <FileTreeSidebarView fileTreeState={state} onHide={onHide} onFileSelected={onFileSelected} />,
  );
  return { host, state, onFileSelected, onHide };
}

const row = (name: string) => screen.getByRole('treeitem', { name });
const rowNames = () =>
  screen.queryAllByRole('treeitem').map((item) => item.getAttribute('aria-label'));

describe('path helpers', () => {
  it('should split, join and relate Windows and POSIX paths', () => {
    expect(pathName('C:\\repo\\src\\a.ts')).toBe('a.ts');
    expect(pathName('/srv/app/b.md')).toBe('b.md');
    expect(parentPath('C:\\repo\\a.ts')).toBe('C:\\repo');
    expect(parentPath('C:\\a.ts')).toBe('C:\\');
    expect(parentPath('/a.ts')).toBe('/');
    expect(joinPath('C:\\repo', 'b.ts')).toBe('C:\\repo\\b.ts');
    expect(joinPath('/srv/app', 'b.ts')).toBe('/srv/app/b.ts');
    expect(relativePath('C:\\Repo', 'c:\\repo\\src\\a.ts')).toBe('src/a.ts');
    expect(relativePath('/srv/App', '/srv/app/a.ts')).toBeNull();
    expect(relativePath('C:\\repo', 'C:\\repository\\a.ts')).toBeNull();
    expect(samePath('C:\\repo\\', 'c:/repo')).toBe(true);
  });

  it('should accept only single-component file names', () => {
    expect(validFileName('notes.md')).toBe(true);
    for (const bad of ['', ' ', '.', '..', 'a/b', 'a\\b', 'a:b', 'a?b', 'a*b', 'a|b']) {
      expect(validFileName(bad)).toBe(false);
    }
  });
});

describe('fileTreeRows', () => {
  const leaf = (name: string, parent = '') => ({
    name,
    absolutePath: `${ROOT}\\${parent}${name}`,
    relativePath: `${parent.replace(/\\/g, '/')}${name}`,
    isDirectory: false,
    children: [],
  });
  const nodes = [
    {
      ...leaf('src'),
      isDirectory: true,
      children: [leaf('main.ts', 'src\\'), leaf('notes.md', 'src\\')],
    },
    leaf('README.md'),
  ];

  it('given expanded folders, should flatten with depths', () => {
    expect(fileTreeRows(nodes, new Set()).map((r) => r.node.name)).toEqual(['src', 'README.md']);
    const rows = fileTreeRows(nodes, new Set([`${ROOT}\\src`]));
    expect(rows.map((r) => [r.node.name, r.depth, r.expanded])).toEqual([
      ['src', 0, true],
      ['main.ts', 1, false],
      ['notes.md', 1, false],
      ['README.md', 0, false],
    ]);
  });

  it('given a search query, should keep matches and the folders leading to them', () => {
    const rows = fileTreeRows(nodes, new Set([`${ROOT}\\src`]), 'MAIN');
    expect(rows.map((r) => r.node.name)).toEqual(['src', 'main.ts']);
  });
});

describe('fileTreeMenuItems', () => {
  const actions = Object.fromEntries(
    [
      'onFileSelected',
      'onNodeExpanded',
      'onNodeCollapsed',
      'onOpenInDefaultApp',
      'onRevealInExplorer',
      'onCopyPath',
      'onCopyRelativePath',
      'onRename',
      'onDelete',
    ].map((name) => [name, vi.fn()]),
  ) as unknown as FileTreeActions;
  const file = {
    name: 'a.ts',
    absolutePath: `${ROOT}\\a.ts`,
    relativePath: 'a.ts',
    isDirectory: false,
    children: [],
  };

  it('given a local file, should offer the Mac menu plus Open, Rename and Delete', () => {
    expect(
      fileTreeMenuItems(file, { isLocal: true, canRename: true }, actions).map((i) => i.title),
    ).toEqual([
      'Open',
      'Open in Default App',
      'Reveal in Explorer',
      'Copy Path',
      'Copy Relative Path',
      'Rename…',
      'Delete…',
    ]);
  });

  it('given a local folder, should not offer to open it', () => {
    const folder = { ...file, name: 'src', isDirectory: true };
    expect(
      fileTreeMenuItems(folder, { isLocal: true, canRename: true }, actions).map((i) => i.title),
    ).toEqual(['Reveal in Explorer', 'Copy Path', 'Copy Relative Path', 'Rename…', 'Delete…']);
  });

  it('given a remote file, should offer no Explorer items and no rename', () => {
    expect(
      fileTreeMenuItems(file, { isLocal: false, canRename: false }, actions).map((i) => i.title),
    ).toEqual(['Open', 'Copy Path', 'Copy Relative Path', 'Delete…']);
  });
});

describe('FileTreeSidebarView', () => {
  it('shows the header, search field and the root listing', async () => {
    const { onHide } = await mounted();
    expect(screen.getByText('Files')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('Search files...')).toBeInTheDocument();
    expect(rowNames()).toEqual(['src', 'README.md']);
    expect(row('src')).toHaveAttribute('aria-expanded', 'false');
    fireEvent.click(screen.getByRole('button', { name: 'Hide file tree' }));
    expect(onHide).toHaveBeenCalled();
  });

  it('expands and collapses a folder and opens a file', async () => {
    const { onFileSelected } = await mounted();
    fireEvent.click(row('src'));
    await waitFor(() => {
      expect(rowNames()).toEqual(['src', 'util', 'main.ts', 'README.md']);
    });
    expect(row('main.ts')).toHaveAttribute('aria-level', '2');
    fireEvent.click(row('main.ts'));
    expect(onFileSelected).toHaveBeenCalledWith(`${ROOT}\\src\\main.ts`, 'main.ts');
    expect(row('main.ts')).toHaveAttribute('aria-selected', 'true');
    fireEvent.click(row('src'));
    await waitFor(() => {
      expect(rowNames()).toEqual(['src', 'README.md']);
    });
  });

  it('moves the selection with the keyboard and opens with Enter', async () => {
    const { onFileSelected } = await mounted();
    const outline = screen.getByRole('tree');
    fireEvent.keyDown(outline, { key: 'ArrowDown' });
    expect(row('src')).toHaveAttribute('aria-selected', 'true');
    fireEvent.keyDown(outline, { key: 'ArrowRight' });
    await waitFor(() => {
      expect(rowNames()).toContain('main.ts');
    });
    fireEvent.keyDown(outline, { key: 'ArrowDown' });
    fireEvent.keyDown(outline, { key: 'ArrowDown' });
    fireEvent.keyDown(outline, { key: 'Enter' });
    expect(onFileSelected).toHaveBeenCalledWith(`${ROOT}\\src\\main.ts`, 'main.ts');
  });

  it('filters by the search field', async () => {
    const { state } = await mounted();
    const src = state.rootNodes[0];
    if (src === undefined) throw new Error('no src');
    await act(() => state.expandNode(src));
    fireEvent.change(screen.getByPlaceholderText('Search files...'), { target: { value: 'read' } });
    expect(rowNames()).toEqual(['README.md']);
  });

  it('reveals, opens and copies paths from the context menu', async () => {
    const opened: string[] = [];
    (globalThis as SpyGlobal).__PU_OPENER_SPY__ = opened;
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const { onFileSelected } = await mounted();
    const menuItem = (target: string, title: string) => {
      fireEvent.contextMenu(row(target), { clientX: 10, clientY: 10 });
      fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: title }));
    };
    menuItem('README.md', 'Reveal in Explorer');
    menuItem('README.md', 'Open in Default App');
    expect(opened).toEqual([`reveal:${ROOT}\\README.md`, `open-path:${ROOT}\\README.md`]);
    menuItem('README.md', 'Copy Path');
    menuItem('README.md', 'Copy Relative Path');
    expect(writeText.mock.calls).toEqual([[`${ROOT}\\README.md`], ['README.md']]);
    menuItem('README.md', 'Open');
    expect(onFileSelected).toHaveBeenCalledWith(`${ROOT}\\README.md`, 'README.md');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('refuses to open a script in its default app and offers Reveal in Explorer', async () => {
    const opened: string[] = [];
    (globalThis as SpyGlobal).__PU_OPENER_SPY__ = opened;
    const host = memoryFileHost(
      { [`${ROOT}\\build.cmd`]: 'echo hi', [`${ROOT}\\notes.txt`]: 'n' },
      { folders: [ROOT], isLocal: true },
    );
    const state = new FileTreeState();
    await state.load(ROOT, host);
    render(<FileTreeSidebarView fileTreeState={state} onHide={vi.fn()} onFileSelected={vi.fn()} />);
    const menuItem = (target: string, title: string) => {
      fireEvent.contextMenu(row(target), { clientX: 10, clientY: 10 });
      fireEvent.click(within(screen.getByRole('menu')).getByRole('menuitem', { name: title }));
    };
    menuItem('build.cmd', 'Open in Default App');
    const alert = screen.getByRole('alertdialog', { name: 'build.cmd was not opened' });
    expect(alert).toHaveTextContent('Windows would run this file');
    expect(opened).toEqual([]);
    fireEvent.click(within(alert).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    menuItem('build.cmd', 'Open in Default App');
    fireEvent.click(screen.getByRole('button', { name: 'Reveal in Explorer' }));
    expect(opened).toEqual([`reveal:${ROOT}\\build.cmd`]);
    menuItem('notes.txt', 'Open in Default App');
    expect(opened).toEqual([`reveal:${ROOT}\\build.cmd`, `open-path:${ROOT}\\notes.txt`]);
  });

  it('closes the context menu on Escape', async () => {
    await mounted();
    fireEvent.contextMenu(row('README.md'), { clientX: 10, clientY: 10 });
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('renames a file after asking for the new name', async () => {
    const { host } = await mounted();
    fireEvent.contextMenu(row('README.md'), { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename…' }));
    const input = screen.getByRole('textbox', { name: 'New name' });
    expect(input).toHaveValue('README.md');
    fireEvent.change(input, { target: { value: 'bad/name' } });
    expect(screen.getByRole('button', { name: 'Rename' })).toBeDisabled();
    fireEvent.change(input, { target: { value: 'NOTES.md' } });
    fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
    await waitFor(() => {
      expect(rowNames()).toEqual(['src', 'NOTES.md']);
    });
    expect(host.files.has(`${ROOT}\\NOTES.md`)).toBe(true);
  });

  it('deletes a folder after confirming, and cancels without deleting', async () => {
    const { host } = await mounted();
    fireEvent.contextMenu(row('src'), { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    expect(screen.getByRole('alertdialog', { name: 'Delete src?' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(host.files.has(`${ROOT}\\src\\main.ts`)).toBe(true);
    fireEvent.contextMenu(row('src'), { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => {
      expect(rowNames()).toEqual(['README.md']);
    });
    expect(host.files.has(`${ROOT}\\src\\main.ts`)).toBe(false);
  });

  it('reports a failed delete', async () => {
    const { host } = await mounted();
    host.remove = () => Promise.reject(new Error('Access is denied.'));
    fireEvent.contextMenu(row('README.md'), { clientX: 10, clientY: 10 });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Delete…' }));
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(await screen.findByText('Access is denied.')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });

  it('offers a remote tree no Explorer items and no rename', async () => {
    await mounted(false);
    fireEvent.contextMenu(row('README.md'), { clientX: 10, clientY: 10 });
    const titles = within(screen.getByRole('menu'))
      .getAllByRole('menuitem')
      .map((item) => item.textContent);
    expect(titles).toEqual(['Open', 'Copy Path', 'Copy Relative Path', 'Delete…']);
  });
});
