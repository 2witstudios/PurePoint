// The sidebar's menus and project drag (WIN-7.8): each row's context menu offers exactly the
// entries `SidebarMenuModel` makes, each entry runs its callback behind the Mac's confirmation,
// and dragging a project row reorders it among its siblings.

import { createEvent, fireEvent, render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { makeAgent, makeMachine, makeProject } from '../../model/sidebarFixtures.ts';
import {
  type ProjectSnapshot,
  type SidebarNode,
  machineKey,
  rowKey,
  selection as sidebarSelection,
} from '../../model/sidebarItem.ts';
import { contextMenu } from '../../model/sidebarMenu.ts';
import { type WorkspacesByProject, buildSidebarTree } from '../../model/sidebarOutline.ts';
import { adoptingWorkspace, worktreeContainer } from '../../model/workspace.ts';
import { createSidebarExpansionStore } from '../../state/sidebarExpansionStore.ts';
import {
  SidebarOutlineViewController,
  type SidebarOutlineViewControllerProps,
} from './SidebarOutlineViewController.tsx';
import { PROJECT_DRAG_TYPE, projectDropTarget } from './sidebarOutlineViewControllerProjectDrag.ts';

let counter = 0;
function local(name: string, worktrees = 1): ProjectSnapshot {
  counter += 1;
  return makeProject(`/tmp/menus-${name}-${String(counter)}`, {
    worktreeCount: worktrees,
    machineId: null,
    hasLoadedWorktrees: true,
  });
}

function renderOutline(
  props: Partial<SidebarOutlineViewControllerProps> & { projects: ProjectSnapshot[] },
) {
  return render(
    <SidebarOutlineViewController expansionStore={createSidebarExpansionStore(null)} {...props} />,
  );
}

function row(key: string): HTMLElement {
  const found = screen
    .queryAllByRole('treeitem')
    .find((element) => element.getAttribute('data-row-key') === key);
  if (found === undefined) throw new Error(`no row ${key}`);
  return found;
}

function projectRowKey(project: ProjectSnapshot): string {
  return rowKey('project', project.projectId);
}

/** Right-click a row; its menu's items as drawn (`---` separators, `(off)` disabled). */
function openMenu(key: string): string[] {
  fireEvent.contextMenu(row(key), { clientX: 20, clientY: 30 });
  const menu = screen.getByRole('menu');
  return Array.from(menu.querySelectorAll('[role="menuitem"], [role="separator"]'), (node) =>
    node.getAttribute('role') === 'separator'
      ? '---'
      : `${node.textContent}${(node as HTMLButtonElement).disabled ? ' (off)' : ''}`,
  );
}

/** What the model says the menu is, in the same form. */
function modelTitles(node: SidebarNode, context = {}): string[] {
  return contextMenu(node, context).map((entry) =>
    entry.type === 'separator' ? '---' : `${entry.title}${entry.isEnabled ? '' : ' (off)'}`,
  );
}

function choose(title: string): void {
  fireEvent.click(screen.getByRole('menuitem', { name: title }));
}

function confirmDialog(name: string, button: string): void {
  const dialog = screen.getByRole('alertdialog', { name });
  fireEvent.click(within(dialog).getByRole('button', { name: button }));
}

describe('sidebar context menus', () => {
  it('given each kind of row, should offer exactly the model’s entries', () => {
    const first = local('first');
    const second = local('second');
    const machine = makeMachine('studio', {
      state: { kind: 'connected', daemon: { kind: 'running' } },
      hiddenProjectCount: 2,
      actionError: 'oops',
    });
    const workspace = adoptingWorkspace('agent-wt-0', worktreeContainer('wt-0'));
    const workspacesByProject: WorkspacesByProject = { [first.projectId]: [workspace] };
    const siblingProjectIds = () => [first.projectId, second.projectId];
    renderOutline({
      projects: [first, second],
      machines: [machine],
      workspacesByProject,
      menuContext: { siblingProjectIds },
    });
    const nodes = buildSidebarTree([first, second], [machine], workspacesByProject);
    const find = (key: string): SidebarNode => {
      const walk = (list: readonly SidebarNode[]): SidebarNode | null => {
        for (const node of list) {
          if (node.rowKey === key) return node;
          const inner = walk(node.children);
          if (inner !== null) return inner;
        }
        return null;
      };
      const node = walk(nodes);
      if (node === null) throw new Error(key);
      return node;
    };

    const projectKey = projectRowKey(first);
    expect(openMenu(projectKey)).toEqual(modelTitles(find(projectKey), { siblingProjectIds }));
    expect(openMenu(projectKey)).toEqual([
      'Kill All Agents (1)',
      '---',
      'Move Up (off)',
      'Move Down',
      '---',
      'Remove Project…',
    ]);
    const worktreeKey = rowKey('worktree', 'wt-0', projectKey);
    expect(openMenu(worktreeKey)).toEqual(modelTitles(find(worktreeKey)));
    const workspaceKey = row(worktreeKey).nextElementSibling?.getAttribute('data-row-key') ?? '';
    expect(openMenu(workspaceKey)).toEqual(['Rename…', '---', 'Kill Agent']);
    expect(openMenu(machineKey(machine.id))).toEqual(modelTitles(find(machineKey(machine.id))));
    // A section title has no menu.
    fireEvent.contextMenu(row(nodes.find((node) => node.kind === 'sectionHeader')?.rowKey ?? ''));
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('given a workspace row, should rename and kill from its menu', () => {
    const project = local('ws');
    const workspace = adoptingWorkspace('agent-wt-0', worktreeContainer('wt-0'));
    const onRenameAgent = vi.fn();
    const onKillWorkspace = vi.fn();
    renderOutline({
      projects: [project],
      workspacesByProject: { [project.projectId]: [workspace] },
      onRenameAgent,
      onKillWorkspace,
    });
    const key = rowKey(
      'workspace',
      workspace.id,
      rowKey('worktree', 'wt-0', projectRowKey(project)),
    );
    const workspaceRow = screen
      .getAllByRole('treeitem')
      .find((element) => element.dataset.kind === 'workspace');
    const workspaceKey = workspaceRow?.dataset.rowKey ?? key;

    openMenu(workspaceKey);
    choose('Rename…');
    expect(screen.queryByRole('menu')).toBeNull();
    const field = screen.getByRole('textbox', { name: 'Rename' });
    expect(field).toHaveFocus();
    fireEvent.change(field, { target: { value: 'Renamed' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    expect(onRenameAgent).toHaveBeenCalledWith(project.projectId, 'agent-wt-0', 'Renamed');

    openMenu(workspaceKey);
    choose('Kill Agent');
    expect(onKillWorkspace).toHaveBeenCalledWith(project.projectId, workspace.id);
  });

  it('given a worktree and a project, should ask before each destructive entry', () => {
    const first = local('dest');
    const second = local('other');
    const callbacks = {
      onKillWorktreeAgents: vi.fn(),
      onDeleteWorktree: vi.fn(),
      onKillAllProjectAgents: vi.fn(),
      onRemoveProject: vi.fn(),
      onMoveProject: vi.fn(),
    };
    renderOutline({
      projects: [first, second],
      menuContext: { siblingProjectIds: () => [first.projectId, second.projectId] },
      ...callbacks,
    });
    const worktreeKey = rowKey('worktree', 'wt-0', projectRowKey(first));

    openMenu(worktreeKey);
    choose('Kill All Agents (1)');
    expect(callbacks.onKillWorktreeAgents).not.toHaveBeenCalled();
    confirmDialog('Kill All Agents in branch-0?', 'Kill All');
    expect(callbacks.onKillWorktreeAgents).toHaveBeenCalledWith(first.projectId, 'wt-0');

    openMenu(worktreeKey);
    choose('Delete Worktree…');
    confirmDialog('Delete worktree branch-0?', 'Delete');
    expect(callbacks.onDeleteWorktree).toHaveBeenCalledWith(first.projectId, 'wt-0');

    openMenu(projectRowKey(first));
    choose('Kill All Agents (1)');
    confirmDialog(`Kill All Agents in ${first.projectRoot.split('/').pop() ?? ''}?`, 'Kill All');
    expect(callbacks.onKillAllProjectAgents).toHaveBeenCalledWith(first.projectId);

    openMenu(projectRowKey(first));
    choose('Move Down');
    expect(callbacks.onMoveProject).toHaveBeenCalledWith(first.projectId, 1);
    openMenu(projectRowKey(second));
    choose('Move Up');
    expect(callbacks.onMoveProject).toHaveBeenLastCalledWith(second.projectId, 0);

    openMenu(projectRowKey(first));
    choose('Remove Project…');
    confirmDialog(`Remove ${first.projectRoot.split('/').pop() ?? ''}?`, 'Remove');
    expect(callbacks.onRemoveProject).toHaveBeenCalledWith(first.projectId);
  });

  it('given a remote project and its machine, should hide, retry and run the machine entries', () => {
    const remote = {
      ...makeProject('/srv/app', { worktreeCount: 0, hasLoadedWorktrees: true }),
      layoutError: 'Could not save.',
    };
    const machine = makeMachine('studio', {
      state: { kind: 'connected', daemon: { kind: 'running' } },
      hiddenProjectCount: 1,
      actionError: 'Failed',
      hasLoadedProjects: true,
      projects: [remote],
    });
    const callbacks = {
      onHideProject: vi.fn(),
      onRetryLayout: vi.fn(),
      onRemoveProject: vi.fn(),
      onDisconnectMachine: vi.fn(),
      onConnectMachine: vi.fn(),
      onShowHiddenProjects: vi.fn(),
      onDismissMachineError: vi.fn(),
      onRemoveMachine: vi.fn(),
    };
    const view = renderOutline({
      projects: [],
      machines: [machine],
      menuContext: {
        machineName: () => 'studio',
        machineUnsavedFileNames: () => ['notes.md'],
      },
      ...callbacks,
    });
    const remoteKey = rowKey('project', remote.projectId, machineKey(machine.id));

    expect(openMenu(remoteKey)).toEqual([
      'Retry Layout',
      'Hide on This PC…',
      'Remove from studio…',
    ]);
    choose('Retry Layout');
    expect(callbacks.onRetryLayout).toHaveBeenCalledWith(remote.projectId);
    openMenu(remoteKey);
    choose('Hide on This PC…');
    confirmDialog('Hide app?', 'Hide');
    expect(callbacks.onHideProject).toHaveBeenCalledWith(remote.projectId);

    const key = machineKey(machine.id);
    openMenu(key);
    choose('Show Hidden Projects (1)');
    expect(callbacks.onShowHiddenProjects).toHaveBeenCalledWith(machine.id);
    openMenu(key);
    choose('Dismiss Error');
    expect(callbacks.onDismissMachineError).toHaveBeenCalledWith(machine.id);
    // Unsaved files make Disconnect ask first.
    openMenu(key);
    choose('Disconnect');
    expect(callbacks.onDisconnectMachine).not.toHaveBeenCalled();
    confirmDialog('Disconnect studio?', 'Disconnect');
    expect(callbacks.onDisconnectMachine).toHaveBeenCalledWith(machine.id);
    openMenu(key);
    choose('Remove Machine…');
    confirmDialog('Remove studio?', 'Remove');
    expect(callbacks.onRemoveMachine).toHaveBeenCalledWith(machine.id);

    view.rerender(
      <SidebarOutlineViewController
        expansionStore={createSidebarExpansionStore(null)}
        projects={[]}
        machines={[{ ...machine, state: { kind: 'disconnected' } }]}
        {...callbacks}
      />,
    );
    openMenu(key);
    choose('Connect');
    expect(callbacks.onConnectMachine).toHaveBeenCalledWith(machine.id);
  });

  it('given the menu key or Shift+F10, should open the selected row’s menu; Escape closes it', () => {
    const project = local('keys');
    renderOutline({
      projects: [project],
      selection: sidebarSelection.project(project.projectId),
    });
    const tree = screen.getByRole('tree');
    fireEvent.keyDown(tree, { key: 'F10', shiftKey: true });
    const menu = screen.getByRole('menu');
    expect(screen.getAllByRole('menuitem')[0]).toHaveFocus();
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(tree).toHaveFocus();
    fireEvent.keyDown(tree, { key: 'ContextMenu' });
    expect(screen.getByRole('menu')).toBeInTheDocument();
    // A press elsewhere closes it.
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('project drag', () => {
  function dataTransfer(): DataTransfer {
    const data = new Map<string, string>();
    return {
      get types() {
        return [...data.keys()];
      },
      setData: (type: string, value: string) => {
        data.set(type, value);
      },
      getData: (type: string) => data.get(type) ?? '',
      dropEffect: 'none',
      effectAllowed: 'all',
    } as unknown as DataTransfer;
  }

  it('given a project dropped on another, should move it there among its siblings', () => {
    const a = local('a', 0);
    const b = local('b', 0);
    const c = local('c', 0);
    const onMoveProject = vi.fn();
    renderOutline({ projects: [a, b, c], onMoveProject });
    const transfer = dataTransfer();
    fireEvent.dragStart(row(projectRowKey(c)), { dataTransfer: transfer });
    expect(transfer.getData(PROJECT_DRAG_TYPE)).toBe(c.projectId);
    // jsdom has no DragEvent, so the pointer's height is set on the event by hand: above the
    // (0×0) row is its top half.
    const at = (type: 'dragOver' | 'drop', element: HTMLElement) => {
      const event = createEvent[type](element, { dataTransfer: transfer });
      Object.defineProperty(event, 'clientY', { value: -10 });
      fireEvent(element, event);
    };
    at('dragOver', row(projectRowKey(a)));
    expect(row(projectRowKey(a))).toHaveAttribute('data-drop-position', 'before');
    at('drop', row(projectRowKey(a)));
    expect(onMoveProject).toHaveBeenCalledWith(c.projectId, 0);
    expect(row(projectRowKey(a))).not.toHaveAttribute('data-drop-position');
  });

  it('given drop targets, should land before, after, inside and never across sections', () => {
    const a = local('ta', 1);
    const b = local('tb', 0);
    const remote = makeProject('/srv/x', { worktreeCount: 0 });
    const machine = makeMachine('m', { projects: [remote] });
    const roots = buildSidebarTree([a, b], [machine], {});
    const [nodeA, nodeB, header, machineNode] = roots;
    if (!nodeA || !nodeB || !header || !machineNode) throw new Error('tree');
    expect(projectDropTarget(a.projectId, nodeB, 'after', roots)?.drop.index).toBe(1);
    expect(projectDropTarget(b.projectId, nodeA, 'before', roots)?.drop.index).toBe(0);
    // A row inside a project stands for "before that project".
    const worktree = nodeA.children[0];
    if (!worktree) throw new Error('worktree');
    expect(projectDropTarget(b.projectId, worktree, 'after', roots)).toMatchObject({
      rowKey: nodeA.rowKey,
      position: 'before',
      drop: { index: 0 },
    });
    // The MACHINES title is the end of the local projects.
    expect(projectDropTarget(a.projectId, header, 'before', roots)?.drop.index).toBe(1);
    // Where it already is, or another section: no drop.
    expect(projectDropTarget(a.projectId, nodeA, 'before', roots)).toBeNull();
    expect(
      projectDropTarget(a.projectId, machineNode.children[0] ?? nodeA, 'before', roots),
    ).toBeNull();
    expect(projectDropTarget(remote.projectId, nodeA, 'before', roots)).toBeNull();
  });

  it('given an agent row, should not be draggable; a project row is', () => {
    const project = { ...local('drag'), rootAgents: [makeAgent('root')] };
    renderOutline({ projects: [project] });
    expect(row(projectRowKey(project))).toHaveAttribute('draggable', 'true');
    expect(row(rowKey('worktree', 'wt-0', projectRowKey(project)))).not.toHaveAttribute(
      'draggable',
    );
  });
});
