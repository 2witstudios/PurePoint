// Port of apps/purepoint-macos/verification/SignalRoutingChecks.swift (WIN-7.7): scoped routing
// by host, project and worktree, duplicate IDs, ambiguous workspaces and presence.
import { describe, expect, it } from 'vitest';
import type { SignalSnapshot } from '../protocol';
import { AgentSignalAlertPolicy, makeSignalAlertAgent, signalEventKey } from './agentSignalPolicy';
import {
  type AgentSignalDestination,
  type SignalPresence,
  type SignalRoutingTarget,
  destinationMatches,
  isSignalVisible,
  signalRoutingTarget,
} from './agentSignalRouting';

const scope: AgentSignalDestination = {
  projectId: 'machine:remote:/repo',
  worktreeId: 'wt',
  agentId: 'same',
  launchId: 'launch',
  generation: 2,
  nativeSessionId: 'native',
  eventId: 'event',
};

const target = (init: Partial<SignalRoutingTarget>): SignalRoutingTarget => ({
  projectId: '/repo',
  worktreeId: 'wt',
  workspaceId: 'local',
  agentId: 'same',
  leafId: 0,
  surfaceId: 0,
  workspaceOrdinal: 0,
  ...init,
});

const local = target({});
const remote = target({
  projectId: 'machine:remote:/repo',
  workspaceId: 'remote',
  leafId: 1,
  surfaceId: 9,
});

const presence = (changes: Partial<SignalPresence> = {}): SignalPresence => ({
  appActive: true,
  keyWindow: true,
  settingsVisible: false,
  selectedWorkspaceId: 'remote',
  activeWorkspaceId: 'remote',
  focusedAgentId: 'same',
  activeProjectId: scope.projectId,
  ...changes,
});

function snapshot(generation = 2, session = 'native', worktree: string | null = 'wt') {
  return {
    schema_version: 1,
    identity: {
      agent_id: 'same',
      worktree_id: worktree,
      launch_id: 'launch',
      generation,
      native_session_id: session,
    },
    activity: 'idle',
    pending_attention: [],
    outcome: { kind: 'errored', event_id: 'event', turn_id: null, confidence: 'authoritative' },
    observation: { state: 'current', last_received_at_ms: 99_000, expires_at_ms: 101_000 },
    evidence: null,
  } satisfies SignalSnapshot;
}

describe('signalRoutingTarget', () => {
  it('routes to the one pane of that host, project and worktree', () => {
    expect(signalRoutingTarget(scope, [local, remote])).toEqual(remote);
    expect(signalRoutingTarget(scope, [local])).toBeNull();
    expect(signalRoutingTarget(scope, [remote, remote])).toBeNull();
  });

  it('refuses a workspace ID another project or worktree also claims', () => {
    const collision = target({ workspaceId: 'remote', agentId: 'other' });
    expect(signalRoutingTarget(scope, [collision, remote])).toBeNull();
    const sameProjectCollision = target({
      projectId: scope.projectId,
      worktreeId: 'other-worktree',
      workspaceId: 'remote',
      agentId: 'other',
      workspaceOrdinal: 1,
    });
    expect(signalRoutingTarget(scope, [sameProjectCollision, remote])).toBeNull();
  });
});

describe('isSignalVisible', () => {
  it('is visible only in the focused pane of the selected, active workspace', () => {
    expect(isSignalVisible(presence(), scope)).toBe(true);
    expect(isSignalVisible(presence({ settingsVisible: true }), scope)).toBe(false);
    expect(isSignalVisible(presence({ selectedWorkspaceId: null }), scope)).toBe(false);
    expect(isSignalVisible(presence({ appActive: false, keyWindow: false }), scope)).toBe(false);
    expect(isSignalVisible(presence({ focusedAgentId: 'other' }), scope)).toBe(false);
    expect(isSignalVisible(presence({ activeProjectId: '/repo' }), scope)).toBe(false);
  });
});

describe('destinationMatches', () => {
  const now = 100_000;

  it('matches only the same generation, session, worktree and a fresh event', () => {
    expect(destinationMatches(scope, snapshot(), now)).toBe(true);
    expect(destinationMatches(scope, snapshot(3), now)).toBe(false);
    expect(destinationMatches(scope, snapshot(2, 'replacement'), now)).toBe(false);
    expect(destinationMatches(scope, snapshot(2, 'native', null), now)).toBe(false);
    expect(destinationMatches(scope, snapshot(), 101_000)).toBe(false);
    expect(destinationMatches(scope, null, now)).toBe(false);
  });

  it('keeps a hidden error on the badge and keys a replacement launch apart', () => {
    const error = makeSignalAlertAgent({
      identity: 'scoped',
      launch: 'launch',
      presentation: { label: 'Error', eventId: 'event', needsAttention: true, detail: '' },
      destination: scope,
    });
    const hidden = isSignalVisible(presence({ settingsVisible: true }), scope);
    const key = signalEventKey(error) as string;
    const policy = new AgentSignalAlertPolicy();
    expect(policy.project([error], () => false, new Set(hidden ? [key] : [])).badge).toBe(1);
    const replacement = makeSignalAlertAgent({
      ...error,
      destination: { ...scope, generation: 3 },
    });
    expect(signalEventKey(replacement)).not.toBe(key);
  });
});
