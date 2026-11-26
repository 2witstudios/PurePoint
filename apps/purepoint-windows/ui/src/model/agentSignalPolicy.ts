// Port of apps/purepoint-macos/purepoint-macos/Models/AgentSignalPolicy.swift (WIN-7.7).
//
// Which signals notify and what the badge counts. The badge counts distinct agents whose signal
// needs attention (a permission or question request, or an error not yet seen). An alert is
// posted once per event key — the agent, its launch, generation, session and event — so a repeated
// status never notifies twice, and a new launch or event notifies again. Delivered keys persist
// (at most 4096) so a restart does not repeat them.

import {
  type AgentSignalPresentation,
  UNKNOWN_SIGNAL_PRESENTATION,
  opaqueKey,
} from './agentSignalSnapshot.ts';
import type { AgentSignalDestination } from './agentSignalRouting.ts';

const MAX_DELIVERED = 4096;

/** One agent as the policy sees it (Swift `AgentSignalAlertPolicy.Agent`). */
export interface SignalAlertAgent {
  /** Opaque, stable per project, worktree and agent. */
  readonly identity: string;
  readonly launch: string;
  readonly presentation: AgentSignalPresentation;
  readonly qualificationKey: string | null;
  /**
   * The Mac's `macOSActionable`: the pending request can be answered from a notification on this
   * platform. Windows toasts only open the pane, so the Windows app does not gate on it.
   */
  readonly actionable: boolean;
  readonly destination: AgentSignalDestination | null;
}

export type SignalAlertAgentInit = Pick<SignalAlertAgent, 'identity' | 'launch'> &
  Partial<Omit<SignalAlertAgent, 'identity' | 'launch'>>;

export function makeSignalAlertAgent(init: SignalAlertAgentInit): SignalAlertAgent {
  return {
    identity: init.identity,
    launch: init.launch,
    presentation: init.presentation ?? UNKNOWN_SIGNAL_PRESENTATION,
    qualificationKey: init.qualificationKey ?? null,
    actionable: init.actionable ?? false,
    destination: init.destination ?? null,
  };
}

/** The notification identity for the agent's current event, or null when it has none. */
export function signalEventKey(agent: SignalAlertAgent): string | null {
  const event = agent.presentation.eventId;
  if (event === null) return null;
  return opaqueKey([
    agent.identity,
    agent.launch,
    String(agent.destination?.generation ?? 0),
    agent.destination?.nativeSessionId ?? '',
    event,
  ]);
}

export interface SignalProjection {
  readonly badge: number;
  readonly alerts: readonly SignalAlertAgent[];
}

export class AgentSignalAlertPolicy {
  private readonly delivered: Set<string>;
  private order: string[];

  constructor(deliveredKeys: readonly string[] = []) {
    const seen = new Set<string>();
    this.order = deliveredKeys.slice(-MAX_DELIVERED).filter((key) => {
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    this.delivered = new Set(this.order);
  }

  get deliveredKeys(): readonly string[] {
    return this.order;
  }

  /**
   * The badge and the agents to alert for. Attention alerts need `qualifiedAttention`; an
   * unqualified one is not marked delivered, so it alerts once qualification arrives. A finished
   * turn alerts without qualification. Other states never alert.
   */
  project(
    agents: readonly SignalAlertAgent[],
    qualifiedAttention: (agent: SignalAlertAgent) => boolean = () => false,
    acknowledgedErrors: ReadonlySet<string> = new Set(),
  ): SignalProjection {
    const attention = new Set(
      agents
        .filter((agent) => {
          if (!agent.presentation.needsAttention) return false;
          if (agent.presentation.label !== 'Error') return true;
          const key = signalEventKey(agent);
          return key === null || !acknowledgedErrors.has(key);
        })
        .map((agent) => agent.identity),
    );
    const alerts: SignalAlertAgent[] = [];
    for (const agent of agents) {
      const key = signalEventKey(agent);
      if (key === null || this.delivered.has(key) || acknowledgedErrors.has(key)) continue;
      if (this.delivered.size >= MAX_DELIVERED) {
        const current = new Set(agents.flatMap((candidate) => signalEventKey(candidate) ?? []));
        const old = this.order.find((candidate) => !current.has(candidate));
        if (old === undefined) continue;
        this.delivered.delete(old);
        this.order = this.order.filter((candidate) => candidate !== old);
      }
      // Keep state and badges current even when notification delivery is unqualified.
      if (agent.presentation.needsAttention && !qualifiedAttention(agent)) continue;
      if (!agent.presentation.needsAttention && agent.presentation.label !== 'Finished') continue;
      this.delivered.add(key);
      this.order.push(key);
      alerts.push(agent);
    }
    return { badge: attention.size, alerts };
  }
}
