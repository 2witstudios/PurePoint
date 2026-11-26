// Port of apps/purepoint-macos/purepoint-macosTests/FileTreeStateTests.swift (WIN-8.4), over an
// in-memory host (the backend's listing and `git check-ignore` are tested in
// src-tauri/src/files/tree.rs).
import { describe, expect, it } from 'vitest';
import { memoryFileHost } from '../services/fileHost.fixtures';
import { FileTreeState } from './fileTreeState';

const ROOT = 'C:\\repo';

function tree(files: string[], dirs: string[] = []) {
  const host = memoryFileHost(Object.fromEntries(files.map((name) => [`${ROOT}\\${name}`, ''])), {
    folders: [ROOT, ...dirs.map((dir) => `${ROOT}\\${dir}`)],
  });
  return { host, state: new FileTreeState() };
}

const names = (state: FileTreeState) => state.rootNodes.map((node) => node.name);
const child = (state: FileTreeState, name: string) => {
  const node = state.rootNodes.find((candidate) => candidate.name === name);
  if (node === undefined) throw new Error(`no ${name}`);
  return node;
};

describe('FileTreeState', () => {
  // MARK: - Dot file visibility

  it('given a dot file, should include it in the tree', async () => {
    const { host, state } = tree(['.gitignore', '.env.example', 'README.md']);
    await state.load(ROOT, host);
    expect(names(state)).toEqual(
      expect.arrayContaining(['.gitignore', '.env.example', 'README.md']),
    );
  });

  it('given a dot directory, should include it in the tree', async () => {
    const { host, state } = tree([], ['.claude', '.pu']);
    await state.load(ROOT, host);
    expect(names(state)).toEqual(expect.arrayContaining(['.claude', '.pu']));
  });

  // MARK: - Hidden names still excluded

  it('given a .git directory, should exclude it from the tree', async () => {
    const { host, state } = tree(['.DS_Store'], ['.git', '.build']);
    await state.load(ROOT, host);
    expect(names(state)).not.toContain('.git');
    expect(names(state)).not.toContain('.DS_Store');
    expect(names(state)).not.toContain('.build');
  });

  it('given non-dot hidden names, should exclude them from the tree', async () => {
    const { host, state } = tree([], ['node_modules', 'DerivedData', 'xcuserdata', '__pycache__']);
    await state.load(ROOT, host);
    expect(names(state)).toEqual([]);
  });

  // MARK: - Mixed content

  it('given mixed content, should show dot files but not hidden names', async () => {
    const { host, state } = tree(
      ['.gitignore', '.env', '.DS_Store', 'main.swift'],
      ['.claude', '.git', 'Sources'],
    );
    await state.load(ROOT, host);
    const shown = new Set(names(state));
    for (const name of ['.gitignore', '.env', '.claude', 'main.swift', 'Sources']) {
      expect(shown.has(name)).toBe(true);
    }
    expect(shown.has('.git')).toBe(false);
    expect(shown.has('.DS_Store')).toBe(false);
  });

  it('given ignored files, should keep them filtered across polling and rule changes', async () => {
    const { host, state } = tree(['.gitignore', 'root.ignore', 'keep.txt'], ['Sources']);
    host.files.set(`${ROOT}\\Sources\\child.ignore`, { content: '', modifiedMs: 1, binary: false });
    host.files.set(`${ROOT}\\Sources\\keep.swift`, { content: '', modifiedMs: 1, binary: false });
    host.ignored = (relative) => relative.endsWith('.ignore');
    await state.load(ROOT, host);
    expect(names(state)).not.toContain('root.ignore');
    await state.expandNode(child(state, 'Sources'));
    expect(child(state, 'Sources').children.map((node) => node.name)).toEqual(['keep.swift']);

    // Every published tree is already filtered, including while a refresh is in flight.
    const seen: string[][] = [];
    const unsubscribe = state.store.subscribe((s) => {
      seen.push(s.rootNodes.flatMap((node) => [node.name, ...node.children.map((c) => c.name)]));
    });
    for (let iteration = 0; iteration < 3; iteration++) {
      host.files.set(`${ROOT}\\poll-${String(iteration)}.txt`, {
        content: 'new',
        modifiedMs: 1,
        binary: false,
      });
      const refreshed = state.refresh();
      expect(names(state)).not.toContain('root.ignore');
      await refreshed;
      expect(names(state)).toContain(`poll-${String(iteration)}.txt`);
      expect(names(state)).not.toContain('root.ignore');
      expect(child(state, 'Sources').children.map((node) => node.name)).not.toContain(
        'child.ignore',
      );
    }
    expect(seen.flat().some((name) => name.endsWith('.ignore'))).toBe(false);
    unsubscribe();

    // The rules change (an emptied .gitignore): the next refresh shows them.
    host.ignored = () => false;
    await state.refresh();
    expect(names(state)).toContain('root.ignore');
    expect(child(state, 'Sources').children.map((node) => node.name)).toContain('child.ignore');
  });

  // MARK: - Windows port: ordering, lazy listing, watching

  it('given a listing, should sort folders first then names case-insensitively', async () => {
    const { host, state } = tree(['b.txt', 'A.txt', 'c.txt'], ['zeta', 'Alpha']);
    await state.load(ROOT, host);
    expect(names(state)).toEqual(['Alpha', 'zeta', 'A.txt', 'b.txt', 'c.txt']);
    expect(child(state, 'zeta').relativePath).toBe('zeta');
  });

  it('given a collapsed folder, should list it only when expanded', async () => {
    const { host, state } = tree(['a.txt'], ['src', 'src\\nested']);
    host.files.set(`${ROOT}\\src\\main.ts`, { content: '', modifiedMs: 1, binary: false });
    await state.load(ROOT, host);
    expect(host.listed).toEqual([ROOT]);
    expect(child(state, 'src').children).toEqual([]);
    await state.expandNode(child(state, 'src'));
    expect(child(state, 'src').children.map((node) => node.relativePath)).toEqual([
      'src/nested',
      'src/main.ts',
    ]);
    expect(state.store.getState().expandedPaths.has(`${ROOT}\\src`)).toBe(true);
    await state.collapseNode(child(state, 'src'));
    expect(child(state, 'src').children).toEqual([]);
  });

  it('given a watched root, should refresh on a change and stop when told to', async () => {
    const { host, state } = tree(['a.txt']);
    await state.load(ROOT, host);
    host.writeExternally(`${ROOT}\\b.txt`, 'b');
    await expect.poll(() => names(state)).toEqual(['a.txt', 'b.txt']);
    state.stopWatching();
    host.writeExternally(`${ROOT}\\c.txt`, 'c');
    await Promise.resolve();
    expect(names(state)).toEqual(['a.txt', 'b.txt']);
  });

  it('given a slower refresh superseded by a newer one, should keep the newer tree', async () => {
    const { host, state } = tree(['a.txt']);
    await state.load(ROOT, host);
    let release: () => void = () => undefined;
    host.gate = new Promise((resolve) => {
      release = resolve;
    });
    const slow = state.refresh();
    host.gate = null;
    host.files.set(`${ROOT}\\new.txt`, { content: '', modifiedMs: 1, binary: false });
    await state.refresh();
    expect(names(state)).toContain('new.txt');
    host.files.delete(`${ROOT}\\new.txt`);
    release();
    await slow;
    expect(names(state)).toContain('new.txt');
  });

  it('given a new root, should drop the old tree and its expansions', async () => {
    const { host, state } = tree(['a.txt'], ['src']);
    host.files.set('C:\\other\\b.txt', { content: '', modifiedMs: 1, binary: false });
    await state.load(ROOT, host);
    await state.expandNode(child(state, 'src'));
    await state.load('C:\\other', host);
    expect(names(state)).toEqual(['b.txt']);
    expect(state.store.getState().expandedPaths.size).toBe(0);
  });

  it('given an unreadable root, should show an empty tree', async () => {
    const { host, state } = tree([]);
    await state.load('C:\\missing', host);
    expect(state.rootNodes).toEqual([]);
  });
});
