// Port of apps/purepoint-macos/purepoint-macosTests/AgentsHubModelsTests.swift (WIN-8.7).
import { describe, expect, it } from 'vitest';
import type { AgentDefInfo, SwarmDefInfo, TemplateInfo } from '../protocol';
import {
  agentDefinitionFromInfo,
  agentPromptSourceModeTitle,
  agentsHubTabTitle,
  makeAgentDefinition,
  makeSavedPrompt,
  makeSwarmDefinition,
  makeSwarmRosterItem,
  parseTags,
  PromptScope,
  savedPromptFromInfo,
  scopeBadge,
  swarmDefinitionFromInfo,
  swarmDefinitionTotalAgents,
} from './agentsHubModels';

describe('AgentsHubModelsTests', () => {
  // MARK: - SavedPrompt

  it('testSavedPromptFromTemplateInfo', () => {
    const info = JSON.parse(
      '{"name":"review","description":"Code review","agent":"claude","source":"local","variables":["BRANCH"]}',
    ) as TemplateInfo;
    const prompt = savedPromptFromInfo(info);

    expect(prompt.id).toBe('local:review');
    expect(prompt.name).toBe('review');
    expect(prompt.description).toBe('Code review');
    expect(prompt.agent).toBe('claude');
    expect(prompt.body).toBe('');
    expect(prompt.source).toBe('local');
    expect(prompt.variables).toEqual(['BRANCH']);
  });

  it('testSavedPromptManualInit', () => {
    const prompt = makeSavedPrompt({
      name: 'test',
      description: 'A test',
      agent: 'claude',
      body: 'Do something.',
      source: 'global',
      variables: ['VAR'],
    });

    expect(prompt.id).toBe('global:test');
    expect(prompt.name).toBe('test');
    expect(prompt.body).toBe('Do something.');
  });

  // MARK: - AgentDefinition

  it('testAgentDefinitionFromAgentDefInfo', () => {
    const info = JSON.parse(
      '{"name":"reviewer","agent_type":"claude","template":"review","inline_prompt":null,"tags":["review"],"scope":"local","available_in_command_dialog":true,"icon":"shield"}',
    ) as AgentDefInfo;
    const def = agentDefinitionFromInfo(info);

    expect(def.id).toBe('local:reviewer');
    expect(def.name).toBe('reviewer');
    expect(def.agentType).toBe('claude');
    expect(def.template).toBe('review');
    expect(def.inlinePrompt).toBeNull();
    expect(def.tags).toEqual(['review']);
    expect(def.scope).toBe('local');
    expect(def.availableInCommandDialog).toBe(true);
    expect(def.icon).toBe('shield');
  });

  it('testAgentDefinitionManualInit', () => {
    const def = makeAgentDefinition({ name: 'test-agent' });

    expect(def.id).toBe('local:test-agent');
    expect(def.agentType).toBe('claude');
    expect(def.template).toBeNull();
    expect(def.tags).toEqual([]);
    expect(def.scope).toBe('local');
    expect(def.availableInCommandDialog).toBe(true);
    expect(def.icon).toBeNull();
  });

  // MARK: - SwarmDefinition

  it('testSwarmDefinitionFromSwarmDefInfo', () => {
    const info = JSON.parse(
      '{"name":"full-stack","worktree_count":3,"worktree_template":"feature","roster":[{"agent_def":"reviewer","role":"review","quantity":2}],"include_terminal":true,"scope":"local"}',
    ) as SwarmDefInfo;
    const swarm = swarmDefinitionFromInfo(info);

    expect(swarm.id).toBe('local:full-stack');
    expect(swarm.name).toBe('full-stack');
    expect(swarm.worktreeCount).toBe(3);
    expect(swarm.worktreeTemplate).toBe('feature');
    expect(swarm.roster).toHaveLength(1);
    expect(swarm.roster[0]?.agentDef).toBe('reviewer');
    expect(swarm.roster[0]?.role).toBe('review');
    expect(swarm.roster[0]?.quantity).toBe(2);
    expect(swarm.includeTerminal).toBe(true);
    expect(swarm.scope).toBe('local');
  });

  it('testSwarmDefinitionTotalAgents', () => {
    const swarm = makeSwarmDefinition({
      name: 'test',
      worktreeCount: 3,
      worktreeTemplate: 'feature',
      roster: [
        makeSwarmRosterItem({ agentDef: 'reviewer', role: 'review', quantity: 2 }),
        makeSwarmRosterItem({ agentDef: 'fixer', role: 'fix', quantity: 1 }),
      ],
      includeTerminal: false,
      scope: 'local',
    });

    expect(swarmDefinitionTotalAgents(swarm)).toBe(9); // 3 * (2 + 1)
  });

  it('testSwarmDefinitionManualInit', () => {
    const swarm = makeSwarmDefinition({ name: 'empty' });

    expect(swarm.id).toBe('local:empty');
    expect(swarm.worktreeCount).toBe(1);
    expect(swarm.roster).toEqual([]);
    expect(swarm.includeTerminal).toBe(false);
    expect(swarm.scope).toBe('local');
  });

  // MARK: - PromptScopeChoice

  it('testPromptScopeChoiceWireValues', () => {
    expect(PromptScope.wireValue('global')).toBe('global');
    expect(PromptScope.wireValue('project')).toBe('local');
  });

  it('testPromptScopeChoiceTitles', () => {
    expect(PromptScope.title('global')).toBe('Global');
    expect(PromptScope.title('project')).toBe('Project');
  });

  // MARK: - AgentPromptSourceMode

  it('testAgentPromptSourceModeTitles', () => {
    expect(agentPromptSourceModeTitle('library')).toBe('From Prompts');
    expect(agentPromptSourceModeTitle('inline')).toBe('Inline');
  });
});

describe('AgentsHubModels (Windows)', () => {
  it('gives each roster item its own id', () => {
    const a = makeSwarmRosterItem({ agentDef: 'x', role: '', quantity: 1 });
    const b = makeSwarmRosterItem({ agentDef: 'x', role: '', quantity: 1 });
    expect(a.id).not.toBe(b.id);
  });

  it('reads a roster entry without a quantity as one agent', () => {
    const swarm = swarmDefinitionFromInfo({
      name: 's',
      worktree_count: 2,
      worktree_template: '',
      roster: [{ agent_def: 'a', role: '' }],
      include_terminal: false,
      scope: 'global',
    });
    expect(swarmDefinitionTotalAgents(swarm)).toBe(2);
  });

  it('titles the hub tabs and scope badges as the Mac does', () => {
    expect(agentsHubTabTitle('prompts')).toBe('Prompts');
    expect(agentsHubTabTitle('triggers')).toBe('Triggers');
    expect(scopeBadge('global', 'repo')).toBe('Global');
    expect(scopeBadge('local', 'repo')).toBe('repo');
    expect(scopeBadge('local', null)).toBe('Project');
  });

  it('splits the tag field on commas and drops blanks', () => {
    expect(parseTags(' review, ,fix ,')).toEqual(['review', 'fix']);
  });
});
