// WIN-6.4: every `pu grid` command the daemon rebroadcasts (`grid-event`) applied to the layout,
// as Swift's `WorkspaceRegistry.handleRemoteCommand` applies the commands `DaemonGridSubscription`
// receives. The Swift cases (`remote...` in WorkspaceRegistryTests.swift) are ported in
// workspaceRegistry.test.ts; these cover each command and its unknown-target handling, which the
// Swift suite leaves untested. AppState's forwarding of the event is covered in appState.test.ts.

import { describe, expect, it } from 'vitest';
import {
  agentContent,
  focusedAgentId,
  focusedSurface,
  leafIdOfSurface,
  paneCount,
  tabCount,
  workspaceAgentIds,
  type Workspace,
} from '../model/workspace';
import { allLeafIds } from '../model/paneSplit';
import { makeAgentModel, type AgentModel } from '../model/workspaceModel';
import type { GridCommand } from '../protocol';
import { ManualScheduler, MemoryWorkspacePersistence } from './workspaceRegistry.fixtures';
import { WorkspaceRegistry } from './workspaceRegistry';

const root = '/tmp/pp-grid-commands';

function agent(id: string): AgentModel {
  return makeAgentModel({
    id,
    name: id,
    agentType: 'claude',
    status: 'running',
    prompt: '',
    startedAt: '',
  });
}

/** A registry showing `ws-ag-a` (one pane, leaf 0, holding ag-a in tab 0). */
function makeRegistry(agents: string[] = ['ag-a']): WorkspaceRegistry {
  const registry = new WorkspaceRegistry({
    persistence: new MemoryWorkspacePersistence(),
    scheduler: new ManualScheduler(),
  });
  registry.reconcile(root, agents.map(agent), []);
  registry.activeWorkspaceId = 'ws-ag-a';
  return registry;
}

function ws(registry: WorkspaceRegistry, id = 'ws-ag-a'): Workspace {
  const found = registry.workspace(id);
  expect(found).not.toBeNull();
  return found as Workspace;
}

function killedAgents(registry: WorkspaceRegistry): string[] {
  const killed: string[] = [];
  registry.onCloseAgent = (_, agentId) => killed.push(agentId);
  return killed;
}

function apply(registry: WorkspaceRegistry, command: GridCommand, from = root): void {
  registry.handleRemoteCommand(command, from);
}

describe('grid commands', () => {
  it('split: splits the named leaf on the given axis, h being top/bottom', () => {
    const registry = makeRegistry();

    apply(registry, { action: 'split', leaf_id: 0, axis: 'h' });

    const workspace = ws(registry);
    expect(paneCount(workspace)).toBe(2);
    expect(workspace.root.type === 'split' && workspace.root.axis).toBe('horizontal');
  });

  it('split: any other axis (or none) splits left/right', () => {
    const registry = makeRegistry();

    apply(registry, { action: 'split' });

    const workspace = ws(registry);
    expect(workspace.root.type === 'split' && workspace.root.axis).toBe('vertical');
  });

  it('close: closes the focused pane when no leaf is named, killing its agent', () => {
    const registry = makeRegistry(['ag-a', 'ag-b']);
    registry.split('ws-ag-a', 0, 'vertical');
    registry.setAgent('ag-b', 'ws-ag-a', ws(registry).panes[1]?.activeTabId as number);
    registry.setFocus('ws-ag-a', 0);
    const killed = killedAgents(registry);

    apply(registry, { action: 'close', leaf_id: null });

    expect(killed).toEqual(['ag-a']);
    expect(allLeafIds(ws(registry).root)).toEqual([1]);
  });

  it('focus: focuses a named leaf, and ignores one that does not exist', () => {
    const registry = makeRegistry();
    registry.split('ws-ag-a', 0, 'vertical');

    apply(registry, { action: 'focus', leaf_id: 0 });
    expect(ws(registry).focusedLeafId).toBe(0);

    apply(registry, { action: 'focus', leaf_id: 99 });
    expect(ws(registry).focusedLeafId).toBe(0);
  });

  it('focus: moves focus by direction, unknown directions meaning right', () => {
    const registry = makeRegistry();
    registry.split('ws-ag-a', 0, 'vertical');
    registry.setFocus('ws-ag-a', 0);

    apply(registry, { action: 'focus', direction: 'sideways' });
    expect(ws(registry).focusedLeafId).toBe(1);

    apply(registry, { action: 'focus', direction: 'left' });
    expect(ws(registry).focusedLeafId).toBe(0);

    apply(registry, { action: 'focus', direction: 'up' });
    expect(ws(registry).focusedLeafId).toBe(0);
  });

  it('focus: with neither a leaf nor a direction does nothing', () => {
    const registry = makeRegistry();
    registry.split('ws-ag-a', 0, 'vertical');
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'focus' });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('set_agent: binds the agent to the named pane’s active tab', () => {
    const registry = makeRegistry(['ag-a', 'ag-b']);
    registry.split('ws-ag-a', 0, 'vertical');
    const tab = ws(registry).panes[1]?.activeTabId as number;

    apply(registry, { action: 'set_agent', leaf_id: 1, agent_id: 'ag-b' });

    expect(ws(registry).panes[1]?.tabs).toEqual([{ id: tab, content: agentContent('ag-b') }]);
    expect(registry.workspace('ws-ag-b')).toBeNull();
  });

  it('set_agent: an unknown leaf is ignored', () => {
    const registry = makeRegistry(['ag-a', 'ag-b']);
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'set_agent', leaf_id: 7, agent_id: 'ag-b' });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('new_tab: without an agent opens an empty tab beside the active one', () => {
    const registry = makeRegistry();

    apply(registry, { action: 'new_tab', leaf_id: 0 });

    const workspace = ws(registry);
    expect(tabCount(workspace)).toBe(2);
    expect(focusedSurface(workspace)?.content).toEqual({ kind: 'empty' });
  });

  it('new_tab: an unknown leaf is ignored', () => {
    const registry = makeRegistry();
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'new_tab', leaf_id: 5, agent_id: 'ag-a' });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('select_tab: cycles forward for any direction but prev, and ignores out-of-range positions', () => {
    const registry = makeRegistry();
    registry.newTab('ws-ag-a', 0);
    registry.selectTabAt('ws-ag-a', 0, 0);

    apply(registry, { action: 'select_tab', direction: 'next' });
    expect(focusedSurface(ws(registry))?.content).toEqual({ kind: 'empty' });

    apply(registry, { action: 'select_tab', direction: 'next' });
    expect(focusedAgentId(ws(registry))).toBe('ag-a');

    apply(registry, { action: 'select_tab', index: 9 });
    apply(registry, { action: 'select_tab', index: 0 });
    expect(focusedAgentId(ws(registry))).toBe('ag-a');
  });

  it('close_tab: by tab ID closes that tab and kills its agent', () => {
    const registry = makeRegistry(['ag-a', 'ag-b']);
    const other = registry.newTab('ws-ag-a', 0) as number;
    registry.setAgent('ag-b', 'ws-ag-a', other);
    const killed = killedAgents(registry);

    apply(registry, { action: 'close_tab', tab_id: 0 });

    expect(killed).toEqual(['ag-a']);
    expect(ws(registry).panes[0]?.tabs.map((tab) => tab.id)).toEqual([other]);
  });

  it('close_tab: without a tab ID closes the target pane’s active tab', () => {
    const registry = makeRegistry();
    registry.newTab('ws-ag-a', 0);
    const killed = killedAgents(registry);

    apply(registry, { action: 'close_tab', leaf_id: null });

    expect(killed).toEqual([]);
    expect(workspaceAgentIds(ws(registry))).toEqual(['ag-a']);
    expect(tabCount(ws(registry))).toBe(1);
  });

  it('close_tab: an unknown tab or leaf is ignored', () => {
    const registry = makeRegistry();
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'close_tab', tab_id: 42 });
    apply(registry, { action: 'close_tab', leaf_id: 42 });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('move_tab: moves a tab to a 1-based position in another pane, keeping its agent', () => {
    const registry = makeRegistry();
    registry.split('ws-ag-a', 0, 'vertical');
    registry.newTab('ws-ag-a', 0);
    const killed = killedAgents(registry);

    apply(registry, { action: 'move_tab', tab_id: 0, to_leaf: 1, index: 1 });

    const workspace = ws(registry);
    expect(killed).toEqual([]);
    expect(leafIdOfSurface(workspace, 0)).toBe(1);
    expect(workspace.panes[1]?.tabs[0]?.content).toEqual(agentContent('ag-a'));
  });

  it('move_tab: without a position appends; an unknown pane is ignored', () => {
    const registry = makeRegistry();
    registry.split('ws-ag-a', 0, 'vertical');
    registry.newTab('ws-ag-a', 0);

    apply(registry, { action: 'move_tab', tab_id: 0, to_leaf: 9 });
    expect(leafIdOfSurface(ws(registry), 0)).toBe(0);

    apply(registry, { action: 'move_tab', tab_id: 0, to_leaf: 1 });
    expect(ws(registry).panes[1]?.tabs.at(-1)?.id).toBe(0);
  });

  it('break_tab: moves the named tab into a new pane split off on the axis', () => {
    const registry = makeRegistry();
    registry.newTab('ws-ag-a', 0);

    apply(registry, { action: 'break_tab', tab_id: 0, axis: 'h' });

    const workspace = ws(registry);
    expect(paneCount(workspace)).toBe(2);
    expect(workspace.root.type === 'split' && workspace.root.axis).toBe('horizontal');
    expect(focusedAgentId(workspace)).toBe('ag-a');
  });

  it('break_tab: without a tab ID breaks out the focused tab; a pane’s only tab stays', () => {
    const registry = makeRegistry();
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'break_tab' });
    expect(registry.workspace('ws-ag-a')).toEqual(before);

    const empty = registry.newTab('ws-ag-a', 0) as number;
    apply(registry, { action: 'break_tab' });
    const workspace = ws(registry);
    expect(paneCount(workspace)).toBe(2);
    expect(focusedSurface(workspace)?.id).toBe(empty);
  });

  it('get_layout: changes nothing (the daemon answers it from the layout file)', () => {
    const registry = makeRegistry();
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'get_layout' });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('with no workspace on screen and none named, commands are ignored', () => {
    const registry = makeRegistry();
    registry.activeWorkspaceId = null;
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'split' });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('a command naming an unknown workspace is ignored', () => {
    const registry = makeRegistry();
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'split', workspace_id: 'ws-gone' });

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });

  it('a command from another project does not touch the workspace on screen', () => {
    const registry = makeRegistry();
    const before = registry.workspace('ws-ag-a');

    apply(registry, { action: 'split' }, root + '-other');

    expect(registry.workspace('ws-ag-a')).toEqual(before);
  });
});
