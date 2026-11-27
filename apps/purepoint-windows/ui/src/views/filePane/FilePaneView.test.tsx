// WIN-8.5: the relevant cases of apps/purepoint-macos/purepoint-macosTests/FilePaneTests.swift (a
// file is a kind of tab: it sits in a pane's stack beside agent tabs and is persisted inside the
// leaf that holds it), then FilePaneView itself over an in-memory host: the tree, the editor with
// line numbers and highlighting, Ctrl+S, the dirty dot, and the conflict and external change
// banners.
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { EditorView } from '@codemirror/view';
import { describe, expect, it, vi } from 'vitest';
import { leaf, split } from '../../model/paneSplit';
import {
  agentContent,
  closeTab,
  EMPTY_CONTENT,
  fileContent,
  liveAgent,
  makeWorkspace,
  newTab,
  paneCount,
  reconcileWorkspaces,
  setSurfaceContent,
  surfaceById,
  workspaceAgentIds,
  type Workspace,
} from '../../model/workspace';
import { documentText, workspacesFromText } from '../../model/workspacePersistence';
import { memoryFileHost } from '../../services/fileHost.fixtures';
import { FileTabStore } from '../../state/fileTabStore';
import { EditorState } from '../../state/editorState';
import { SYNTAX_HIGHLIGHT_LANGUAGES } from '../../services/syntaxHighlight';
import { loadLanguage } from '../../services/codeHighlight';
import { stubScrollViewLayout } from '../../test/virtualLayout';
import { FilePaneView } from './FilePaneView';

stubScrollViewLayout('file-tree-outline', 240, 2000);

/** Pane 0 holds agent "a" (surface 0); pane 1 holds one empty tab (surface 1). */
function twoPaneWorkspace(): Workspace {
  return makeWorkspace({
    id: 'ws-a',
    container: { kind: 'projectRoot' },
    root: split('vertical', 0.5, leaf(0), leaf(1)),
    panes: {
      0: { tabs: [{ id: 0, content: agentContent('a') }], activeTabId: 0 },
      1: { tabs: [{ id: 1, content: EMPTY_CONTENT }], activeTabId: 1 },
    },
    focusedLeafId: 1,
    nextLeafId: 2,
    nextSurfaceId: 2,
  });
}

describe('FilePaneTests (Swift)', () => {
  it('given an empty tab, opening a file records the path', () => {
    const ws = setSurfaceContent(twoPaneWorkspace(), fileContent('C:\\tmp\\x.md'), 1);
    expect(surfaceById(ws, 1)?.content).toEqual(fileContent('C:\\tmp\\x.md'));
  });

  it('given an agent pane, a file opens as a new tab beside it', () => {
    const { workspace: ws, surfaceId } = newTab(twoPaneWorkspace(), 0, fileContent(null));
    expect(ws.panes[0]?.tabs.map((tab) => tab.content)).toEqual([
      agentContent('a'),
      fileContent(null),
    ]);
    expect(ws.panes[0]?.activeTabId).toBe(surfaceId);
    expect(workspaceAgentIds(ws)).toEqual(['a']);
  });

  it('given a file tab closed, its pane collapses', () => {
    const ws = setSurfaceContent(twoPaneWorkspace(), fileContent(null), 1);
    const { workspace: closed, agentIds } = closeTab(ws, 1);
    expect(agentIds).toEqual([]);
    expect(paneCount(closed)).toBe(1);
    expect(closed.panes[1]).toBeUndefined();
  });

  it('reconcile keeps a file tab next to a live agent', () => {
    const ws = setSurfaceContent(twoPaneWorkspace(), fileContent('C:\\tmp\\x.md'), 1);
    const result = reconcileWorkspaces([ws], [liveAgent('a', { kind: 'projectRoot' })]);
    expect(result).toHaveLength(1);
    expect(result[0] && surfaceById(result[0], 1)?.content).toEqual(fileContent('C:\\tmp\\x.md'));
  });

  it('persistence round-trips file tabs', () => {
    const ws = setSurfaceContent(twoPaneWorkspace(), fileContent('C:\\tmp\\x.md'), 1);
    const text = documentText([ws]);
    if (text === null) throw new Error('no document');
    const loaded = workspacesFromText(text, 'C:\\repo');
    expect(loaded[0] && surfaceById(loaded[0], 1)?.content).toEqual(fileContent('C:\\tmp\\x.md'));
  });
});

describe('syntax highlighting', () => {
  it('has a grammar for every language the Mac highlights', async () => {
    for (const language of SYNTAX_HIGHLIGHT_LANGUAGES) {
      expect(await loadLanguage(language), language).not.toBeNull();
    }
  });
});

const ROOT = 'C:\\repo';
const MAIN = `${ROOT}\\src\\main.ts`;

function mount(initialPath: string | null = MAIN) {
  const host = memoryFileHost(
    { [MAIN]: 'const a = 1;\nconst b = 2;\n', [`${ROOT}\\README.md`]: '# Title\n' },
    { folders: [ROOT] },
  );
  const store = new FileTabStore();
  const session = store.session('ws', 3, initialPath);
  const onPathChange = vi.fn();
  render(
    <FilePaneView
      rootPath={ROOT}
      host={host}
      initialPath={initialPath}
      session={session}
      isFocused={false}
      onPathChange={onPathChange}
    />,
  );
  const editor = () => {
    if (!(session.editor instanceof EditorState)) throw new Error('no editor');
    return session.editor;
  };
  return { host, session, editor, onPathChange };
}

function view(): EditorView {
  const content = document.querySelector('.cm-content');
  const found = content instanceof HTMLElement ? EditorView.findFromDOM(content) : null;
  if (found === null) throw new Error('no editor view');
  return found;
}

/** Type as the user would (a transaction CodeMirror reports as an edit). */
function type(text: string) {
  act(() => {
    const current = view();
    current.dispatch({ changes: { from: 0, insert: text }, userEvent: 'input.type' });
  });
}

function pressCtrlS() {
  const content = document.querySelector('.cm-content');
  if (content === null) throw new Error('no content');
  fireEvent.keyDown(content, { key: 's', code: 'KeyS', ctrlKey: true });
}

describe('FilePaneView', () => {
  it('opens its file in the editor with line numbers, with the tree hidden', async () => {
    mount();
    await waitFor(() => {
      expect(document.querySelector('.cm-content')?.textContent).toContain('const b = 2;');
    });
    expect(screen.getByTestId('file-pane-name')).toHaveTextContent('main.ts');
    expect(document.querySelector('.cm-lineNumbers')?.textContent).toContain('2');
    expect(screen.getByTestId('editor')).toHaveAttribute('data-language', 'typescript');
    expect(screen.queryByRole('tree')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Show file tree' }));
    expect(screen.getByRole('tree')).toBeInTheDocument();
  });

  it('starts with the tree when it has no file, and opens what the tree selects', async () => {
    const { onPathChange } = mount(null);
    expect(screen.getByText('Select a file')).toBeInTheDocument();
    fireEvent.click(await screen.findByRole('treeitem', { name: 'README.md' }));
    await waitFor(() => {
      expect(screen.getByTestId('file-pane-name')).toHaveTextContent('README.md');
    });
    expect(onPathChange).toHaveBeenCalledWith(`${ROOT}\\README.md`);
  });

  it('marks edits unsaved and saves them with Ctrl+S', async () => {
    const { host, session } = mount();
    await waitFor(() => {
      expect(document.querySelector('.cm-content')).not.toBeNull();
    });
    await waitFor(() => {
      expect(view().state.doc.toString()).toContain('const a');
    });
    type('// edited\n');
    expect(screen.getByTestId('file-pane-dirty')).toBeInTheDocument();
    expect(session.hasUnsavedChanges).toBe(true);
    pressCtrlS();
    await waitFor(() => {
      expect(host.files.get(MAIN)?.content).toBe('// edited\nconst a = 1;\nconst b = 2;\n');
    });
    await waitFor(() => {
      expect(screen.queryByTestId('file-pane-dirty')).toBeNull();
    });
  });

  it('shows the conflict banner when the file changed since it was opened', async () => {
    const { host } = mount();
    await waitFor(() => {
      expect(view().state.doc.toString()).toContain('const a');
    });
    // Changed on disk without a watcher event (e.g. the event is still debouncing).
    host.files.set(MAIN, { content: 'theirs\n', modifiedMs: 99_999, binary: false });
    type('mine ');
    pressCtrlS();
    expect(await screen.findByTestId('editor-save-conflict')).toHaveTextContent(
      'main.ts changed on disk since it was opened.',
    );
    fireEvent.click(screen.getByRole('button', { name: 'Overwrite' }));
    await waitFor(() => {
      expect(host.files.get(MAIN)?.content).toBe('mine const a = 1;\nconst b = 2;\n');
    });
    expect(screen.queryByTestId('editor-save-conflict')).toBeNull();
  });

  it('reloads a clean file changed on disk, and warns about a dirty one', async () => {
    const { host } = mount();
    await waitFor(() => {
      expect(view().state.doc.toString()).toContain('const a');
    });
    act(() => {
      host.writeExternally(MAIN, 'outside 1\n');
    });
    await waitFor(() => {
      expect(view().state.doc.toString()).toBe('outside 1\n');
    });
    expect(screen.queryByTestId('file-pane-dirty')).toBeNull();
    type('mine ');
    act(() => {
      host.writeExternally(MAIN, 'outside 2\n');
    });
    expect(await screen.findByTestId('editor-external-change')).toHaveTextContent(
      'File changed on disk.',
    );
    expect(view().state.doc.toString()).toBe('mine outside 1\n');
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }));
    await waitFor(() => {
      expect(view().state.doc.toString()).toBe('outside 2\n');
    });
    expect(screen.queryByTestId('editor-external-change')).toBeNull();
  });

  it('shows a binary file as a placeholder', async () => {
    const host = memoryFileHost({ [`${ROOT}\\a.bin`]: 'x\0y' });
    const session = new FileTabStore().session('ws', 1, `${ROOT}\\a.bin`);
    render(
      <FilePaneView
        rootPath={ROOT}
        host={host}
        initialPath={`${ROOT}\\a.bin`}
        session={session}
        isFocused={false}
        onPathChange={() => undefined}
      />,
    );
    expect(await screen.findByText("Binary file — can't be shown")).toBeInTheDocument();
  });
});
