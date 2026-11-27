// Port of apps/purepoint-macos/verification/SignalWireChecks.swift (WIN-7.7): the pinned v1 wire
// fixture's presentation, freshness, identity, authority and outcomes, plus
// `AgentModel.signalPresentation`.
import { describe, expect, it } from 'vitest';
import fixtureText from '../../../../../crates/pu-core/tests/fixtures/signals-status-v1.json?raw';
import type { SignalSnapshot } from '../protocol';
import {
  UNKNOWN_SIGNAL_PRESENTATION,
  opaqueKey,
  signalAccessibilityDescription,
  signalPresentation,
  signalQualificationKey,
} from './agentSignalSnapshot';
import { agentSignalPresentation, makeAgentModel } from './workspaceModel';

interface Fixture {
  agents: { signal: SignalSnapshot }[];
  worktrees: { agents: Record<string, { signal: SignalSnapshot }> }[];
}

const fixture = JSON.parse(fixtureText) as Fixture;
const wire = (fixture.agents[0] as { signal: SignalSnapshot }).signal;
const NOW = 1792195201 * 1000;

function snapshot(changes: Record<string, unknown> = {}): SignalSnapshot {
  return { ...structuredClone(wire), ...changes };
}

const label = (value: SignalSnapshot, agentId = 'ag-root', nowMs = NOW, sessionId?: string) =>
  signalPresentation(value, agentId, { nowMs, sessionId: sessionId ?? null }).label;

describe('signal qualification key', () => {
  const evidence = (toolVersion: string) => ({
    tool: 'pi',
    tool_version: toolVersion,
    adapter_version: '1',
    confidence: 'authoritative',
    event_id: 'e',
  });

  it('encodes the tool and adapter as the Mac does', () => {
    expect(signalQualificationKey(snapshot({ evidence: evidence('1.3.0') }))).toBe(
      'WyJwaSIsIjEiXQ==',
    );
  });

  it('survives tool updates', () => {
    expect(signalQualificationKey(snapshot({ evidence: evidence('9.9.9') }))).toBe(
      signalQualificationKey(snapshot({ evidence: evidence('1.3.0') })),
    );
  });

  it('is absent without evidence', () => {
    expect(signalQualificationKey(snapshot({ evidence: null }))).toBeNull();
  });

  it('escapes slashes and keeps non-ASCII text as Swift JSONEncoder does', () => {
    expect(atob(opaqueKey(['C:/a', null]))).toBe('["C:\\/a",null]');
    expect(opaqueKey(['é'])).toBe(btoa('["\u00c3\u00a9"]'));
  });
});

describe('signal presentation', () => {
  it('presents the current permission request', () => {
    const current = signalPresentation(snapshot(), 'ag-root', { nowMs: NOW });
    expect(current.label).toBe('Permission');
    expect(current.needsAttention).toBe(true);
    expect(current.eventId).toBe('event-permission-1');
    expect(signalAccessibilityDescription(current)).toBe(
      'Permission, 1 pending requests: permission request-1',
    );
  });

  it('is unknown for another agent, session, an unavailable agent or outside its window', () => {
    expect(label(snapshot(), 'other')).toBe('Unknown');
    expect(label(snapshot(), 'ag-root', NOW, 'other')).toBe('Unknown');
    expect(signalPresentation(snapshot(), 'ag-root', { available: false, nowMs: NOW })).toEqual(
      UNKNOWN_SIGNAL_PRESENTATION,
    );
    expect(label(snapshot(), 'ag-root', 1792195199 * 1000)).toBe('Unknown');
    expect(label(snapshot(), 'ag-root', 1792195230 * 1000)).toBe('Unknown');
  });

  it.each(['stale', 'disconnected', 'lost', 'process_dead', 'future_state'])(
    'is unknown while the observation is %s',
    (state) => {
      const changed = snapshot({
        observation: {
          state,
          last_received_at_ms: 1792195200000,
          expires_at_ms: 1792195230000,
        },
      });
      expect(label(changed)).toBe('Unknown');
    },
  );

  it('is unknown for a future schema, no expiry or a malformed snapshot', () => {
    expect(label(snapshot({ schema_version: 2 }))).toBe('Unknown');
    expect(
      label(snapshot({ observation: { state: 'current', last_received_at_ms: 1792195200000 } })),
    ).toBe('Unknown');
    expect(label({ schema_version: 'future malformed' } as unknown as SignalSnapshot)).toBe(
      'Unknown',
    );
    expect(label(snapshot({ pending_attention: null }))).toBe('Unknown');
  });

  it('presents working and authoritative idle', () => {
    expect(label(snapshot({ pending_attention: [] }))).toBe('Working');
    const idle = signalPresentation(
      snapshot({ activity: 'idle', pending_attention: [], outcome: null }),
      'ag-root',
      { nowMs: NOW },
    );
    expect(idle.label).toBe('Idle');
    expect(idle.needsAttention).toBe(false);
  });

  it('claims idle only from authoritative evidence', () => {
    const inferred = snapshot({
      activity: 'idle',
      pending_attention: [],
      outcome: null,
      evidence: {
        tool: 'pi',
        tool_version: '1.3.0',
        adapter_version: '1',
        confidence: 'inferred',
        event_id: 'e',
      },
    });
    expect(label(inferred)).toBe('Unknown');
    expect(
      label(snapshot({ activity: 'idle', pending_attention: [], outcome: null, evidence: null })),
    ).toBe('Unknown');
    expect(label(snapshot({ activity: 'unknown', pending_attention: [], outcome: null }))).toBe(
      'Unknown',
    );
  });

  it('presents an unbound launch as idle only before a session is known', () => {
    const identity = { ...wire.identity, native_session_id: '' };
    const unboundIdle = snapshot({
      identity,
      activity: 'idle',
      pending_attention: [],
      outcome: null,
    });
    expect(label(unboundIdle)).toBe('Idle');
    expect(label(unboundIdle, 'ag-root', NOW, 'known-thread')).toBe('Unknown');
    for (const changes of [
      { activity: 'working', pending_attention: [], outcome: null },
      {},
      {
        activity: 'idle',
        pending_attention: [],
        outcome: { kind: 'completed', event_id: 'outcome', confidence: 'authoritative' },
      },
    ]) {
      expect(label(snapshot({ ...changes, identity }))).toBe('Unknown');
    }
  });

  it.each([
    ['completed', 'Finished', false],
    ['errored', 'Error', true],
    ['interrupted', 'Stopped', false],
    ['future_outcome', 'Unknown', false],
  ])('presents a %s outcome as %s', (kind, expected, attention) => {
    const value = signalPresentation(
      snapshot({
        activity: 'idle',
        pending_attention: [],
        outcome: { kind, event_id: 'outcome', confidence: 'authoritative' },
      }),
      'ag-root',
      { nowMs: NOW },
    );
    expect(value.label).toBe(expected);
    expect(value.needsAttention).toBe(attention);
  });

  it.each(['heuristic', 'source_confirmed', 'future_confidence'])(
    'ignores a %s outcome',
    (confidence) => {
      const value = snapshot({
        activity: 'idle',
        pending_attention: [],
        outcome: { kind: 'errored', event_id: 'outcome', confidence },
      });
      expect(label(value)).toBe('Unknown');
    },
  );

  it('prefers a permission request over a question', () => {
    const question = { ...wire.pending_attention[0], kind: 'question', request_id: 'q' };
    const value = snapshot({ pending_attention: [question, wire.pending_attention[0]] });
    expect(label(value)).toBe('Permission');
    expect(label(snapshot({ pending_attention: [question] }))).toBe('Question');
  });

  it('presents the worktree agent of the fixture as finished', () => {
    const worker = fixture.worktrees[0]?.agents['ag-worker']?.signal as SignalSnapshot;
    expect(label(worker, 'ag-worker')).toBe('Finished');
  });
});

describe('AgentModel.signalPresentation', () => {
  const base = {
    id: 'ag-root',
    name: 'Root',
    agentType: 'claude',
    status: 'running' as const,
    prompt: '',
    startedAt: '',
  };

  it('is Suspended for a suspended agent, unknown when unavailable', () => {
    const suspended = makeAgentModel({ ...base, id: 'suspended', suspended: true });
    expect(agentSignalPresentation(suspended, true, NOW).label).toBe('Suspended');
    expect(agentSignalPresentation(suspended, false, NOW)).toEqual(UNKNOWN_SIGNAL_PRESENTATION);
  });

  it('follows the signal while alive', () => {
    const idle = snapshot({ activity: 'idle', pending_attention: [], outcome: null });
    const agent = makeAgentModel({ ...base, signal: idle });
    expect(agentSignalPresentation(agent, true, NOW).label).toBe('Idle');
    const working = makeAgentModel({ ...base, signal: snapshot({ pending_attention: [] }) });
    expect(agentSignalPresentation(working, true, NOW).label).toBe('Working');
  });

  it('is unknown with no signal, or once the process has exited', () => {
    expect(agentSignalPresentation(makeAgentModel(base), true, NOW).label).toBe('Unknown');
    const broken = makeAgentModel({ ...base, status: 'broken', signal: snapshot() });
    expect(agentSignalPresentation(broken, true, NOW).label).toBe('Unknown');
  });

  it('checks the manifest session against the signal', () => {
    const agent = makeAgentModel({ ...base, sessionId: 'native-root', signal: snapshot() });
    expect(agentSignalPresentation(agent, true, NOW).label).toBe('Permission');
    const replaced = makeAgentModel({ ...base, sessionId: 'replacement', signal: snapshot() });
    expect(agentSignalPresentation(replaced, true, NOW).label).toBe('Unknown');
  });
});
