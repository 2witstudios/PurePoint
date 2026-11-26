// The Agents hub (WIN-8.7) against an in-memory daemon: list, create, spawn and run.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Request, Response, SwarmDefInfo, TemplateInfo, AgentDefInfo } from '../../protocol';
import { AppStateContext } from '../shell/appContext.ts';
import { makeTestApp } from '../shell/testApp.ts';
import { AgentsHubView } from './AgentsHubView.tsx';

const sent: Request[] = [];
let templates: (TemplateInfo & { body: string })[] = [];
let agentDefs: AgentDefInfo[] = [];
let swarmDefs: SwarmDefInfo[] = [];

function reply(request: Request): Response {
  switch (request.type) {
    case 'list_templates':
      return {
        type: 'template_list',
        templates: templates.map((t) => ({
          name: t.name,
          description: t.description,
          agent: t.agent,
          source: t.source,
          variables: t.variables,
          command: t.command ?? null,
        })),
      };
    case 'get_template': {
      const t = templates.find((x) => x.name === request.name);
      return t === undefined
        ? { type: 'error', code: 'NOT_FOUND', message: 'no template' }
        : { type: 'template_detail', ...t };
    }
    case 'save_template':
      templates = [
        ...templates.filter((t) => t.name !== request.name),
        {
          name: request.name,
          description: request.description,
          agent: request.agent,
          source: request.scope,
          variables: [],
          command: request.command ?? null,
          body: request.body,
        },
      ];
      return { type: 'ok' };
    case 'list_agent_defs':
      return { type: 'agent_def_list', agent_defs: agentDefs };
    case 'save_agent_def':
      agentDefs = [
        ...agentDefs,
        {
          name: request.name,
          agent_type: request.agent_type,
          template: request.template ?? null,
          inline_prompt: request.inline_prompt ?? null,
          tags: request.tags ?? [],
          scope: request.scope,
          available_in_command_dialog: request.available_in_command_dialog ?? true,
          icon: null,
          command: request.command ?? null,
        },
      ];
      return { type: 'ok' };
    case 'list_swarm_defs':
      return { type: 'swarm_def_list', swarm_defs: swarmDefs };
    case 'save_swarm_def':
      swarmDefs = [
        ...swarmDefs,
        {
          name: request.name,
          worktree_count: request.worktree_count ?? 1,
          worktree_template: request.worktree_template ?? '',
          roster: request.roster ?? [],
          include_terminal: request.include_terminal ?? false,
          scope: request.scope,
        },
      ];
      return { type: 'ok' };
    case 'spawn':
      return { type: 'spawn_result', worktree_id: null, agent_id: 'ag-1', status: 'running' };
    case 'run_swarm':
      return { type: 'run_swarm_result', spawned_agents: ['ag-2', 'ag-3'] };
    default:
      return { type: 'ok' };
  }
}

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string, args?: { json: Request }) => {
    if (command !== 'daemon_request' || args === undefined) return Promise.resolve(null);
    sent.push(args.json);
    return Promise.resolve(reply(args.json));
  },
}));

function renderHub() {
  const app = makeTestApp();
  app.openProject('C:\\repos\\demo');
  render(
    <AppStateContext.Provider value={app}>
      <AgentsHubView />
    </AppStateContext.Provider>,
  );
  return app;
}

function sheet(): HTMLElement {
  return screen.getByRole('dialog');
}

beforeEach(() => {
  sent.length = 0;
  templates = [];
  agentDefs = [];
  swarmDefs = [];
});

describe('AgentsHubView', () => {
  it('creates a project prompt and spawns from it', async () => {
    renderHub();
    fireEvent.click(screen.getByRole('tab', { name: 'Prompts' }));
    fireEvent.click(await screen.findByRole('button', { name: 'New prompt' }));
    const dialog = within(sheet());
    fireEvent.change(dialog.getByLabelText('Name'), { target: { value: 'greet' } });
    fireEvent.click(dialog.getByRole('radio', { name: 'Project' }));
    fireEvent.change(dialog.getByLabelText('Agent type'), { target: { value: 'terminal' } });
    fireEvent.change(dialog.getByLabelText('Prompt body'), { target: { value: 'echo hi' } });
    fireEvent.click(dialog.getByRole('button', { name: 'Create' }));

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(sent.find((r) => r.type === 'save_template')).toMatchObject({
      project_root: 'C:\\repos\\demo',
      name: 'greet',
      agent: 'terminal',
      scope: 'local',
      body: 'echo hi',
    });
    const detail = await screen.findByTestId('prompt-detail');
    await waitFor(() => {
      expect(within(detail).getByLabelText('Prompt body')).toHaveValue('echo hi');
    });
    expect(screen.getByRole('option', { name: /greet/ })).toHaveTextContent('demo');

    fireEvent.click(within(detail).getByRole('button', { name: 'Spawn' }));
    expect(await screen.findByTestId('hub-spawned')).toHaveTextContent('ag-1');
    expect(sent.find((r) => r.type === 'spawn')).toMatchObject({
      prompt: 'echo hi',
      agent: 'terminal',
      root: true,
    });
  });

  it('hides the prompt source for a terminal agent and sends its command', async () => {
    renderHub();
    fireEvent.click(await screen.findByRole('button', { name: 'New agent' }));
    const dialog = within(sheet());
    fireEvent.change(dialog.getByLabelText('Name'), { target: { value: 'term' } });
    fireEvent.change(dialog.getByLabelText('Agent type'), { target: { value: 'terminal' } });
    expect(dialog.queryByRole('radiogroup', { name: 'Prompt source' })).toBeNull();
    fireEvent.change(dialog.getByLabelText('Command'), { target: { value: 'node x.mjs' } });
    fireEvent.change(dialog.getByLabelText('Tags'), { target: { value: 'a, b' } });
    fireEvent.click(dialog.getByRole('button', { name: 'Create' }));

    await screen.findByTestId('agent-detail');
    expect(sent.find((r) => r.type === 'save_agent_def')).toMatchObject({
      name: 'term',
      agent_type: 'terminal',
      template: null,
      inline_prompt: null,
      command: 'node x.mjs',
      tags: ['a', 'b'],
      scope: 'global',
    });
  });

  it('needs a roster to create a swarm, then runs it', async () => {
    renderHub();
    fireEvent.click(screen.getByRole('tab', { name: 'Swarms' }));
    fireEvent.click(await screen.findByRole('button', { name: 'New swarm' }));
    const dialog = within(sheet());
    fireEvent.change(dialog.getByLabelText('Name'), { target: { value: 'pair' } });
    const create = dialog.getByRole('button', { name: 'Create' });
    expect(create).toBeDisabled();
    fireEvent.click(dialog.getByRole('button', { name: 'Add Agent' }));
    fireEvent.change(dialog.getByLabelText('Agent def 1'), { target: { value: 'term' } });
    fireEvent.change(dialog.getByLabelText('Quantity 1'), { target: { value: '2' } });
    fireEvent.change(dialog.getByLabelText('Worktrees'), { target: { value: '1' } });
    expect(create).toBeEnabled();
    fireEvent.click(create);

    const detail = await screen.findByTestId('swarm-detail');
    expect(detail).toHaveTextContent('1 worktrees · 2 agents total');
    fireEvent.click(within(detail).getByRole('button', { name: 'Run' }));
    expect(await screen.findByTestId('hub-spawned')).toHaveTextContent('ag-2, ag-3');
    expect(sent.find((r) => r.type === 'run_swarm')).toMatchObject({
      project_root: 'C:\\repos\\demo',
      swarm_name: 'pair',
    });
  });

  it('shows a daemon error in the banner until dismissed', async () => {
    renderHub();
    fireEvent.click(await screen.findByRole('button', { name: 'New agent' }));
    templates = [];
    const dialog = within(sheet());
    fireEvent.change(dialog.getByLabelText('Name'), { target: { value: 'x' } });
    fireEvent.change(dialog.getByLabelText('Template name'), { target: { value: 'missing' } });
    fireEvent.click(dialog.getByRole('button', { name: 'Create' }));
    const detail = await screen.findByTestId('agent-detail');
    fireEvent.click(within(detail).getByRole('button', { name: 'Spawn' }));
    const alert = await screen.findByRole('alert');
    expect(alert).toHaveTextContent("template 'missing' not found");
    fireEvent.click(within(alert).getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
