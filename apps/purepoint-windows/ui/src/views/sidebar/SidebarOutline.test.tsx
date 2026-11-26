// Port of apps/purepoint-macos/purepoint-macosTests/SidebarOutlineViewControllerTests.swift
// (WIN-7.2), platform-neutral cases, against the rendered outline; then the Windows outline's own
// behaviour (selection, keyboard, rename, delete, windowing, the footer and the shell).
//
// Swift drives `NSOutlineView.collapseItem`/`expandItem`; here a row is closed or opened through
// its disclosure button, as the user would. `controller.projectNodes` reads become the rows on
// screen, and `isItemExpanded` reads `aria-expanded`.

import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { makeAgent, makeMachine, makeProject, MemoryStorage } from '../../model/sidebarFixtures.ts';
import {
  type ProjectSnapshot,
  type SidebarSelection,
  machineKey,
  projectNode,
  rowKey,
  selection as sidebarSelection,
} from '../../model/sidebarItem.ts';
import type { WorkspacesByProject } from '../../model/sidebarOutline.ts';
import {
  adoptingWorkspace,
  agentContent,
  splitPane,
  worktreeContainer,
} from '../../model/workspace.ts';
import { makeWorktreeModel } from '../../model/workspaceModel.ts';
import { createSidebarExpansionStore } from '../../state/sidebarExpansionStore.ts';
import { AppStateContext } from '../shell/appContext.ts';
import { ContentView } from '../shell/ContentView.tsx';
import { makeTestApp } from '../shell/testApp.ts';
import { SidebarFooter } from './SidebarFooter.tsx';
import {
  SIDEBAR_ROW_HEIGHT,
  SidebarOutlineViewController,
  type SidebarOutlineViewControllerProps,
  sidebarKeyCommand,
  visibleRows,
} from './SidebarOutlineViewController.tsx';

// MARK: - Helpers

let rootCounter = 0;
function uniqueRoot(name: string): string {
  rootCounter += 1;
  return `/tmp/sidebar-${name}-${String(rootCounter)}`;
}

/** Swift `makeSidebarProject(root:worktreeCount:)`: a local project with `wt-<i>` worktrees. */
function makeSidebarProject(
  root: string,
  worktreeCount: number,
  extra: Partial<ProjectSnapshot> = {},
): ProjectSnapshot {
  return { ...makeProject(root, { worktreeCount, machineId: null }), ...extra };
}

/** Swift `makeSidebarWorktree(_:branch:)`. */
function makeSidebarWorktree(id: string, branch = `branch-${id}`) {
  return makeWorktreeModel({
    id,
    name: id,
    path: `/tmp/wt-${id}`,
    branch,
    status: 'active',
    agents: [makeAgent(`agent-${id}`)],
  });
}

function projectKey(project: ProjectSnapshot): string {
  return rowKey('project', project.projectId);
}

function worktreeKey(project: ProjectSnapshot, worktreeId: string): string {
  return rowKey('worktree', worktreeId, projectKey(project));
}

function rows(): HTMLElement[] {
  return screen.queryAllByRole('treeitem');
}

function rowFor(key: string): HTMLElement | null {
  return rows().find((row) => row.getAttribute('data-row-key') === key) ?? null;
}

function requireRow(key: string): HTMLElement {
  const row = rowFor(key);
  if (row === null) throw new Error(`no row ${key}`);
  return row;
}

function isExpanded(key: string): boolean {
  return requireRow(key).getAttribute('aria-expanded') === 'true';
}

/** Close or open a row through its disclosure button. */
function toggle(key: string): void {
  const row = requireRow(key);
  fireEvent.click(within(row).getByRole('button', { name: /^(Collapse|Expand)$/ }));
}

function tree(): HTMLElement {
  return screen.getByRole('tree');
}

function renderOutline(props: SidebarOutlineViewControllerProps) {
  const view = render(<SidebarOutlineViewController {...props} />);
  return {
    ...view,
    update: (next: Partial<SidebarOutlineViewControllerProps>) => {
      props = { ...props, ...next };
      view.rerender(<SidebarOutlineViewController {...props} />);
    },
  };
}

function memoryStore(storage = new MemoryStorage()) {
  return { storage, store: createSidebarExpansionStore(storage) };
}

// MARK: - SidebarOutlineViewControllerTests

describe('SidebarOutlineViewControllerTests', () => {
  it('givenEmptyProjectShouldHaveNoSidebarChildren', () => {
    const { store } = memoryStore();
    renderOutline({ projects: [makeSidebarProject('/tmp/empty', 0)], expansionStore: store });

    expect(rows()).toHaveLength(1);
    expect(rows()[0]).toHaveAttribute('data-kind', 'project');
  });

  it('givenProjectWithWorktreesShouldOnlyShowWorktreeChildren', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject('/tmp/project', 2);
    renderOutline({ projects: [project], expansionStore: store });

    const children = rows().filter((row) => row.getAttribute('aria-level') === '2');
    expect(children.map((row) => row.getAttribute('data-row-key'))).toEqual([
      worktreeKey(project, 'wt-0'),
      worktreeKey(project, 'wt-1'),
    ]);
    expect(children.every((row) => row.getAttribute('data-kind') === 'worktree')).toBe(true);
  });

  it('givenUnreadChannelChangesShouldRefreshProjectBadgeWithoutChildRows', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject(uniqueRoot('unread'), 0);
    const view = renderOutline({ projects: [project], expansionStore: store });
    expect(screen.queryByTestId('channelUnreadCount')).toBeNull();

    view.update({ projects: [{ ...project, channelUnread: 1 }] });
    expect(rows()).toHaveLength(1);
    const badge = screen.getByTestId('channelUnreadCount');
    expect(badge).toHaveTextContent('1');
    expect(badge).toHaveAttribute('aria-label', '1 unread channel messages');

    view.update({ projects: [{ ...project, channelUnread: 0 }] });
    expect(screen.queryByTestId('channelUnreadCount')).toBeNull();
  });

  it('givenScrolledSidebarShouldPreserveScrollPositionAcrossUnchangedRebuild', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject('/tmp/project', 30);
    const view = renderOutline({ projects: [project], expansionStore: store });
    const outline = tree();

    outline.scrollTop = 180;
    fireEvent.scroll(outline);
    const startingOffset = outline.scrollTop;
    expect(startingOffset).toBeGreaterThan(0);

    // The same rows from fresh snapshots: an unchanged rebuild.
    view.update({ projects: [makeSidebarProject('/tmp/project', 30)] });

    expect(tree()).toBe(outline);
    expect(tree().scrollTop).toBe(startingOffset);
  });

  // MARK: - Row expansion

  it('givenARowTheUserClosedShouldStayClosedWhenAnUnrelatedRefreshRebuildsTheTree', () => {
    const { store } = memoryStore();
    const closed = makeSidebarProject(uniqueRoot('closed'), 2);
    const unrelated = makeSidebarProject(uniqueRoot('unrelated'), 1);
    const view = renderOutline({ projects: [closed, unrelated], expansionStore: store });
    expect(isExpanded(projectKey(closed))).toBe(true);

    toggle(projectKey(closed));

    // An unrelated refresh: another project's worktree is renamed.
    view.update({
      projects: [closed, { ...unrelated, worktrees: [makeSidebarWorktree('wt-0', 'renamed')] }],
    });

    expect(isExpanded(projectKey(closed))).toBe(false);
    expect(isExpanded(projectKey(unrelated))).toBe(true);
  });

  it('givenARowTheUserClosedShouldKeepItsStateWhenItsParentClosesAndReopens', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject(uniqueRoot('parent'), 2);
    const view = renderOutline({ projects: [project], expansionStore: store });

    toggle(worktreeKey(project, 'wt-0'));
    toggle(projectKey(project));

    // Closing the project must not discard what its rows were doing.
    view.update({
      projects: [
        {
          ...project,
          worktrees: [makeSidebarWorktree('wt-0', 'renamed'), makeSidebarWorktree('wt-1')],
        },
      ],
    });
    expect(isExpanded(projectKey(project))).toBe(false);

    toggle(projectKey(project));

    expect(isExpanded(worktreeKey(project, 'wt-0'))).toBe(false);
    expect(isExpanded(worktreeKey(project, 'wt-1'))).toBe(true);
  });

  it('givenANewRowShouldAppearExpandedBesideTheRowsTheUserLeftAlone', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject(uniqueRoot('new-row'), 1);
    const view = renderOutline({ projects: [project], expansionStore: store });
    toggle(worktreeKey(project, 'wt-0'));

    view.update({
      projects: [
        { ...project, worktrees: [...project.worktrees, makeSidebarWorktree('wt-added')] },
      ],
    });

    expect(isExpanded(worktreeKey(project, 'wt-0'))).toBe(false);
    expect(isExpanded(worktreeKey(project, 'wt-added'))).toBe(true);
  });

  it('givenASelectionInsideAClosedRowShouldOpenItsAncestorsAndSelectTheRow', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject(uniqueRoot('select'), 1);
    const workspace = adoptingWorkspace('ag-1', worktreeContainer('wt-0'));
    const workspacesByProject: WorkspacesByProject = { [project.projectId]: [workspace] };
    const view = renderOutline({ projects: [project], workspacesByProject, expansionStore: store });
    toggle(projectKey(project));
    expect(rowFor(rowKey('workspace', workspace.id, worktreeKey(project, 'wt-0')))).toBeNull();

    view.update({ selection: sidebarSelection.workspace(workspace.id) });

    const workspaceRow = requireRow(
      rowKey('workspace', workspace.id, worktreeKey(project, 'wt-0')),
    );
    expect(workspaceRow).toHaveAttribute('aria-selected', 'true');
    expect(tree()).toHaveAttribute('aria-activedescendant', workspaceRow.id);
  });

  it('givenAProjectThatHasNotReadItsSnapshotShouldKeepWhatItRecorded', () => {
    const storage = new MemoryStorage();
    const root = uniqueRoot('unread-snapshot');
    const worktreeRowKey = rowKey('worktree', 'wt-0', rowKey('project', root));
    // A previous launch closed a worktree row under this project.
    createSidebarExpansionStore(storage).getState().setExpanded(worktreeRowKey, false);
    const store = createSidebarExpansionStore(storage);

    // The relaunch order: the project row is here, but its snapshot has not been read, so its
    // worktrees are not in the tree yet.
    const view = renderOutline({ projects: [makeSidebarProject(root, 0)], expansionStore: store });

    // The snapshot lands, and the row the user closed arrives with it. It must still be closed.
    view.update({ projects: [makeSidebarProject(root, 1, { hasLoadedWorktrees: true })] });

    expect([...store.getState().closedRowKeys]).toEqual([worktreeRowKey]);
    expect(isExpanded(worktreeRowKey)).toBe(false);
  });

  it('givenAProjectThatFinishedLoadingItsRowsUnchangedShouldStillPrune', () => {
    const storage = new MemoryStorage();
    const root = uniqueRoot('unchanged');
    const goneKey = rowKey('worktree', 'wt-gone', rowKey('project', root));
    createSidebarExpansionStore(storage).getState().setExpanded(goneKey, false);
    const store = createSidebarExpansionStore(storage);

    // A project with no worktrees at all: reading its snapshot changes no row, so only the
    // readiness flag differs. The prune still has to happen.
    const view = renderOutline({ projects: [makeSidebarProject(root, 0)], expansionStore: store });
    view.update({ projects: [makeSidebarProject(root, 0, { hasLoadedWorktrees: true })] });

    expect(store.getState().closedRowKeys.size).toBe(0);
  });

  it('givenASettledTreeShouldDropRowsThatAreGone', () => {
    const storage = new MemoryStorage();
    const root = uniqueRoot('read');
    const goneKey = rowKey('worktree', 'wt-gone', rowKey('project', root));
    createSidebarExpansionStore(storage).getState().setExpanded(goneKey, false);
    const store = createSidebarExpansionStore(storage);

    // Settled, and the row it recorded is not in the tree: it must not come back closed if that
    // id is ever reused.
    renderOutline({
      projects: [makeSidebarProject(root, 1, { hasLoadedWorktrees: true })],
      expansionStore: store,
    });

    expect(store.getState().closedRowKeys.size).toBe(0);
  });

  it('givenARowClosedBeforeALaunchShouldStayClosedOnTheFirstBuild', () => {
    const storage = new MemoryStorage();
    const root = uniqueRoot('launch');
    const first = renderOutline({
      projects: [makeSidebarProject(root, 1)],
      expansionStore: createSidebarExpansionStore(storage),
    });
    const project = makeSidebarProject(root, 1);
    toggle(projectKey(project));
    first.unmount();

    // The next launch reads what the last one recorded.
    renderOutline({
      projects: [makeSidebarProject(root, 1)],
      expansionStore: createSidebarExpansionStore(storage),
    });

    expect(isExpanded(projectKey(project))).toBe(false);
  });

  it('givenAProjectRootThatSpellsOutAnotherRowsKeyShouldNotShareIt', () => {
    // A folder literally named after a worktree row under another project: without an escape
    // both rows would answer to the same key, so closing one would close both.
    const worktreeRow = rowKey('worktree', 'wt-abc12345', rowKey('project', '/repo'));
    const lookalike = projectNode(makeSidebarProject('/repo/worktree:wt-abc12345', 0), []).rowKey;
    expect(lookalike).not.toBe(worktreeRow);
  });

  it('givenTheSameRootOnTwoMachinesShouldGiveEveryRowItsOwnKey', () => {
    const project = makeSidebarProject('/srv/app', 1);
    const keys = (node: ReturnType<typeof projectNode>): Set<string> => {
      const all = new Set([node.rowKey]);
      for (const child of node.children) for (const key of keys(child)) all.add(key);
      return all;
    };

    // Two machines hosting a project at the same root: the same rows, two rows each.
    const first = projectNode(project, [], machineKey('11111111-1111-4111-8111-111111111111'));
    const second = projectNode(project, [], machineKey('22222222-2222-4222-8222-222222222222'));

    const firstKeys = keys(first);
    expect([...keys(second)].some((key) => firstKeys.has(key))).toBe(false);
    expect(firstKeys.size).toBe(2);
  });
});

// MARK: - The Windows outline

describe('SidebarOutlineViewController', () => {
  function fixture() {
    const project = makeSidebarProject(uniqueRoot('fixture'), 2);
    const rootWorkspace = adoptingWorkspace('root-agent', { kind: 'projectRoot' });
    const worktreeWorkspace = adoptingWorkspace('agent-wt-0', worktreeContainer('wt-0'));
    const withRootAgent: ProjectSnapshot = {
      ...project,
      rootAgents: [makeAgent('root-agent')],
      worktrees: [makeSidebarWorktree('wt-0', 'branch-0'), makeSidebarWorktree('wt-1', 'branch-1')],
    };
    const workspacesByProject: WorkspacesByProject = {
      [project.projectId]: [rootWorkspace, worktreeWorkspace],
    };
    return { project: withRootAgent, rootWorkspace, worktreeWorkspace, workspacesByProject };
  }

  it('draws compact rows with the DEC-13 icons, status dots, pills and badges', () => {
    const { store } = memoryStore();
    const { project, rootWorkspace, workspacesByProject } = fixture();
    const machine = makeMachine('studio', {
      state: { kind: 'connected', daemon: { kind: 'running' } },
      statusText: 'Connected',
    });
    renderOutline({
      projects: [project],
      machines: [machine],
      workspacesByProject,
      expansionStore: store,
    });

    const projectRow = requireRow(projectKey(project));
    expect(projectRow.style.height).toBe(`${String(SIDEBAR_ROW_HEIGHT)}px`);
    expect(projectRow.querySelector('[data-symbol="folder.fill"]')).not.toBeNull();
    expect(within(projectRow).getByRole('button', { name: 'Add' })).toBeInTheDocument();

    const worktreeRow = requireRow(worktreeKey(project, 'wt-0'));
    expect(worktreeRow.querySelector('[data-symbol="arrow.triangle.branch"]')).not.toBeNull();
    expect(within(worktreeRow).getByLabelText('1 agents')).toHaveTextContent('1');

    const workspaceRow = requireRow(rowKey('workspace', rootWorkspace.id, projectKey(project)));
    expect(workspaceRow).toHaveAttribute('aria-label', 'root-agent, Unknown');
    expect(workspaceRow.querySelector('.sidebar-status-dot')).toHaveAttribute(
      'data-color',
      'green',
    );
    expect(within(workspaceRow).getByText('Unknown')).toHaveClass('sidebar-pill');

    expect(screen.getByText('MACHINES')).toBeInTheDocument();
    const machineRow = requireRow(machineKey(machine.id));
    expect(machineRow.querySelector('[data-symbol="desktopcomputer"]')).not.toBeNull();
    expect(machineRow.querySelector('.sidebar-status-dot')).toHaveAttribute('data-color', 'green');
    expect(within(machineRow).getByText('Connected')).toBeInTheDocument();
  });

  it('counts panes and tabs on a workspace row', () => {
    const { store } = memoryStore();
    const { project, workspacesByProject, rootWorkspace } = fixture();
    const split = splitPane(rootWorkspace, 0, 'horizontal', agentContent('agent-wt-1')).workspace;
    renderOutline({
      projects: [project],
      workspacesByProject: { ...workspacesByProject, [project.projectId]: [split] },
      expansionStore: store,
    });
    const row = requireRow(rowKey('workspace', split.id, projectKey(project)));
    expect(within(row).getByLabelText('Panes').parentElement).toHaveTextContent('2');
    expect(row.getAttribute('aria-label')).toContain(', 2 panes');
  });

  it('reports the selection of a project, a worktree and a workspace, and none for a machine', () => {
    const { store } = memoryStore();
    const { project, worktreeWorkspace, workspacesByProject } = fixture();
    const machine = makeMachine('studio');
    const onSelectionChanged = vi.fn();
    const view = renderOutline({
      projects: [project],
      machines: [machine],
      workspacesByProject,
      expansionStore: store,
      onSelectionChanged,
    });

    fireEvent.mouseDown(requireRow(projectKey(project)), { button: 0 });
    expect(onSelectionChanged).toHaveBeenLastCalledWith(
      sidebarSelection.project(project.projectId),
    );
    fireEvent.mouseDown(requireRow(worktreeKey(project, 'wt-0')), { button: 0 });
    expect(onSelectionChanged).toHaveBeenLastCalledWith(sidebarSelection.worktree('wt-0'));
    const workspaceKey = rowKey('workspace', worktreeWorkspace.id, worktreeKey(project, 'wt-0'));
    fireEvent.mouseDown(requireRow(workspaceKey), { button: 0 });
    expect(onSelectionChanged).toHaveBeenLastCalledWith(
      sidebarSelection.workspace(worktreeWorkspace.id),
    );

    // A machine row is selectable but addresses nothing in the detail.
    view.update({ selection: sidebarSelection.workspace(worktreeWorkspace.id) });
    onSelectionChanged.mockClear();
    fireEvent.mouseDown(requireRow(machineKey(machine.id)), { button: 0 });
    expect(onSelectionChanged).not.toHaveBeenCalled();
    expect(requireRow(machineKey(machine.id))).toHaveAttribute('aria-selected', 'true');

    // The section title cannot be selected.
    const header = rows().find((row) => row.getAttribute('data-kind') === 'sectionHeader');
    expect(header).toBeDefined();
    expect(header).not.toHaveAttribute('aria-selected');

    // A nav selection clears the outline's.
    view.update({ selection: sidebarSelection.nav('agents') });
    expect(rows().some((row) => row.getAttribute('aria-selected') === 'true')).toBe(false);
  });

  it('moves with the arrows, closes and opens with Left and Right', () => {
    const { store } = memoryStore();
    const { project, workspacesByProject } = fixture();
    let selection: SidebarSelection | null = null;
    const view = renderOutline({ projects: [project], workspacesByProject, expansionStore: store });
    const onSelectionChanged = (next: SidebarSelection | null) => {
      selection = next;
      view.update({ selection: next });
    };
    view.update({ onSelectionChanged });

    fireEvent.keyDown(tree(), { key: 'ArrowDown' });
    expect(selection).toEqual(sidebarSelection.project(project.projectId));
    fireEvent.keyDown(tree(), { key: 'ArrowDown' });
    fireEvent.keyDown(tree(), { key: 'ArrowDown' });
    expect(selection).toEqual(sidebarSelection.worktree('wt-0'));

    // Left closes the open worktree, then moves to its parent.
    fireEvent.keyDown(tree(), { key: 'ArrowLeft' });
    expect(isExpanded(worktreeKey(project, 'wt-0'))).toBe(false);
    fireEvent.keyDown(tree(), { key: 'ArrowLeft' });
    expect(selection).toEqual(sidebarSelection.project(project.projectId));

    // Right opens a closed row.
    fireEvent.keyDown(tree(), { key: 'ArrowDown' });
    fireEvent.keyDown(tree(), { key: 'ArrowDown' });
    fireEvent.keyDown(tree(), { key: 'ArrowRight' });
    expect(isExpanded(worktreeKey(project, 'wt-0'))).toBe(true);

    fireEvent.keyDown(tree(), { key: 'ArrowUp' });
    fireEvent.keyDown(tree(), { key: 'ArrowUp' });
    expect(selection).toEqual(sidebarSelection.project(project.projectId));
  });

  it('selects a closed row when a row that held the selection closes', () => {
    const { store } = memoryStore();
    const { project, worktreeWorkspace, workspacesByProject } = fixture();
    const onSelectionChanged = vi.fn();
    renderOutline({
      projects: [project],
      workspacesByProject,
      selection: sidebarSelection.workspace(worktreeWorkspace.id),
      expansionStore: store,
      onSelectionChanged,
    });
    toggle(worktreeKey(project, 'wt-0'));
    expect(onSelectionChanged).toHaveBeenLastCalledWith(sidebarSelection.worktree('wt-0'));
  });

  it('steps over rows that cannot be selected (SidebarKeyNavigation)', () => {
    const machine = makeMachine('studio');
    const nodes = [
      projectNode(makeSidebarProject('/a', 0), []),
      ...visibleRows(
        [
          { kind: 'sectionHeader', title: 'MACHINES', id: 's', rowKey: 's', children: [] },
          { kind: 'machine', machine, id: 'm', rowKey: 'm', children: [] },
        ],
        new Set(),
      ).map((row) => row.node),
    ];
    const visible = visibleRows(nodes, new Set());
    expect(sidebarKeyCommand('down', visible, 0)).toEqual({ kind: 'select', index: 2 });
    expect(sidebarKeyCommand('up', visible, 2)).toEqual({ kind: 'select', index: 0 });
    expect(sidebarKeyCommand('up', visible, null)).toEqual({ kind: 'select', index: 2 });
    expect(sidebarKeyCommand('right', visible, null)).toEqual({ kind: 'none' });
  });

  it('runs Enter on a machine row as connect or disconnect, and on other rows as a selection', () => {
    const { store } = memoryStore();
    const { project } = fixture();
    const machine = makeMachine('studio');
    const onToggleMachineConnection = vi.fn();
    const onSelectionChanged = vi.fn();
    renderOutline({
      projects: [project],
      machines: [machine],
      selection: sidebarSelection.project(project.projectId),
      expansionStore: store,
      onToggleMachineConnection,
      onSelectionChanged,
    });
    fireEvent.keyDown(tree(), { key: 'Enter' });
    expect(onSelectionChanged).toHaveBeenCalledWith(sidebarSelection.project(project.projectId));

    fireEvent.mouseDown(requireRow(machineKey(machine.id)), { button: 0 });
    fireEvent.keyDown(tree(), { key: 'Enter' });
    expect(onToggleMachineConnection).toHaveBeenCalledWith(machine.id);
  });

  it('renames a workspace inline with F2, renaming its agent', () => {
    const { store } = memoryStore();
    const { project, worktreeWorkspace, workspacesByProject } = fixture();
    const onRenameAgent = vi.fn();
    renderOutline({
      projects: [project],
      workspacesByProject,
      selection: sidebarSelection.workspace(worktreeWorkspace.id),
      expansionStore: store,
      onRenameAgent,
    });

    fireEvent.keyDown(tree(), { key: 'F2' });
    const field = screen.getByRole('textbox', { name: 'Rename' });
    expect(field).toHaveValue('agent-wt-0');
    expect(field).toHaveFocus();
    fireEvent.change(field, { target: { value: '  Magic Price ' } });
    fireEvent.keyDown(field, { key: 'Enter' });

    expect(onRenameAgent).toHaveBeenCalledWith(project.projectId, 'agent-wt-0', 'Magic Price');
    expect(screen.queryByRole('textbox')).toBeNull();
    expect(tree()).toHaveFocus();
  });

  it('leaves the name as it was when a rename is cancelled, emptied or unchanged', () => {
    const { store } = memoryStore();
    const { project, worktreeWorkspace, workspacesByProject } = fixture();
    const onRenameAgent = vi.fn();
    renderOutline({
      projects: [project],
      workspacesByProject,
      selection: sidebarSelection.workspace(worktreeWorkspace.id),
      expansionStore: store,
      onRenameAgent,
    });

    fireEvent.keyDown(tree(), { key: 'F2' });
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Other' } });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Escape' });
    fireEvent.keyDown(tree(), { key: 'F2' });
    fireEvent.change(screen.getByRole('textbox'), { target: { value: '   ' } });
    fireEvent.blur(screen.getByRole('textbox'));
    fireEvent.keyDown(tree(), { key: 'F2' });
    fireEvent.keyDown(screen.getByRole('textbox'), { key: 'Enter' });

    expect(onRenameAgent).not.toHaveBeenCalled();
  });

  it('only renames workspace rows', () => {
    const { store } = memoryStore();
    const { project } = fixture();
    renderOutline({
      projects: [project],
      selection: sidebarSelection.project(project.projectId),
      expansionStore: store,
    });
    fireEvent.keyDown(tree(), { key: 'F2' });
    expect(screen.queryByRole('textbox')).toBeNull();
  });

  it('deletes a worktree with Delete after its confirmation', () => {
    const { store } = memoryStore();
    const { project } = fixture();
    const onDeleteWorktree = vi.fn();
    renderOutline({
      projects: [project],
      selection: sidebarSelection.worktree('wt-1'),
      expansionStore: store,
      onDeleteWorktree,
    });

    fireEvent.keyDown(tree(), { key: 'Delete' });
    let dialog = screen.getByRole('alertdialog', { name: 'Delete worktree branch-1?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    expect(onDeleteWorktree).not.toHaveBeenCalled();
    expect(screen.queryByRole('alertdialog')).toBeNull();

    fireEvent.keyDown(tree(), { key: 'Delete' });
    dialog = screen.getByRole('alertdialog');
    expect(dialog).toHaveTextContent(
      'This will kill 1 running agent, remove the worktree directory, and delete the branch locally and from GitHub.',
    );
    fireEvent.click(within(dialog).getByRole('button', { name: 'Delete' }));
    expect(onDeleteWorktree).toHaveBeenCalledWith(project.projectId, 'wt-1');
  });

  it('removes a project or a machine with Delete after its confirmation', () => {
    const { store } = memoryStore();
    const { project } = fixture();
    const machine = makeMachine('studio');
    const onRemoveProject = vi.fn();
    const onRemoveMachine = vi.fn();
    const view = renderOutline({
      projects: [project],
      machines: [machine],
      selection: sidebarSelection.project(project.projectId),
      expansionStore: store,
      onRemoveProject,
      onRemoveMachine,
    });

    fireEvent.keyDown(tree(), { key: 'Delete' });
    const projectDialog = screen.getByRole('alertdialog', {
      name: `Remove ${project.projectRoot.split('/').pop() ?? ''}?`,
    });
    fireEvent.keyDown(projectDialog, { key: 'Escape' });
    expect(onRemoveProject).not.toHaveBeenCalled();
    fireEvent.keyDown(tree(), { key: 'Delete' });
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(onRemoveProject).toHaveBeenCalledWith(project.projectId);

    view.update({ selection: null });
    fireEvent.mouseDown(requireRow(machineKey(machine.id)), { button: 0 });
    fireEvent.keyDown(tree(), { key: 'Delete' });
    screen.getByRole('alertdialog', { name: 'Remove studio?' });
    fireEvent.click(screen.getByRole('button', { name: 'Remove' }));
    expect(onRemoveMachine).toHaveBeenCalledWith(machine.id);
  });

  it('kills a one-agent workspace with Delete, and asks first for a workspace of several', () => {
    const { store } = memoryStore();
    const { project, worktreeWorkspace, rootWorkspace, workspacesByProject } = fixture();
    const onKillWorkspace = vi.fn();
    const view = renderOutline({
      projects: [project],
      workspacesByProject,
      selection: sidebarSelection.workspace(worktreeWorkspace.id),
      expansionStore: store,
      onKillWorkspace,
    });

    // The menu's Kill Agent, which the Mac runs without asking.
    fireEvent.keyDown(tree(), { key: 'Delete' });
    expect(onKillWorkspace).toHaveBeenCalledWith(project.projectId, worktreeWorkspace.id);

    const split = splitPane(rootWorkspace, 0, 'horizontal', agentContent('other')).workspace;
    view.update({
      workspacesByProject: { [project.projectId]: [split] },
      selection: sidebarSelection.workspace(split.id),
    });
    onKillWorkspace.mockClear();
    fireEvent.keyDown(tree(), { key: 'Delete' });
    expect(onKillWorkspace).not.toHaveBeenCalled();
    const dialog = screen.getByRole('alertdialog', { name: 'Close this workspace?' });
    fireEvent.click(within(dialog).getByRole('button', { name: 'Close' }));
    expect(onKillWorkspace).toHaveBeenCalledWith(project.projectId, split.id);
  });

  it('opens the command palette from the add buttons', () => {
    const { store } = memoryStore();
    const { project } = fixture();
    const onShowCommandPalette = vi.fn();
    renderOutline({ projects: [project], expansionStore: store, onShowCommandPalette });

    fireEvent.click(within(requireRow(projectKey(project))).getByRole('button', { name: 'Add' }));
    expect(onShowCommandPalette).toHaveBeenLastCalledWith(project.projectId, null, true);
    fireEvent.click(
      within(requireRow(worktreeKey(project, 'wt-0'))).getByRole('button', { name: 'Add' }),
    );
    expect(onShowCommandPalette).toHaveBeenLastCalledWith(
      project.projectId,
      sidebarSelection.worktree('wt-0'),
      false,
    );
  });

  it('draws only the rows in view, and follows the scroll', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject('/tmp/big', 500);
    renderOutline({ projects: [project], expansionStore: store });

    expect(tree()).toHaveAttribute('data-row-count', '501');
    expect(rows().length).toBeLessThan(100);
    expect(rowFor(worktreeKey(project, 'wt-400'))).toBeNull();

    act(() => {
      tree().scrollTop = 400 * SIDEBAR_ROW_HEIGHT;
      fireEvent.scroll(tree());
    });
    expect(rowFor(worktreeKey(project, 'wt-400'))).not.toBeNull();
    expect(rowFor(projectKey(project))).toBeNull();
  });

  it('brings a row selected from elsewhere into view', () => {
    const { store } = memoryStore();
    const project = makeSidebarProject('/tmp/reveal', 500);
    const view = renderOutline({ projects: [project], expansionStore: store });
    Object.defineProperty(tree(), 'clientHeight', { configurable: true, value: 240 });

    view.update({ selection: sidebarSelection.worktree('wt-300') });

    expect(tree().scrollTop).toBe(302 * SIDEBAR_ROW_HEIGHT - 240);
  });
});

// MARK: - Footer and shell

describe('SidebarView', () => {
  function renderShell() {
    const app = makeTestApp();
    render(
      <AppStateContext.Provider value={app}>
        <ContentView />
      </AppStateContext.Provider>,
    );
    return app;
  }

  it('shows "No project open" until a project opens, then its row', () => {
    const app = renderShell();
    expect(screen.getByText('No project open')).toBeInTheDocument();
    act(() => {
      app.openProject('C:\\work\\purepoint');
    });
    expect(screen.queryByText('No project open')).toBeNull();
    expect(screen.getByRole('treeitem', { name: 'purepoint' })).toBeInTheDocument();
  });

  it('changes the detail as a project is selected, and back to a nav screen', () => {
    const app = renderShell();
    act(() => {
      app.openProject('C:\\work\\purepoint');
    });
    fireEvent.mouseDown(screen.getByRole('treeitem', { name: 'purepoint' }), { button: 0 });
    expect(screen.getByText('ProjectDetailView')).toBeInTheDocument();
    expect(app.activeProjectRoot).toBe('C:\\work\\purepoint');

    fireEvent.click(screen.getByRole('button', { name: 'Point Guard' }));
    expect(screen.getByText('PointGuardView')).toBeInTheDocument();
    expect(screen.getByRole('treeitem', { name: 'purepoint' })).toHaveAttribute(
      'aria-selected',
      'false',
    );
  });

  it('opens Settings from the footer gear', () => {
    const app = renderShell();
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    expect(app.store.getState().showSettings).toBe(true);
  });

  it('opens the command palette for the selected project from the footer', () => {
    const app = makeTestApp();
    act(() => {
      app.openProject('C:\\work\\purepoint');
    });
    const openPalette = vi.fn();
    const selection = sidebarSelection.project('C:\\work\\purepoint');
    render(
      <AppStateContext.Provider value={app}>
        <SidebarFooter selection={selection} openPalette={openPalette} />
      </AppStateContext.Provider>,
    );
    expect(screen.getByRole('button', { name: 'New' })).toHaveTextContent('Ctrl+Shift+N');
    fireEvent.click(screen.getByRole('button', { name: 'New' }));
    expect(openPalette).toHaveBeenCalledWith({
      projectId: 'C:\\work\\purepoint',
      selection,
      includeWorktree: true,
    });
  });
});
