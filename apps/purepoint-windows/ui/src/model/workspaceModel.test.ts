// Port of apps/purepoint-macos/purepoint-macosTests/WorkspaceModelTests.swift (WIN-6.1).
// Each title keeps the Swift test function name. Swift's synthesized `==` is `structurallyEqual`.

import {
  makeAgentModel,
  makeWorktreeModel,
  structurallyEqual,
  type AgentModel,
  type WorktreeModel,
} from './workspaceModel';

function agent({
  name = 'Original',
  prompt = '',
  sessionId = null,
}: { name?: string; prompt?: string; sessionId?: string | null } = {}): AgentModel {
  return makeAgentModel({
    id: 'ag-1',
    name,
    agentType: 'claude',
    status: 'running',
    prompt,
    startedAt: '',
    sessionId,
  });
}

function worktree(
  agentModel: AgentModel,
  { name = 'Original', path = '/tmp/wt-1' }: { name?: string; path?: string } = {},
): WorktreeModel {
  return makeWorktreeModel({
    id: 'wt-1',
    name,
    path,
    branch: 'pu/test',
    status: 'active',
    agents: [agentModel],
  });
}

describe('WorkspaceModelTests', () => {
  it('testGivenRootAgentRenameShouldDetectChangedSnapshotWithoutStatusChange', () => {
    // ProjectState.mergeRootAgents uses array equality to decide whether to apply a snapshot.
    expect(structurallyEqual([agent()], [agent({ name: 'Renamed' })])).toBe(false);
  });

  it('testGivenWorktreeAgentRenameShouldDetectChangedSnapshotWithoutStatusChange', () => {
    // Nested agent names must also reach ProjectState.mergeWorktrees.
    expect(structurallyEqual([worktree(agent())], [worktree(agent({ name: 'Renamed' }))])).toBe(
      false,
    );
  });

  it('testGivenUpdatedMetadataShouldDetectChangedSnapshot', () => {
    expect(structurallyEqual(agent(), agent({ prompt: 'Updated prompt' }))).toBe(false);
    expect(structurallyEqual(agent(), agent({ sessionId: 'new-session' }))).toBe(false);
    expect(structurallyEqual(worktree(agent()), worktree(agent(), { name: 'Renamed' }))).toBe(
      false,
    );
    expect(structurallyEqual(worktree(agent()), worktree(agent(), { path: '/tmp/moved' }))).toBe(
      false,
    );
  });

  it('testGivenUnchangedSnapshotShouldAvoidRedundantUpdate', () => {
    expect(structurallyEqual([agent()], [agent()])).toBe(true);
    expect(structurallyEqual([worktree(agent())], [worktree(agent())])).toBe(true);
  });
});
