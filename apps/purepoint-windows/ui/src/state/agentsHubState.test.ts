// Port of apps/purepoint-macos/purepoint-macosTests/AgentsHubStateTests.swift (WIN-8.7), plus the
// daemon round trips the Swift suite leaves to manual testing.
import { describe, expect, it } from 'vitest';
import type { Request, Response } from '../protocol';
import {
  makeAgentDefinition,
  makeSavedPrompt,
  makeSwarmDefinition,
  makeSwarmRosterItem,
} from '../model/agentsHubModels';
import { AgentsHubState } from './agentsHubState';

const idle = { send: () => Promise.resolve<Response>({ type: 'ok' }) };

function scripted(reply: (request: Request) => Response) {
  const sent: Request[] = [];
  return {
    sent,
    client: {
      send: (request: Request) => {
        sent.push(request);
        return Promise.resolve(reply(request));
      },
    },
  };
}

describe('AgentsHubStateTests', () => {
  it('testInitialStateIsEmpty', () => {
    const state = new AgentsHubState(idle);

    expect(state.prompts).toEqual([]);
    expect(state.agents).toEqual([]);
    expect(state.swarms).toEqual([]);
    expect(state.selectedPromptId).toBeNull();
    expect(state.selectedAgentId).toBeNull();
    expect(state.selectedSwarmId).toBeNull();
    expect(state.showingCreatePrompt).toBe(false);
    expect(state.showingCreateAgent).toBe(false);
    expect(state.showingCreateSwarm).toBe(false);
  });

  it('testSelectedPromptReturnsNilWhenEmpty', () => {
    const state = new AgentsHubState(idle);

    expect(state.selectedPrompt).toBeNull();
  });

  it('testSelectedPromptReturnsMatchingPrompt', () => {
    const state = new AgentsHubState(idle);
    state.prompts = [
      makeSavedPrompt({
        name: 'review',
        description: 'Review',
        agent: 'claude',
        body: '',
        source: 'local',
        variables: [],
      }),
      makeSavedPrompt({
        name: 'deploy',
        description: 'Deploy',
        agent: 'claude',
        body: '',
        source: 'global',
        variables: [],
      }),
    ];
    state.selectedPromptId = 'global:deploy';

    expect(state.selectedPrompt?.name).toBe('deploy');
  });

  it('testSelectedAgentReturnsMatchingAgent', () => {
    const state = new AgentsHubState(idle);
    state.agents = [
      makeAgentDefinition({ name: 'reviewer' }),
      makeAgentDefinition({ name: 'fixer' }),
    ];
    state.selectedAgentId = 'local:fixer';

    expect(state.selectedAgent?.name).toBe('fixer');
  });

  it('testSelectedSwarmReturnsMatchingSwarm', () => {
    const state = new AgentsHubState(idle);
    state.swarms = [
      makeSwarmDefinition({ name: 'full-stack' }),
      makeSwarmDefinition({ name: 'lite' }),
    ];
    state.selectedSwarmId = 'local:lite';

    expect(state.selectedSwarm?.name).toBe('lite');
  });

  it('testSelectedPromptReturnsNilForMismatchId', () => {
    const state = new AgentsHubState(idle);
    state.prompts = [
      makeSavedPrompt({
        name: 'review',
        description: 'Review',
        agent: 'claude',
        body: '',
        source: 'local',
        variables: [],
      }),
    ];
    state.selectedPromptId = 'nonexistent';

    expect(state.selectedPrompt).toBeNull();
  });
});

describe('AgentsHubState (daemon)', () => {
  it('merges each list across roots, keeping the first copy of a global entry', async () => {
    const { client } = scripted((request) => {
      const root = ('project_root' in request ? request.project_root : null) ?? '';
      switch (request.type) {
        case 'list_templates':
          return {
            type: 'template_list',
            templates: [
              { name: 'shared', description: '', agent: '', source: 'global', variables: [] },
              {
                name: `own-${root.slice(-1)}`,
                description: '',
                agent: '',
                source: 'local',
                variables: [],
              },
            ],
          };
        case 'list_agent_defs':
          return { type: 'agent_def_list', agent_defs: [] };
        case 'list_swarm_defs':
          return {
            type: 'swarm_def_list',
            swarm_defs: [
              {
                name: 'swarm',
                worktree_count: 1,
                worktree_template: '',
                roster: [],
                include_terminal: false,
                scope: 'local',
              },
            ],
          };
        default:
          return { type: 'ok' };
      }
    });
    const state = new AgentsHubState(client);
    await state.loadAll(['C:\\repos\\a', 'C:\\repos\\b']);

    expect(state.prompts.map((p) => p.id)).toEqual(['local:own-a', 'local:own-b', 'global:shared']);
    expect(state.prompts.find((p) => p.name === 'own-b')?.projectName).toBe('b');
    expect(state.prompts.find((p) => p.name === 'shared')?.projectRoot).toBeNull();
    // Two project swarms of the same name: the first root's wins.
    expect(state.swarms).toHaveLength(1);
    expect(state.swarms[0]?.projectRoot).toBe('C:\\repos\\a');
    expect(state.selectedPromptId).toBe('local:own-a');
    expect(state.isLoading).toBe(false);
    expect(state.error).toBeNull();
  });

  it('fills in a prompt body after saving it', async () => {
    const saved: Request[] = [];
    const { client } = scripted((request) => {
      switch (request.type) {
        case 'save_template':
          saved.push(request);
          return { type: 'ok' };
        case 'list_templates':
          return {
            type: 'template_list',
            templates: [
              { name: 'p', description: 'd', agent: 'terminal', source: 'local', variables: [] },
            ],
          };
        case 'get_template':
          return {
            type: 'template_detail',
            name: 'p',
            description: 'd',
            agent: 'terminal',
            body: 'echo hi',
            source: 'local',
            variables: [],
            command: 'echo hi',
          };
        default:
          return { type: 'ok' };
      }
    });
    const state = new AgentsHubState(client);
    await state.saveTemplate({
      projectRoot: '/p',
      projectRoots: ['/p'],
      name: 'p',
      description: 'd',
      agent: 'terminal',
      body: 'echo hi',
      scope: 'local',
      command: 'echo hi',
    });

    expect(saved[0]).toMatchObject({ scope: 'local', command: 'echo hi', agent: 'terminal' });
    expect(state.selectedPrompt?.body).toBe('echo hi');
    expect(state.selectedPrompt?.command).toBe('echo hi');
  });

  it('reports a refused save instead of dropping it', async () => {
    const { client } = scripted(() => ({ type: 'error', code: 'BAD', message: 'no scope' }));
    const state = new AgentsHubState(client);
    await state.saveAgentDef({
      projectRoot: '/p',
      projectRoots: ['/p'],
      def: makeAgentDefinition({ name: 'x' }),
    });
    expect(state.error).toBe('Failed to save agent def: no scope');
  });

  it('sends a swarm roster as the daemon payload and reports what a run started', async () => {
    const { client, sent } = scripted((request) => {
      if (request.type === 'run_swarm') {
        return { type: 'run_swarm_result', spawned_agents: ['ag-1', 'ag-2'] };
      }
      return { type: 'swarm_def_list', swarm_defs: [] };
    });
    const state = new AgentsHubState(client);
    await state.saveSwarmDef({
      projectRoot: '/p',
      projectRoots: ['/p'],
      def: makeSwarmDefinition({
        name: 'pair',
        roster: [makeSwarmRosterItem({ agentDef: 'term', role: 'run', quantity: 2 })],
      }),
    });
    expect(sent[0]).toEqual({
      type: 'save_swarm_def',
      project_root: '/p',
      name: 'pair',
      worktree_count: 1,
      worktree_template: '',
      roster: [{ agent_def: 'term', role: 'run', quantity: 2 }],
      include_terminal: false,
      scope: 'local',
    });
    expect(await state.runSwarm('/p', 'pair')).toEqual(['ag-1', 'ag-2']);
    expect(state.store.getState().lastSpawnedAgents).toEqual(['ag-1', 'ag-2']);
  });

  it('reports a swarm that stopped part way', async () => {
    const { client } = scripted(() => ({
      type: 'run_swarm_partial',
      spawned_agents: ['ag-1'],
      error_code: 'NOT_FOUND',
      error_message: 'agent def missing',
    }));
    const state = new AgentsHubState(client);
    expect(await state.runSwarm('/p', 'pair')).toEqual(['ag-1']);
    expect(state.error).toBe('Failed to run swarm: agent def missing');
  });

  it('spawns from a prompt with its body, agent type and command', async () => {
    const { client, sent } = scripted((request) => {
      if (request.type === 'get_template') {
        return {
          type: 'template_detail',
          name: 'p',
          description: '',
          agent: 'terminal',
          body: 'echo from-prompt',
          source: 'local',
          variables: [],
          command: null,
        };
      }
      return { type: 'spawn_result', worktree_id: null, agent_id: 'ag-9', status: 'running' };
    });
    const state = new AgentsHubState(client);
    const prompt = makeSavedPrompt({
      name: 'p',
      description: '',
      agent: 'terminal',
      body: '',
      source: 'local',
      variables: [],
    });
    expect(await state.spawnFromPrompt('/p', prompt)).toBe('ag-9');
    expect(sent.at(-1)).toEqual({
      type: 'spawn',
      project_root: '/p',
      prompt: 'echo from-prompt',
      agent: 'terminal',
      root: true,
      command: null,
    });
  });

  it("spawns an agent definition from its template's body", async () => {
    const { client, sent } = scripted((request) => {
      if (request.type === 'get_template') {
        return {
          type: 'template_detail',
          name: 'tpl',
          description: '',
          agent: '',
          body: 'body text',
          source: 'global',
          variables: [],
          command: 'node run.mjs',
        };
      }
      return { type: 'spawn_result', worktree_id: null, agent_id: 'ag-3', status: 'running' };
    });
    const state = new AgentsHubState(client);
    const def = makeAgentDefinition({ name: 'd', agentType: 'terminal', template: 'tpl' });
    expect(await state.spawnAgentDef('/p', def)).toBe('ag-3');
    expect(sent.at(-1)).toMatchObject({
      prompt: 'body text',
      agent: 'terminal',
      command: 'node run.mjs',
    });
  });

  it('deletes with the definition scope and reloads that list', async () => {
    const { client, sent } = scripted((request) =>
      request.type === 'list_agent_defs'
        ? { type: 'agent_def_list', agent_defs: [] }
        : { type: 'ok' },
    );
    const state = new AgentsHubState(client);
    await state.deleteAgentDef({
      projectRoot: '/p',
      projectRoots: ['/p'],
      name: 'x',
      scope: 'global',
    });
    expect(sent.map((r) => r.type)).toEqual(['delete_agent_def', 'list_agent_defs']);
    expect(sent[0]).toMatchObject({ scope: 'global', name: 'x' });
  });
});
