// Port of apps/purepoint-macos/purepoint-macosTests/WorkspaceFilesStateTests.swift (WIN-8.4). The
// Swift cases run real git in a temp repository; here the git calls are scripted to answer as git
// would for the same steps (the backend's git is tested in src-tauri), and the Files tree lists an
// in-memory host.
import { describe, expect, it } from 'vitest';
import type { FileDiff } from '../services/git';
import { memoryFileHost } from '../services/fileHost.fixtures';
import { WorkspaceFilesState, type WorkspaceFilesGit } from './workspaceFilesState';

const ROOT = 'C:\\repo';

function untracked(filename: string): FileDiff {
  return { filename, statusCode: '??', added: 1, removed: 0, hunks: [] } as unknown as FileDiff;
}

/** A working tree whose files the test edits: every untracked file is a change. */
function scriptedGit(tree: Map<string, string>): WorkspaceFilesGit {
  return {
    fetchWorkingTreeChanges: () => Promise.resolve([...tree.keys()].sort().map(untracked)),
    fetchWorkingTreeFileDiff: (_root, file) =>
      Promise.resolve({
        ...file,
        hunks: [
          {
            header: '@@ -0,0 +1 @@',
            lines: [
              {
                type: 'addition',
                content: (tree.get(file.filename) ?? '').trimEnd(),
                oldLineNo: null,
                newLineNo: 1,
              },
            ],
          },
        ],
      }),
  };
}

describe('WorkspaceFilesState', () => {
  it('given collapsed files, should load summaries and only expand requested diffs', async () => {
    const tree = new Map([
      ['one.txt', 'first\n'],
      ['two.txt', 'second\n'],
    ]);
    const state = new WorkspaceFilesState(ROOT, {
      git: scriptedGit(tree),
      fileHost: memoryFileHost({}),
    });
    await state.refresh();
    const s = () => state.store.getState();
    expect(s().files).toHaveLength(2);
    expect(Object.keys(s().diffs)).toEqual([]);
    const first = s().files[0];
    if (first === undefined) throw new Error('no file');
    state.toggleFile(first);
    await state.loadDiff(first);
    expect(Object.keys(s().diffs).sort()).toEqual(['one.txt']);
    expect([...s().expandedFiles]).toEqual(['one.txt']);

    tree.set('one.txt', 'updated\n');
    await state.refresh();
    expect(s().diffs['one.txt']?.hunks.flatMap((hunk) => hunk.lines)[0]?.content).toBe('updated');
    expect([...s().expandedFiles]).toEqual(['one.txt']);
    expect(s().diffs['two.txt']).toBeUndefined();

    tree.delete('one.txt');
    await state.refresh();
    expect(s().expandedFiles.size).toBe(0);
    expect(Object.keys(s().diffs)).toEqual([]);
  });

  it('given a non-repository, should show an error but still allow file browsing', async () => {
    const host = memoryFileHost({ [`${ROOT}\\readme.txt`]: 'hello\n' });
    const state = new WorkspaceFilesState(ROOT, {
      git: {
        fetchWorkingTreeChanges: () => Promise.reject(new Error('not a git repository')),
        fetchWorkingTreeFileDiff: (_root, file) => Promise.resolve(file),
      },
      fileHost: host,
    });
    await state.refresh();
    expect(state.store.getState().error).not.toBeNull();
    expect(state.store.getState().isLoading).toBe(false);
    state.store.setState({ mode: 'files' });
    await state.loadTreeIfNeeded();
    expect(state.fileTree.rootNodes.map((node) => node.name)).toEqual(['readme.txt']);
    state.fileTree.stopWatching();
  });

  // MARK: - Windows port: the Files half

  it('given the Files mode, should load the tree once and refresh it on every poll', async () => {
    const host = memoryFileHost({ [`${ROOT}\\a.txt`]: 'a' });
    const state = new WorkspaceFilesState(ROOT, { git: scriptedGit(new Map()), fileHost: host });
    state.setMode('files');
    await expect.poll(() => state.fileTree.rootNodes.map((node) => node.name)).toEqual(['a.txt']);
    host.files.set(`${ROOT}\\b.txt`, { content: 'b', modifiedMs: 1, binary: false });
    await state.refresh();
    expect(state.fileTree.rootNodes.map((node) => node.name)).toEqual(['a.txt', 'b.txt']);
  });

  it('given folders and previews, should toggle them by relative path', async () => {
    const host = memoryFileHost({ [`${ROOT}\\src\\main.ts`]: 'x' }, { folders: [ROOT] });
    const state = new WorkspaceFilesState(ROOT, { git: scriptedGit(new Map()), fileHost: host });
    await state.loadTreeIfNeeded();
    const src = state.fileTree.rootNodes[0];
    if (src === undefined) throw new Error('no src');
    await state.toggleFolder(src);
    expect([...state.store.getState().expandedFolders]).toEqual(['src']);
    expect(state.fileTree.rootNodes[0]?.children.map((node) => node.name)).toEqual(['main.ts']);
    state.togglePreview('src/main.ts');
    expect(state.store.getState().expandedPreviews.has('src/main.ts')).toBe(true);
    state.togglePreview('src/main.ts');
    expect(state.store.getState().expandedPreviews.size).toBe(0);
    await state.toggleFolder(src);
    expect(state.store.getState().expandedFolders.size).toBe(0);
    expect(state.fileTree.rootNodes[0]?.children).toEqual([]);
  });
});
