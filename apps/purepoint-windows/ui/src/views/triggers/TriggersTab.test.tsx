// The Agents hub's Triggers tab (WIN-8.9) against an in-memory daemon.
import { fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Request, Response, TriggerInfo } from '../../protocol';
import { AgentsHubView } from '../agents/AgentsHubView.tsx';
import { AppStateContext } from '../shell/appContext.ts';
import { makeTestApp } from '../shell/testApp.ts';

const sent: Request[] = [];
let triggers: TriggerInfo[] = [];

function reply(request: Request): Response {
  switch (request.type) {
    case 'list_triggers':
      return { type: 'trigger_list', triggers };
    case 'save_trigger':
      triggers = [
        ...triggers,
        {
          name: request.name,
          description: request.description ?? null,
          on: request.on,
          sequence: request.sequence,
          variables: request.variables ?? {},
          scope: request.scope,
        },
      ];
      return { type: 'ok' };
    case 'delete_trigger':
      triggers = triggers.filter((t) => t.name !== request.name);
      return { type: 'ok' };
    case 'list_templates':
      return { type: 'template_list', templates: [] };
    case 'list_agent_defs':
      return { type: 'agent_def_list', agent_defs: [] };
    case 'list_swarm_defs':
      return { type: 'swarm_def_list', swarm_defs: [] };
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

function renderTriggers() {
  const app = makeTestApp();
  app.openProject('C:\\repos\\demo');
  render(
    <AppStateContext.Provider value={app}>
      <AgentsHubView />
    </AppStateContext.Provider>,
  );
  fireEvent.click(screen.getByRole('tab', { name: 'Triggers' }));
}

beforeEach(() => {
  sent.length = 0;
  triggers = [];
});

describe('TriggersTab', () => {
  it('creates a project trigger with a gated step and shows it', async () => {
    renderTriggers();
    fireEvent.click(await screen.findByRole('button', { name: 'New trigger' }));
    const dialog = within(screen.getByRole('dialog'));
    const create = dialog.getByRole('button', { name: 'Create' });
    expect(create).toBeDisabled();
    fireEvent.change(dialog.getByLabelText('Name'), { target: { value: 'idle-nudge' } });
    fireEvent.click(dialog.getByRole('radio', { name: 'Pre-Push' }));
    fireEvent.change(dialog.getByLabelText('Scope'), { target: { value: 'local' } });
    fireEvent.change(dialog.getByLabelText('Inject text 1'), { target: { value: 'continue' } });
    fireEvent.click(dialog.getByRole('button', { name: 'Add Step' }));
    fireEvent.change(dialog.getByLabelText('Gate command 2'), { target: { value: 'npm test' } });
    fireEvent.change(dialog.getByLabelText('Exit 2'), { target: { value: '0' } });
    fireEvent.change(dialog.getByLabelText('Retries 2'), { target: { value: '2' } });
    expect(screen.getByTestId('trigger-preview')).toHaveTextContent(
      'On Pre-Push → 2 steps (local)',
    );
    fireEvent.click(create);

    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(sent.find((r) => r.type === 'save_trigger')).toEqual({
      type: 'save_trigger',
      project_root: 'C:\\repos\\demo',
      name: 'idle-nudge',
      description: null,
      on: 'pre_push',
      sequence: [
        { inject: 'continue', gate: null, max_retries: null },
        { inject: null, gate: { run: 'npm test', expect_exit: 0 }, max_retries: 2 },
      ],
      variables: {},
      scope: 'local',
    });
    const row = await screen.findByRole('option', { name: /idle-nudge/ });
    expect(row).toHaveTextContent('Pre-Push · 2 steps');
    expect(row).toHaveTextContent('demo');

    fireEvent.click(row);
    const detail = await screen.findByTestId('trigger-detail');
    expect(detail).toHaveTextContent('Fires on Pre-Push');
    expect(within(detail).getAllByTestId('trigger-step-row')).toHaveLength(2);
    expect(detail).toHaveTextContent('exit=0');
    expect(detail).toHaveTextContent('max retries: 2');
    expect(detail).toHaveTextContent('No running agents to assign this trigger to.');

    fireEvent.click(within(detail).getByRole('button', { name: 'Delete Trigger' }));
    await waitFor(() => {
      expect(screen.queryByRole('option', { name: /idle-nudge/ })).toBeNull();
    });
    expect(sent.find((r) => r.type === 'delete_trigger')).toMatchObject({
      name: 'idle-nudge',
      scope: 'local',
      project_root: 'C:\\repos\\demo',
    });
  });
});
