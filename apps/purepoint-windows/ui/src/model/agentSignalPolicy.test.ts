// Port of apps/purepoint-macos/verification/SignalPolicyChecks.swift and
// SignalPendingTapChecks.swift (WIN-7.7), plus the badge-count rules (the Mac's dock tile; the
// iOS-only AgentSignalBadgeTests' permission rules have no Windows counterpart).
import { describe, expect, it } from 'vitest';
import type { AgentSignalPresentation } from './agentSignalSnapshot';
import { UNKNOWN_SIGNAL_PRESENTATION } from './agentSignalSnapshot';
import { AgentSignalAlertPolicy, makeSignalAlertAgent, signalEventKey } from './agentSignalPolicy';
import { AgentSignalPendingTaps } from './agentSignalPendingTaps';

const present = (label: string, eventId: string | null, needsAttention: boolean) =>
  ({ label, eventId, needsAttention, detail: '' }) satisfies AgentSignalPresentation;

const permission = present('Permission', 'request', true);
const qualified = () => true;
const unqualified = () => false;

describe('AgentSignalAlertPolicy', () => {
  const agent = makeSignalAlertAgent({
    identity: 'host/project/agent',
    launch: 'one',
    presentation: permission,
  });

  it('counts distinct agents and withholds unqualified attention alerts', () => {
    const policy = new AgentSignalAlertPolicy();
    expect(policy.project([agent, agent], unqualified).badge).toBe(1);
    expect(policy.project([agent], unqualified).alerts).toHaveLength(0);
  });

  it('alerts once per event, and again for a new launch', () => {
    const policy = new AgentSignalAlertPolicy();
    expect(policy.project([agent], qualified).alerts).toHaveLength(1);
    expect(policy.project([agent], qualified).alerts).toHaveLength(0);
    const unknown = makeSignalAlertAgent({
      identity: agent.identity,
      launch: 'one',
      presentation: UNKNOWN_SIGNAL_PRESENTATION,
    });
    expect(policy.project([unknown], qualified).badge).toBe(0);
    const next = makeSignalAlertAgent({ ...agent, launch: 'two' });
    expect(policy.project([next], qualified).alerts).toHaveLength(1);
    const restored = new AgentSignalAlertPolicy(policy.deliveredKeys);
    expect(restored.project([next], qualified).alerts).toHaveLength(0);
  });

  it('drops an acknowledged error from the badge', () => {
    const error = makeSignalAlertAgent({
      identity: agent.identity,
      launch: 'two',
      presentation: present('Error', 'failure', true),
    });
    const key = signalEventKey(error) as string;
    expect(new AgentSignalAlertPolicy().project([error]).badge).toBe(1);
    const acknowledged = new AgentSignalAlertPolicy().project([error], qualified, new Set([key]));
    expect(acknowledged.badge).toBe(0);
    expect(acknowledged.alerts).toHaveLength(0);
  });

  it('alerts a finished turn without qualification, and never for other states', () => {
    const finished = makeSignalAlertAgent({
      identity: 'a',
      launch: 'one',
      presentation: present('Finished', 'done', false),
    });
    const stopped = makeSignalAlertAgent({
      identity: 'b',
      launch: 'one',
      presentation: present('Stopped', 'halt', false),
    });
    const result = new AgentSignalAlertPolicy().project([finished, stopped], unqualified);
    expect(result.alerts.map((alert) => alert.identity)).toEqual(['a']);
    expect(result.badge).toBe(0);
  });

  it('deduplicates restored keys and keeps at most 4096', () => {
    const keys = Array.from({ length: 5000 }, (_, index) => `k${String(index % 4500)}`);
    const policy = new AgentSignalAlertPolicy(keys);
    expect(policy.deliveredKeys.length).toBeLessThanOrEqual(4096);
    expect(new Set(policy.deliveredKeys).size).toBe(policy.deliveredKeys.length);
  });

  it('evicts the oldest key that is no longer current once full', () => {
    const full = Array.from({ length: 4096 }, (_, index) => `old-${String(index)}`);
    const policy = new AgentSignalAlertPolicy(full);
    expect(policy.project([agent], qualified).alerts).toHaveLength(1);
    expect(policy.deliveredKeys).toHaveLength(4096);
    expect(policy.deliveredKeys[0]).toBe('old-1');
  });

  it('keys events by generation and session as well as launch', () => {
    const destination = {
      projectId: 'p',
      worktreeId: null,
      agentId: 'a',
      launchId: 'one',
      generation: 2,
      nativeSessionId: 's',
      eventId: 'request',
    };
    const scoped = makeSignalAlertAgent({ ...agent, destination });
    const replaced = makeSignalAlertAgent({
      ...agent,
      destination: { ...destination, generation: 3 },
    });
    expect(signalEventKey(scoped)).not.toBe(signalEventKey(replaced));
    expect(signalEventKey(makeSignalAlertAgent({ identity: 'x', launch: 'y' }))).toBeNull();
  });
});

describe('badge count', () => {
  const at = (identity: string, presentation: AgentSignalPresentation) =>
    makeSignalAlertAgent({ identity, launch: 'l', presentation });

  it('counts each agent needing attention once, whatever its request kinds', () => {
    const result = new AgentSignalAlertPolicy().project([
      at('a', permission),
      at('a', present('Question', 'q', true)),
      at('b', present('Question', 'q', true)),
      at('c', present('Error', 'e', true)),
    ]);
    expect(result.badge).toBe(3);
  });

  it('does not count working, idle, finished, stopped, suspended or unknown agents', () => {
    const result = new AgentSignalAlertPolicy().project([
      at('a', present('Working', null, false)),
      at('b', present('Idle', null, false)),
      at('c', present('Finished', 'f', false)),
      at('d', present('Stopped', 's', false)),
      at('e', present('Suspended', null, false)),
      at('f', UNKNOWN_SIGNAL_PRESENTATION),
    ]);
    expect(result.badge).toBe(0);
  });

  it('keeps counting attention that was already alerted', () => {
    const policy = new AgentSignalAlertPolicy();
    policy.project([at('a', permission)], qualified);
    expect(policy.project([at('a', permission)], qualified).badge).toBe(1);
  });
});

describe('AgentSignalPendingTaps', () => {
  const now = 100_000;

  it('retains a tap until its route loads and it completes', () => {
    const taps = new AgentSignalPendingTaps();
    taps.enqueue('event', now);
    taps.enqueue('event', now);
    expect(taps.next(new Set(), now)).toBeNull();
    expect(taps.next(new Set(['event']), now)).toBe('event');
    expect(taps.next(new Set(['event']), now)).toBe('event');
    taps.complete('event');
    expect(taps.next(new Set(['event']), now)).toBeNull();
  });

  it('expires after two minutes and on clock rollback', () => {
    const taps = new AgentSignalPendingTaps();
    taps.enqueue('expired', now);
    expect(taps.next(new Set(['expired']), 220_000)).toBeNull();
    taps.enqueue('future', now);
    expect(taps.next(new Set(['future']), 99_000)).toBeNull();
  });

  it('keeps the newest 16 and refuses empty or oversized keys', () => {
    const taps = new AgentSignalPendingTaps();
    for (let index = 0; index < 20; index++) taps.enqueue(`key-${String(index)}`, now);
    expect(taps.next(new Set(['key-0']), now)).toBeNull();
    expect(taps.next(new Set(['key-19']), now)).toBe('key-19');
    taps.enqueue('', now);
    taps.enqueue('x'.repeat(8193), now);
    expect(taps.next(new Set(['', 'x'.repeat(8193)]), now)).toBeNull();
  });
});
