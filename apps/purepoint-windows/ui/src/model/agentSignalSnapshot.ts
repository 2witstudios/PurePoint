// Port of apps/purepoint-macos/purepoint-macos/Models/AgentSignalSnapshot.swift (WIN-7.7).
//
// The wire facts are the generated `SignalSnapshot` (protocol/generated); this file holds what the
// Mac's `AgentSignalSnapshot` adds to them: the adapter qualification key and the presentation, a
// projection of the producer's current facts that is independent of process liveness.
//
// Differences from Swift:
// - Times are epoch milliseconds (`nowMs`), not `Date`.
// - The daemon decoder (`services/daemonConnection.ts`) accepts any object as a snapshot, where
//   Swift's `try?` drops a malformed one; the presentation therefore reads each field defensively
//   and treats anything malformed as unknown, as a dropped snapshot is.

import type { SignalSnapshot } from '../protocol';

/** What a signal shows (Swift `AgentSignalPresentation`). */
export interface AgentSignalPresentation {
  readonly label: string;
  readonly eventId: string | null;
  readonly needsAttention: boolean;
  readonly detail: string;
}

export const UNKNOWN_SIGNAL_PRESENTATION: AgentSignalPresentation = {
  label: 'Unknown',
  eventId: null,
  needsAttention: false,
  detail: '',
};

function presentation(
  label: string,
  eventId: string | null,
  needsAttention: boolean,
  detail = '',
): AgentSignalPresentation {
  return { label, eventId, needsAttention, detail };
}

/** Swift `accessibilityDescription`: the label, then the detail when there is one. */
export function signalAccessibilityDescription(value: AgentSignalPresentation): string {
  return value.detail === '' ? value.label : `${value.label}, ${value.detail}`;
}

export function presentationsEqual(
  a: AgentSignalPresentation,
  b: AgentSignalPresentation,
): boolean {
  return (
    a.label === b.label &&
    a.eventId === b.eventId &&
    a.needsAttention === b.needsAttention &&
    a.detail === b.detail
  );
}

const utf8 = new TextEncoder();

/**
 * Swift's `JSONEncoder().encode([String]).base64EncodedString()`: an opaque key whose components
 * may contain separators. Swift escapes `/` as `\/`, so keys match the Mac's byte for byte.
 */
export function opaqueKey(components: readonly (string | null)[]): string {
  const json = JSON.stringify(components).replace(/\//g, '\\/');
  let binary = '';
  for (const byte of utf8.encode(json)) binary += String.fromCharCode(byte);
  return btoa(binary);
}

/** Tool releases change often; qualification follows the adapter, not the tool version. */
export function signalQualificationKey(snapshot: SignalSnapshot): string | null {
  const evidence = snapshot.evidence as Partial<Record<string, unknown>> | null;
  if (evidence === null || typeof evidence !== 'object') return null;
  return opaqueKey([String(evidence.tool), String(evidence.adapter_version)]);
}

export interface SignalPresentationOptions {
  /** The agent's session as the manifest knows it; must match the snapshot's when given. */
  readonly sessionId?: string | null;
  readonly available?: boolean;
  readonly nowMs?: number;
}

const isString = (value: unknown): value is string => typeof value === 'string';

/** Swift `AgentSignalSnapshot.presentation(agentID:sessionID:available:now:)`. */
export function signalPresentation(
  snapshot: SignalSnapshot,
  agentId: string,
  options: SignalPresentationOptions = {},
): AgentSignalPresentation {
  const sessionId = options.sessionId ?? null;
  const available = options.available ?? true;
  const nowMs = options.nowMs ?? Date.now();
  if (typeof snapshot !== 'object' || (snapshot as unknown) === null) {
    return UNKNOWN_SIGNAL_PRESENTATION;
  }
  const identity = snapshot.identity as Partial<SignalSnapshot['identity']> | null;
  const observation = snapshot.observation as Partial<SignalSnapshot['observation']> | null;
  const pendingAll = Array.isArray(snapshot.pending_attention) ? snapshot.pending_attention : null;
  if (
    !available ||
    snapshot.schema_version !== 1 ||
    identity === null ||
    typeof identity !== 'object' ||
    observation === null ||
    typeof observation !== 'object' ||
    pendingAll === null
  ) {
    return UNKNOWN_SIGNAL_PRESENTATION;
  }
  const outcome = snapshot.outcome ?? null;
  const evidence = snapshot.evidence ?? null;
  const nativeSessionId = identity.native_session_id;
  const launchIdle =
    nativeSessionId === '' &&
    sessionId === null &&
    snapshot.activity === 'idle' &&
    pendingAll.length === 0 &&
    outcome === null &&
    evidence?.confidence === 'authoritative';
  const received = observation.last_received_at_ms;
  const expires = observation.expires_at_ms;
  if (
    identity.agent_id !== agentId ||
    !isString(identity.launch_id) ||
    identity.launch_id === '' ||
    !isString(nativeSessionId) ||
    (nativeSessionId === '' && !launchIdle) ||
    (sessionId !== null && sessionId !== nativeSessionId) ||
    observation.state !== 'current' ||
    typeof received !== 'number' ||
    typeof expires !== 'number' ||
    nowMs < received ||
    nowMs >= expires
  ) {
    return UNKNOWN_SIGNAL_PRESENTATION;
  }
  const pending = pendingAll.filter(
    (request) =>
      typeof request === 'object' &&
      (request as unknown) !== null &&
      request.confidence === 'authoritative' &&
      isString(request.request_id) &&
      request.request_id !== '' &&
      isString(request.event_id) &&
      request.event_id !== '',
  );
  const request =
    pending.find((candidate) => candidate.kind === 'permission') ??
    pending.find((candidate) => candidate.kind === 'question');
  if (request !== undefined) {
    return presentation(
      request.kind === 'permission' ? 'Permission' : 'Question',
      request.event_id,
      true,
      `${String(pending.length)} pending requests: ` +
        pending.map((candidate) => `${candidate.kind} ${candidate.request_id}`).join(', '),
    );
  }
  // A string, not the generated union: future activities must read as unknown.
  const activity: string = snapshot.activity;
  if (activity === 'working') return presentation('Working', null, false);
  if (activity === 'idle' && outcome === null && evidence?.confidence === 'authoritative') {
    return presentation('Idle', null, false);
  }
  if (
    (activity !== 'idle' && activity !== 'unknown') ||
    outcome === null ||
    outcome.confidence !== 'authoritative' ||
    !isString(outcome.event_id) ||
    outcome.event_id === ''
  ) {
    return UNKNOWN_SIGNAL_PRESENTATION;
  }
  switch (outcome.kind) {
    case 'completed':
      return presentation('Finished', outcome.event_id, false);
    case 'errored':
      return presentation('Error', outcome.event_id, true);
    case 'interrupted':
      return presentation('Stopped', outcome.event_id, false);
    default:
      return UNKNOWN_SIGNAL_PRESENTATION;
  }
}
