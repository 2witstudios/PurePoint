// The pane grid's views (WIN-7.4): PaneTabBar, the tab close confirmation, tab drag and drop, and
// DraggableSplit, rendered over a real WorkspaceRegistry with an in-memory layout file.
import {
  act,
  cleanup,
  createEvent,
  fireEvent,
  render,
  screen,
  within,
} from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  agentContent,
  fileContent,
  makeWorkspace,
  paneCount,
  type Surface,
  type Workspace,
} from '../../model/workspace.ts';
import { leaf, split } from '../../model/paneSplit.ts';
import { makeAgentModel } from '../../model/workspaceModel.ts';
import {
  ManualScheduler,
  MemoryWorkspacePersistence,
} from '../../state/workspaceRegistry.fixtures.ts';
import { WorkspaceRegistry } from '../../state/workspaceRegistry.ts';
import { AppStateContext } from '../shell/appContext.ts';
import { makeTestApp } from '../shell/testApp.ts';
import { DraggableSplit } from './DraggableSplit.tsx';
import { createPaneActions } from './paneCommands.ts';
import { PaneGridView } from './PaneGridView.tsx';
import {
  TabCloseConfirmationDialog,
  closeReasons,
  confirmTabClose,
  useCloseConfirmation,
  type CloseConfirmationSource,
} from './tabCloseConfirmation.tsx';
import {
  TAB_DRAG_TYPE,
  dropZoneAt,
  surfaceIdFromDragString,
  tabDragString,
  useTabDrag,
} from './tabDrag.ts';

// xterm.js needs a canvas; the grid's own behaviour is what these tests cover.
vi.mock('../terminal/TerminalPane.tsx', () => ({
  TerminalPane: ({ agentId }: { agentId: string }) => (
    <div data-testid="terminal-pane" data-agent-id={agentId} />
  ),
}));

const root = '/tmp/pp-pane-grid';

afterEach(() => {
  cleanup();
  useCloseConfirmation.getState().dismiss();
  useTabDrag.getState().end();
});

/** A registry holding one workspace (agents `ag-1` and `ag-2` stacked in pane 0, `ag-3` in pane 1). */
function setup(workspace?: Workspace) {
  const persistence = new MemoryWorkspacePersistence();
  const registry = new WorkspaceRegistry({ persistence, scheduler: new ManualScheduler() });
  const agents = ['ag-1', 'ag-2', 'ag-3'].map((id) =>
    makeAgentModel({
      id,
      name: id,
      agentType: 'claude',
      status: 'running',
      prompt: '',
      startedAt: '',
    }),
  );
  const layout =
    workspace ??
    makeWorkspace({
      id: 'ws',
      container: { kind: 'projectRoot' },
      root: split('vertical', 0.5, leaf(0), leaf(1)),
      panes: {
        0: {
          tabs: [
            { id: 0, content: agentContent('ag-1') },
            { id: 1, content: agentContent('ag-2') },
          ],
          activeTabId: 0,
        },
        1: { tabs: [{ id: 2, content: agentContent('ag-3') }], activeTabId: 2 },
      },
      focusedLeafId: 0,
      nextLeafId: 2,
      nextSurfaceId: 3,
    });
  const document = persistence.documentData([layout], null);
  if (document === null) throw new Error('no layout document');
  persistence.writeFile(persistence.filePath(root), document);
  registry.reconcile(root, agents, []);
  const app = makeTestApp();
  const view = render(
    <AppStateContext value={app}>
      <PaneGridView workspaceId={layout.id} registry={registry} />
      <TabCloseConfirmationDialog />
    </AppStateContext>,
  );
  const ws = () => {
    const current = registry.workspace(layout.id);
    if (current === null) throw new Error('workspace gone');
    return current;
  };
  return { registry, app, view, ws };
}

function bar(leafId: number): HTMLElement {
  const found = document.querySelector<HTMLElement>(
    `[data-testid="pane-tab-bar"][data-leaf-id="${String(leafId)}"]`,
  );
  if (found === null) throw new Error(`no tab bar for pane ${String(leafId)}`);
  return found;
}

function tab(surfaceId: number): HTMLElement {
  const found = document.querySelector<HTMLElement>(
    `[data-testid="pane-tab"][data-surface-id="${String(surfaceId)}"]`,
  );
  if (found === null) throw new Error(`no tab ${String(surfaceId)}`);
  return found;
}

/** A DataTransfer as Chromium gives handlers (jsdom has none). */
function dataTransfer() {
  const data = new Map<string, string>();
  return {
    get types() {
      return [...data.keys()];
    },
    setData: (type: string, value: string) => {
      data.set(type, value);
    },
    getData: (type: string) => data.get(type) ?? '',
    effectAllowed: 'all',
    dropEffect: 'none',
  };
}

/** A drag event at a point (jsdom has no DragEvent, so the coordinates are set by hand). */
function dragAt(
  type: 'dragOver' | 'drop',
  element: HTMLElement,
  transfer: ReturnType<typeof dataTransfer>,
  x: number,
  y: number,
): void {
  const event = createEvent[type](element, { dataTransfer: transfer });
  Object.defineProperty(event, 'clientX', { value: x });
  Object.defineProperty(event, 'clientY', { value: y });
  fireEvent(element, event);
}

function rect(width: number, height: number): DOMRect {
  return {
    x: 0,
    y: 0,
    left: 0,
    top: 0,
    width,
    height,
    right: width,
    bottom: height,
    toJSON: () => ({}),
  };
}

describe('PaneGridView', () => {
  it('renders the split tree with a cell, tab strip and divider per split', () => {
    setup();
    expect(screen.getAllByTestId('pane-cell')).toHaveLength(2);
    expect(screen.getAllByTestId('pane-tab-bar')).toHaveLength(2);
    expect(screen.getAllByTestId('pane-divider')).toHaveLength(1);
    expect(screen.getByTestId('pane-split').dataset.axis).toBe('vertical');
  });

  it('shows an empty tab placeholder, and focuses the pane it is clicked in', () => {
    const { registry, ws } = setup();
    act(() => {
      registry.newTab('ws', 1);
    });
    expect(screen.getByTestId('pane-empty-tab')).toHaveTextContent('Empty Tab');
    fireEvent.mouseDown(screen.getByTestId('pane-empty-tab'));
    expect(ws().focusedLeafId).toBe(1);
  });
});

describe('PaneTabBar', () => {
  it('always shows the strip: a tab per surface, with the close button on the active tab', () => {
    setup();
    const strip = bar(0);
    expect(within(strip).getAllByTestId('pane-tab')).toHaveLength(2);
    expect(tab(0).dataset.active).toBe('true');
    expect(within(tab(0)).getByTestId('pane-tab-close')).toBeInTheDocument();
    expect(within(tab(1)).queryByTestId('pane-tab-close')).toBeNull();
    fireEvent.mouseEnter(tab(1));
    expect(within(tab(1)).getByTestId('pane-tab-close')).toBeInTheDocument();
    // A single-tab pane still has its strip.
    expect(within(bar(1)).getAllByTestId('pane-tab')).toHaveLength(1);
  });

  it('names tabs as the Mac does', () => {
    const { registry } = setup();
    act(() => {
      registry.openFile('ws', 1, 'C:\\repo\\src\\main.ts');
      registry.newTab('ws', 1);
    });
    expect(within(bar(0)).getByText('ag-1')).toBeInTheDocument();
    expect(screen.getByText('main.ts')).toBeInTheDocument();
    expect(screen.getByText('New Tab')).toBeInTheDocument();
  });

  it('selects a clicked tab and closes a middle-clicked one', () => {
    const { ws } = setup();
    fireEvent.click(tab(1));
    expect(ws().panes[0]?.activeTabId).toBe(1);
    fireEvent(tab(0), new MouseEvent('auxclick', { bubbles: true, button: 1 }));
    expect(ws().panes[0]?.tabs.map((t) => t.id)).toEqual([1]);
  });

  it('opens a new tab, splits right and below, and closes the pane', () => {
    const { ws } = setup();
    fireEvent.click(within(bar(1)).getByRole('button', { name: 'New Tab' }));
    expect(ws().panes[1]?.tabs).toHaveLength(2);
    fireEvent.click(within(bar(1)).getByRole('button', { name: 'Split Right' }));
    expect(paneCount(ws())).toBe(3);
    expect(ws().focusedLeafId).toBe(2);
    fireEvent.click(within(bar(2)).getByRole('button', { name: 'Split Below' }));
    expect(paneCount(ws())).toBe(4);
    fireEvent.click(within(bar(3)).getByRole('button', { name: 'Close Pane' }));
    expect(paneCount(ws())).toBe(3);
  });

  it('hides Close Pane on a lone pane and the split buttons on a full grid', () => {
    const { registry, ws } = setup(
      makeWorkspace({
        id: 'one',
        container: { kind: 'projectRoot' },
        root: leaf(0),
        panes: { 0: { tabs: [{ id: 0, content: agentContent('ag-1') }], activeTabId: 0 } },
        focusedLeafId: 0,
        nextLeafId: 1,
        nextSurfaceId: 1,
      }),
    );
    expect(within(bar(0)).queryByRole('button', { name: 'Close Pane' })).toBeNull();
    act(() => {
      for (let i = 0; i < 5; i++) registry.split('one', ws().focusedLeafId, 'vertical');
    });
    expect(paneCount(ws())).toBe(6);
    expect(screen.queryByRole('button', { name: 'Split Right' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Split Below' })).toBeNull();
  });
});

describe('Tab close confirmation', () => {
  const source = (
    alive: string[],
    unsaved: Record<number, string> = {},
  ): CloseConfirmationSource => ({
    isAgentAlive: (id) => alive.includes(id),
    unsavedFileName: (_ws, surfaceId) => unsaved[surfaceId] ?? null,
  });
  const tabs: Surface[] = [
    { id: 0, content: agentContent('a') },
    { id: 1, content: agentContent('b') },
    { id: 2, content: fileContent('/x/notes.md') },
  ];

  it('asks only when closing loses more than one running agent or unsaved edits', () => {
    expect(closeReasons(tabs.slice(0, 1), 'ws', source(['a']))).toEqual([]);
    expect(closeReasons(tabs, 'ws', source(['a']))).toEqual([]);
    expect(closeReasons(tabs, 'ws', source(['a', 'b']))).toEqual([
      '2 running agents will be stopped.',
    ]);
    expect(closeReasons(tabs, 'ws', source([], { 2: 'notes.md' }))).toEqual([
      'Unsaved changes to notes.md will be lost.',
    ]);
  });

  it('closes at once when nothing is lost, else after the user agrees', () => {
    render(<TabCloseConfirmationDialog />);
    const close = vi.fn();
    confirmTabClose(tabs.slice(0, 1), 'ws', source(['a']), close);
    expect(close).toHaveBeenCalledOnce();

    act(() => {
      confirmTabClose(tabs, 'ws', source(['a', 'b']), close);
    });
    const dialog = screen.getByRole('alertdialog');
    expect(dialog).toHaveTextContent('Close this pane?');
    expect(dialog).toHaveTextContent('2 running agents will be stopped.');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    expect(close).toHaveBeenCalledOnce();

    act(() => {
      confirmTabClose(tabs.slice(2), 'ws', source([], { 2: 'notes.md' }), close);
    });
    expect(screen.getByRole('alertdialog')).toHaveTextContent('Close this tab?');
    fireEvent.click(screen.getByRole('button', { name: 'Close Tab' }));
    expect(close).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole('alertdialog')).toBeNull();
  });

  it('confirms Close Pane in the grid when the pane holds several running agents', () => {
    const { app, ws } = setup();
    vi.spyOn(app, 'agent').mockImplementation((id) =>
      makeAgentModel({
        id,
        name: id,
        agentType: 'claude',
        status: 'running',
        prompt: '',
        startedAt: '',
      }),
    );
    // The known agents' tabs hold their terminals.
    act(() => {
      app.store.setState({ projectRevision: app.store.getState().projectRevision + 1 });
    });
    expect(screen.getAllByTestId('terminal-pane').map((pane) => pane.dataset.agentId)).toEqual([
      'ag-1',
      'ag-3',
    ]);
    fireEvent.click(within(bar(0)).getByRole('button', { name: 'Close Pane' }));
    expect(screen.getByRole('alertdialog')).toHaveTextContent('2 running agents will be stopped.');
    expect(paneCount(ws())).toBe(2);
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Close Pane' }),
    );
    expect(paneCount(ws())).toBe(1);
  });

  it('runs Ctrl+W and the pane menu commands on the workspace on screen', () => {
    const { registry, ws } = setup();
    registry.activate('ws');
    const actions = createPaneActions(registry, {
      isAgentAlive: () => false,
      unsavedFileName: () => null,
    });
    act(() => {
      actions.splitBelow();
    });
    expect(paneCount(ws())).toBe(3);
    act(() => {
      actions.closePane();
      actions.focusLeft();
    });
    expect(paneCount(ws())).toBe(2);
    act(() => {
      actions.selectTab2();
    });
    expect(ws().panes[ws().focusedLeafId]?.activeTabId).toBe(1);
    act(() => {
      actions.closeAgent();
    });
    expect(ws().panes[0]?.tabs.map((t) => t.id)).toEqual([0]);
  });
});

describe('Tab drag and drop', () => {
  it('carries a prefixed tab ID and ignores other text', () => {
    expect(tabDragString(12)).toBe('purepoint-tab:12');
    expect(surfaceIdFromDragString('purepoint-tab:12')).toBe(12);
    expect(surfaceIdFromDragString('12')).toBeNull();
    expect(surfaceIdFromDragString('purepoint-tab:x')).toBeNull();
  });

  it('maps a point to the nearest edge band, else the centre', () => {
    expect(dropZoneAt(10, 50, 100, 100)).toBe('left');
    expect(dropZoneAt(95, 50, 100, 100)).toBe('right');
    expect(dropZoneAt(50, 5, 100, 100)).toBe('top');
    expect(dropZoneAt(50, 90, 100, 100)).toBe('bottom');
    expect(dropZoneAt(50, 50, 100, 100)).toBe('center');
    expect(dropZoneAt(5, 2, 100, 100)).toBe('top');
  });

  it('reorders a tab dropped on another tab of its pane', () => {
    const { ws } = setup();
    const transfer = dataTransfer();
    fireEvent.dragStart(tab(1), { dataTransfer: transfer });
    expect(transfer.types).toContain(TAB_DRAG_TYPE);
    fireEvent.dragOver(tab(0), { dataTransfer: transfer });
    fireEvent.drop(tab(0), { dataTransfer: transfer });
    expect(ws().panes[0]?.tabs.map((t) => t.id)).toEqual([1, 0]);
    expect(useTabDrag.getState().surfaceId).toBeNull();
  });

  it('moves a tab to another pane through its strip or its content', () => {
    const { ws } = setup();
    const transfer = dataTransfer();
    fireEvent.dragStart(tab(1), { dataTransfer: transfer });
    fireEvent.drop(within(bar(1)).getByRole('tablist'), { dataTransfer: transfer });
    expect(ws().panes[1]?.tabs.map((t) => t.id)).toEqual([2, 1]);

    fireEvent.dragStart(tab(1), { dataTransfer: transfer });
    const overlay = document.querySelector<HTMLElement>(
      '[data-testid="pane-drop-overlay"][data-leaf-id="0"]',
    );
    if (overlay === null) throw new Error('no drop overlay while dragging');
    overlay.getBoundingClientRect = () => rect(400, 300);
    dragAt('drop', overlay, transfer, 200, 150);
    expect(ws().panes[0]?.tabs.map((t) => t.id)).toEqual([0, 1]);
    expect(screen.queryByTestId('pane-drop-overlay')).toBeNull();
  });

  it('splits a pane when a tab is dropped on its edge', () => {
    const { ws } = setup();
    const transfer = dataTransfer();
    fireEvent.dragStart(tab(1), { dataTransfer: transfer });
    const overlay = document.querySelector<HTMLElement>(
      '[data-testid="pane-drop-overlay"][data-leaf-id="1"]',
    );
    if (overlay === null) throw new Error('no drop overlay while dragging');
    overlay.getBoundingClientRect = () => rect(400, 300);
    dragAt('dragOver', overlay, transfer, 200, 290);
    expect(overlay.dataset.zone).toBe('bottom');
    dragAt('drop', overlay, transfer, 200, 290);
    const after = ws();
    expect(paneCount(after)).toBe(3);
    expect(after.panes[after.focusedLeafId]?.tabs.map((t) => t.id)).toEqual([1]);
    expect(after.root).toEqual(
      split('vertical', 0.5, leaf(0), split('horizontal', 0.5, leaf(1), leaf(2))),
    );
  });
});

describe('DraggableSplit', () => {
  function renderSplit(onRatioChanged: (ratio: number) => void) {
    render(
      <DraggableSplit
        axis="vertical"
        ratio={0.5}
        onRatioChanged={onRatioChanged}
        first={<span>A</span>}
        second={<span>B</span>}
      />,
    );
    const container = screen.getByTestId('pane-split');
    container.getBoundingClientRect = () => rect(1000, 500);
    return screen.getByRole('separator');
  }

  it('commits the dragged ratio when the drag ends, keeping each side 120px or more', () => {
    const changed = vi.fn();
    const handle = renderSplit(changed);
    fireEvent.pointerDown(handle, { button: 0, clientX: 500, pointerId: 1 });
    fireEvent.pointerMove(handle, { clientX: 700, pointerId: 1 });
    expect(changed).not.toHaveBeenCalled();
    fireEvent.pointerUp(handle, { clientX: 700, pointerId: 1 });
    expect(changed).toHaveBeenCalledWith(0.7);
    fireEvent.pointerDown(handle, { button: 0, clientX: 500, pointerId: 1 });
    fireEvent.pointerUp(handle, { clientX: 990, pointerId: 1 });
    expect(changed).toHaveBeenLastCalledWith(0.88);
  });

  it('steps 5% with the arrow keys', () => {
    const changed = vi.fn();
    const handle = renderSplit(changed);
    expect(handle).toHaveAttribute('aria-valuenow', '50');
    fireEvent.keyDown(handle, { key: 'ArrowRight' });
    expect(changed).toHaveBeenCalledWith(0.55);
    fireEvent.keyDown(handle, { key: 'ArrowLeft' });
    expect(changed).toHaveBeenLastCalledWith(0.45);
  });
});
