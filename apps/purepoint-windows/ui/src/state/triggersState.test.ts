// TriggersState (WIN-8.9). The Mac has no TriggersState suite; these cover the Swift behaviour
// (merge, save, delete, selection) and the Windows additions (error replies, assign_trigger).
import { describe, expect, it } from 'vitest';
import type { Request, Response, TriggerInfo } from '../protocol';
import {
  buildTriggerSequence,
  EMPTY_STEP,
  stepCountLabel,
  triggerItemFromInfo,
  triggerPreviewText,
  TriggerEvents,
} from '../model/triggerItem';
import { TriggersState } from './triggersState';

function info(fields: Partial<TriggerInfo> & { name: string }): TriggerInfo {
  return {
    description: null,
    on: 'agent_idle',
    sequence: [{ inject: 'next' }],
    variables: {},
    scope: 'local',
    ...fields,
  };
}

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

describe('TriggerItem', () => {
  it('maps the wire event and falls back to Agent Idle', () => {
    expect(triggerItemFromInfo(info({ name: 'a', on: 'pre_push' })).event).toBe('pre_push');
    expect(triggerItemFromInfo(info({ name: 'a', on: 'someday' })).event).toBe('agent_idle');
  });

  it('labels each event as the Mac does', () => {
    expect(TriggerEvents.label('agent_idle')).toBe('Agent Idle');
    expect(TriggerEvents.label('pre_commit')).toBe('Pre-Commit');
    expect(TriggerEvents.label('pre_push')).toBe('Pre-Push');
    expect(TriggerEvents.icon('agent_idle')).toBe('moon.zzz');
    expect(TriggerEvents.icon('pre_commit')).toBe('checkmark.circle');
    expect(TriggerEvents.icon('pre_push')).toBe('arrow.up.circle');
  });

  it('places a project trigger in its root and leaves a global one unplaced', () => {
    const local = triggerItemFromInfo(info({ name: 'a' }), 'C:\\repos\\demo');
    expect(local).toMatchObject({
      id: 'local:a',
      projectRoot: 'C:\\repos\\demo',
      projectName: 'demo',
    });
    const global = triggerItemFromInfo(info({ name: 'a', scope: 'global' }), 'C:\\repos\\demo');
    expect(global).toMatchObject({ id: 'global:a', projectRoot: null, projectName: null });
  });

  it('builds the sequence from the step drafts, leaving blanks out', () => {
    expect(
      buildTriggerSequence([
        { inject: 'keep going', gateRun: '', gateExitCode: '', maxRetries: '' },
        { inject: '', gateRun: 'npm test', gateExitCode: '0', maxRetries: '3' },
        { inject: '', gateRun: 'lint', gateExitCode: 'x', maxRetries: '-1' },
      ]),
    ).toEqual([
      { inject: 'keep going', gate: null, max_retries: null },
      { inject: null, gate: { run: 'npm test', expect_exit: 0 }, max_retries: 3 },
      { inject: null, gate: { run: 'lint', expect_exit: null }, max_retries: null },
    ]);
  });

  it('previews the trigger as the sheet does', () => {
    expect(triggerPreviewText('agent_idle', 1, 'global')).toBe('On Agent Idle → 1 step (global)');
    expect(triggerPreviewText('pre_push', 2, 'local')).toBe('On Pre-Push → 2 steps (local)');
    expect(stepCountLabel(0)).toBe('0 steps');
  });
});

describe('TriggersState', () => {
  it('starts empty with nothing selected', () => {
    const state = new TriggersState({ send: () => Promise.resolve({ type: 'ok' }) });
    expect(state.triggers).toEqual([]);
    expect(state.selectedTrigger).toBeNull();
    expect(state.showingCreationSheet).toBe(false);
  });

  it('merges triggers across roots by scope and name, sorted by name', async () => {
    const { client } = scripted((request) =>
      request.type === 'list_triggers'
        ? {
            type: 'trigger_list',
            triggers: [
              info({ name: 'zeta', scope: 'global' }),
              info({ name: `own-${request.project_root.slice(-1)}` }),
            ],
          }
        : { type: 'ok' },
    );
    const state = new TriggersState(client);
    await state.loadTriggers(['C:\\a', 'C:\\b']);
    expect(state.triggers.map((t) => t.id)).toEqual(['local:own-a', 'local:own-b', 'global:zeta']);
    expect(state.isLoading).toBe(false);
    state.select('global:zeta');
    expect(state.selectedTrigger?.name).toBe('zeta');
    state.select('missing');
    expect(state.selectedTrigger).toBeNull();
  });

  it('saves a trigger then reloads the list', async () => {
    let saved: TriggerInfo[] = [];
    const { client, sent } = scripted((request) => {
      if (request.type === 'save_trigger') {
        saved = [info({ name: request.name, on: request.on, sequence: request.sequence })];
        return { type: 'ok' };
      }
      return { type: 'trigger_list', triggers: saved };
    });
    const state = new TriggersState(client);
    await state.saveTrigger({
      projectRoot: '/p',
      projectRoots: ['/p'],
      name: 'idle-nudge',
      description: null,
      on: 'agent_idle',
      sequence: buildTriggerSequence([{ ...EMPTY_STEP, inject: 'continue' }]),
      variables: {},
      scope: 'local',
    });
    expect(sent[0]).toEqual({
      type: 'save_trigger',
      project_root: '/p',
      name: 'idle-nudge',
      description: null,
      on: 'agent_idle',
      sequence: [{ inject: 'continue', gate: null, max_retries: null }],
      variables: {},
      scope: 'local',
    });
    expect(state.triggers.map((t) => t.name)).toEqual(['idle-nudge']);
  });

  it('clears the selection when the selected trigger is deleted', async () => {
    const { client } = scripted(() => ({ type: 'trigger_list', triggers: [] }));
    const state = new TriggersState(client);
    state.select('local:gone');
    await state.deleteTrigger({
      projectRoot: '/p',
      projectRoots: ['/p'],
      name: 'gone',
      scope: 'local',
    });
    expect(state.selectedTriggerId).toBeNull();
  });

  it('reports a refused save', async () => {
    const { client } = scripted(() => ({ type: 'error', code: 'BAD', message: 'bad event' }));
    const state = new TriggersState(client);
    await state.saveTrigger({
      projectRoot: '/p',
      projectRoots: ['/p'],
      name: 'x',
      description: null,
      on: 'nope',
      sequence: [],
      variables: {},
      scope: 'global',
    });
    expect(state.error).toBe('Failed to save trigger: bad event');
  });

  it('assigns a trigger to an agent', async () => {
    const { client, sent } = scripted(() => ({
      type: 'assign_trigger_result',
      agent_id: 'ag-1',
      trigger_name: 'idle-nudge',
      sequence_len: 2,
    }));
    const state = new TriggersState(client);
    const ok = await state.assignTrigger({
      projectRoot: '/p',
      agentId: 'ag-1',
      triggerName: 'idle-nudge',
    });
    expect(ok).toBe(true);
    expect(sent[0]).toEqual({
      type: 'assign_trigger',
      project_root: '/p',
      agent_id: 'ag-1',
      trigger_name: 'idle-nudge',
    });
    expect(state.store.getState().lastAssignment).toEqual({
      agentId: 'ag-1',
      triggerName: 'idle-nudge',
      sequenceLength: 2,
    });
  });

  it('reports a refused assignment', async () => {
    const { client } = scripted(() => ({ type: 'error', code: 'NOT_FOUND', message: 'no agent' }));
    const state = new TriggersState(client);
    expect(await state.assignTrigger({ projectRoot: '/p', agentId: 'x', triggerName: 't' })).toBe(
      false,
    );
    expect(state.error).toBe('Failed to assign trigger: no agent');
  });
});
