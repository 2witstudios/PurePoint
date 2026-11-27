// Port of apps/purepoint-macos/purepoint-macos/Models/AgentSignalRouting.swift (WIN-7.7).
//
// Where a notification leads: the exact launch, session and event it was posted for
// (`AgentSignalDestination`), the one pane that holds that agent (`signalRoutingTarget`), and
// whether the agent is already on screen (`isSignalVisible`, Swift `AgentSignalPresence`).

import type { SignalSnapshot } from '../protocol';
import { signalPresentation } from './agentSignalSnapshot.ts';

export interface AgentSignalDestination {
  /** `ProjectKey` id: includes the daemon machine identity for remote roots. */
  readonly projectId: string;
  readonly worktreeId: string | null;
  readonly agentId: string;
  readonly launchId: string;
  readonly generation: number;
  readonly nativeSessionId: string;
  readonly eventId: string;
}

/** Whether a snapshot is still the launch, session and event the destination was made for. */
export function destinationMatches(
  destination: AgentSignalDestination,
  snapshot: SignalSnapshot | null | undefined,
  nowMs: number = Date.now(),
): boolean {
  if (snapshot === null || snapshot === undefined) return false;
  const identity = snapshot.identity as SignalSnapshot['identity'] | null | undefined;
  if (identity === null || identity === undefined) return false;
  return (
    identity.agent_id === destination.agentId &&
    identity.worktree_id === destination.worktreeId &&
    identity.launch_id === destination.launchId &&
    identity.generation === destination.generation &&
    identity.native_session_id === destination.nativeSessionId &&
    signalPresentation(snapshot, destination.agentId, {
      sessionId: destination.nativeSessionId,
      nowMs,
    }).eventId === destination.eventId
  );
}

/** A pane that holds an agent (Swift `AgentSignalRouting.Target`). */
export interface SignalRoutingTarget {
  readonly projectId: string;
  readonly worktreeId: string | null;
  readonly workspaceId: string;
  readonly agentId: string | null;
  readonly leafId: number;
  readonly surfaceId: number;
  readonly workspaceOrdinal: number;
}

/**
 * The single pane holding the destination's agent, or null when none or several do, or when the
 * workspace ID is claimed by more than one project, worktree or ordinal (navigation is ID-based,
 * so a cross-project collision is refused).
 */
export function signalRoutingTarget(
  destination: AgentSignalDestination,
  targets: readonly SignalRoutingTarget[],
): SignalRoutingTarget | null {
  const scoped = targets.filter(
    (target) =>
      target.projectId === destination.projectId &&
      target.worktreeId === destination.worktreeId &&
      target.agentId === destination.agentId,
  );
  const target = scoped[0];
  if (scoped.length !== 1 || target === undefined) return null;
  const owners = new Set(
    targets
      .filter((candidate) => candidate.workspaceId === target.workspaceId)
      .map((candidate) =>
        JSON.stringify([
          candidate.projectId,
          candidate.worktreeId,
          String(candidate.workspaceOrdinal),
        ]),
      ),
  );
  return owners.size === 1 ? target : null;
}

export interface SignalPresence {
  /** The app is the foreground app (Windows: the main window has focus). */
  readonly appActive: boolean;
  /** Its window is the key window (Windows: the same focus). */
  readonly keyWindow: boolean;
  readonly settingsVisible: boolean;
  readonly selectedWorkspaceId: string | null;
  readonly activeWorkspaceId: string | null;
  readonly focusedAgentId: string | null;
  readonly activeProjectId: string | null;
}

/** Swift `AgentSignalPresence.isVisible`: the destination's agent is the focused pane on screen. */
export function isSignalVisible(
  presence: SignalPresence,
  destination: AgentSignalDestination,
): boolean {
  return (
    presence.appActive &&
    presence.keyWindow &&
    !presence.settingsVisible &&
    presence.selectedWorkspaceId !== null &&
    presence.selectedWorkspaceId === presence.activeWorkspaceId &&
    presence.focusedAgentId === destination.agentId &&
    presence.activeProjectId === destination.projectId
  );
}
