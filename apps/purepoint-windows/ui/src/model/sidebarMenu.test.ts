import { describe, expect, it } from 'vitest';
import { adoptingWorkspace, worktreeContainer } from './workspace.ts';
import { makeMachine, makeProject } from './sidebarFixtures.ts';
import type { SidebarNode } from './sidebarItem.ts';
import { buildSidebarTree, projectDrop } from './sidebarOutline.ts';
import {
  SIDEBAR_DEVICE,
  contextMenu,
  machineAccessibilityActions,
  sidebarConfirmation,
  worktreeAddMenu,
  type SidebarMenuEntry,
} from './sidebarMenu.ts';

// The sidebar's menus and confirmations. (Port of SidebarMenuModelTests.swift; the Mac's
// "This Mac" is "This PC" here.)

/** A machine's project offers hiding on this device and removing from the machine. */
const hideTitle = `Hide on This ${SIDEBAR_DEVICE}…`;
const remoteProjectActions = [hideTitle, 'Remove from Machine…'];

function titles(entries: readonly SidebarMenuEntry[]): string[] {
  return entries.map((entry) => (entry.type === 'action' ? entry.title : '-'));
}

function row(node: SidebarNode | undefined): SidebarNode {
  if (node === undefined) throw new Error('missing row');
  return node;
}

describe('SidebarMenuModelTests', () => {
  // MARK: - Menus

  it('givenEachRowTypeShouldOfferTheMacContextMenu', () => {
    const project = makeProject('/tmp/menus', { worktreeCount: 1 });
    const single = adoptingWorkspace('agent-0', worktreeContainer('wt-0'));
    const machine = makeMachine('m');
    const nodes = buildSidebarTree([project], [machine], { [project.projectId]: [single] });
    const projectRow = row(nodes[0]);
    const worktreeRow = row(projectRow.children[0]);

    expect(titles(contextMenu(projectRow))).toEqual([
      'Kill All Agents (1)',
      '-',
      ...remoteProjectActions,
    ]);
    expect(titles(contextMenu(worktreeRow))).toEqual([
      'Kill All Agents (1)',
      '-',
      'Delete Worktree…',
    ]);
    expect(titles(contextMenu(row(worktreeRow.children[0])))).toEqual([
      'Rename…',
      '-',
      'Kill Agent',
    ]);
    expect(contextMenu(row(nodes[1]))).toEqual([]);
    expect(titles(contextMenu(row(nodes[2])))).toEqual(['Connect', '-', 'Remove Machine…']);

    // `machine.actionError = "ssh failed"`: a new snapshot, so a rebuilt tree.
    const failed = { ...machine, actionError: 'ssh failed' };
    const rebuilt = buildSidebarTree([project], [failed], { [project.projectId]: [single] });
    expect(titles(contextMenu(row(rebuilt[2])))).toEqual([
      'Connect',
      'Dismiss Error',
      '-',
      'Remove Machine…',
    ]);
    expect(machineAccessibilityActions(failed).map((action) => action.name)).toEqual([
      'Connect',
      'Dismiss Error',
      'Remove Machine',
    ]);
  });

  it('givenDeadAgentsShouldOfferOnlyTheDestructiveRowActions', () => {
    const project = makeProject('/tmp/dead', { worktreeCount: 1, status: 'broken' });
    const nodes = buildSidebarTree([project], [], {});
    expect(titles(contextMenu(row(nodes[0])))).toEqual(remoteProjectActions);
    expect(titles(contextMenu(row(nodes[0]?.children[0])))).toEqual(['Delete Worktree…']);
  });

  it('givenAProjectDropShouldMoveItOnlyWithinItsOwnSection', () => {
    const a = makeProject('/tmp/a');
    const b = makeProject('/tmp/b');
    const c = makeProject('/tmp/c');
    const machine = makeMachine('m');
    const roots = buildSidebarTree([a, b, c], [machine], {});
    const drop = (id: string, parent: SidebarNode | null, index: number) => {
      const result = projectDrop(id, parent, index, roots);
      return result === null ? null : `${result.project.projectRoot}@${String(result.index)}`;
    };

    // Dropping before the first row moves to the top; past the last local row moves to the end.
    expect(drop(c.projectId, null, 0)).toBe('/tmp/c@0');
    expect(drop(a.projectId, null, 3)).toBe('/tmp/a@2');
    expect(drop(a.projectId, null, 2)).toBe('/tmp/a@1');
    // Its own place, below the local projects, or under a machine it isn't on: no move.
    expect(drop(a.projectId, null, 0)).toBeNull();
    expect(drop(a.projectId, null, 1)).toBeNull();
    expect(drop(a.projectId, null, 4)).toBeNull();
    expect(drop(a.projectId, row(roots[4]), 0)).toBeNull();
  });

  it('givenAWorktreeAddButtonShouldOfferANewAgentOrTerminal', () => {
    expect(titles(worktreeAddMenu('/tmp/p', 'wt-0'))).toEqual(['New Agent', 'New Terminal']);
  });

  // Not in the Swift suite: the entries carry descriptors naming their targets (WIN-7.8 wiring).
  it('givenAMenuEntryShouldNameWhatItActsOn', () => {
    const project = makeProject('/tmp/targets', { worktreeCount: 1 });
    const single = adoptingWorkspace('agent-0', worktreeContainer('wt-0'));
    const nodes = buildSidebarTree([project], [], { [project.projectId]: [single] });
    const worktreeRow = row(nodes[0]?.children[0]);

    expect(contextMenu(row(worktreeRow.children[0]))[2]).toEqual({
      type: 'action',
      title: 'Kill Agent',
      action: { kind: 'killWorkspace', workspaceId: single.id, agentIds: ['agent-0'] },
      isEnabled: true,
    });
    expect(contextMenu(worktreeRow)[2]).toMatchObject({
      action: { kind: 'deleteWorktree', projectId: project.projectId, worktreeId: 'wt-0' },
    });
    expect(
      contextMenu(row(nodes[0]), { siblingProjectIds: () => [project.projectId, 'other'] }),
    ).toContainEqual({
      type: 'action',
      title: 'Move Up',
      action: { kind: 'moveProjectUp', projectId: project.projectId },
      isEnabled: false,
    });
  });

  // MARK: - Confirmations

  it('givenDestructiveActionsShouldAskWithTheMacWording', () => {
    const project = makeProject('/tmp/confirm', { worktreeCount: 1 });
    const worktree = project.worktrees[0];
    if (worktree === undefined) throw new Error('no worktree');

    expect(sidebarConfirmation.deleteWorktree(worktree)).toEqual({
      title: 'Delete worktree branch-0?',
      message:
        'This will kill 1 running agent, remove the worktree directory, and delete the branch locally and from GitHub.',
      confirmTitle: 'Delete',
    });
    expect(sidebarConfirmation.killWorktreeAgents(worktree).confirmTitle).toBe('Kill All');
    expect(sidebarConfirmation.closeWorkspace(3).message).toBe(
      'This will kill 3 agents across its panes.',
    );
    // Hiding changes nothing on the machine.
    expect(
      sidebarConfirmation
        .hideProject(project)
        .message.endsWith('Nothing on the remote machine is changed.'),
    ).toBe(true);
    // Removing does, for every device, but keeps the files and suspends running agents.
    expect(sidebarConfirmation.removeProject(project)).toEqual({
      title: 'Remove confirm from Machine?',
      message:
        'It disappears from PurePoint on Machine and every device connected to it.' +
        ' Its 1 running agent is suspended and can be resumed once the project is added again.' +
        ' Any other session in the folder is stopped. The folder and .pu/ are kept.',
      confirmTitle: 'Remove',
    });
  });
});
