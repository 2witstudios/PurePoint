// Port of apps/purepoint-macos/purepoint-macos/Models/WorkspaceModel.swift (WIN-6.1).
//
// The sidebar's view models for one project's manifest. They are plain immutable data, compared
// structurally (`structurallyEqual`), so a snapshot that changes any field, including a nested
// agent rename, reaches the sidebar, and an unchanged one does not.

import type {
  AgentEntry,
  AgentStatus,
  SignalSnapshot,
  SpawnPlacement,
  WorktreeStatus,
} from '../protocol';

export interface AgentModel {
  readonly id: string;
  readonly name: string;
  readonly agentType: string;
  readonly status: AgentStatus;
  readonly prompt: string;
  readonly startedAt: string;
  readonly sessionId: string | null;
  readonly suspended: boolean;
  readonly command: string | null;
  readonly signal: SignalSnapshot | null;
  /** The tab this agent was spawned for, if any. */
  readonly placement: SpawnPlacement | null;
}

export interface WorktreeModel {
  readonly id: string;
  readonly name: string;
  readonly path: string;
  readonly branch: string;
  readonly status: WorktreeStatus;
  readonly agents: readonly AgentModel[];
  readonly baseBranch: string | null;
}

type Defaulted<T, K extends keyof T> = Omit<T, K> & Partial<Pick<T, K>>;

export type AgentModelInit = Defaulted<
  AgentModel,
  'sessionId' | 'suspended' | 'command' | 'signal' | 'placement'
>;

export type WorktreeModelInit = Defaulted<WorktreeModel, 'baseBranch'>;

/** Swift's memberwise `AgentModel.init`, with the same defaults. */
export function makeAgentModel(init: AgentModelInit): AgentModel {
  return {
    id: init.id,
    name: init.name,
    agentType: init.agentType,
    status: init.status,
    prompt: init.prompt,
    startedAt: init.startedAt,
    sessionId: init.sessionId ?? null,
    suspended: init.suspended ?? false,
    command: init.command ?? null,
    signal: init.signal ?? null,
    placement: init.placement ?? null,
  };
}

export function makeWorktreeModel(init: WorktreeModelInit): WorktreeModel {
  return { ...init, baseBranch: init.baseBranch ?? null };
}

/**
 * A manifest agent entry as the adapter accepts it: the generated `AgentEntry`, except that
 * `prompt` and `suspended` may be missing, as Swift's decoder allows (older manifests).
 */
export type AgentEntryInput = Omit<AgentEntry, 'prompt' | 'suspended'> & {
  readonly prompt?: string | null;
  readonly suspended?: boolean;
};

/** Swift's `AgentModel.init(from: AgentEntry)`. */
export function agentModelFromEntry(entry: AgentEntryInput): AgentModel {
  return makeAgentModel({
    id: entry.id,
    name: entry.name,
    agentType: entry.agentType,
    status: entry.status,
    prompt: entry.prompt ?? '',
    startedAt: entry.startedAt,
    sessionId: entry.sessionId ?? null,
    suspended: entry.suspended ?? false,
    command: entry.command ?? null,
    signal: entry.signal ?? null,
    placement: entry.placement ?? null,
  });
}

export function agentDisplayName(agent: AgentModel): string {
  return agent.name === '' ? agent.id : agent.name;
}

/** Whether the agent's process is alive (Swift `AgentStatus.isAlive`, over the daemon's two states). */
export function isAgentAlive(agent: AgentModel): boolean {
  return agent.status === 'running';
}

/**
 * Deep equality over plain data (objects, arrays, primitives), as Swift's synthesized `==`.
 * Object key order does not matter; a key whose value is `undefined` counts as absent.
 */
export function structurallyEqual(a: unknown, b: unknown): boolean {
  if (Object.is(a, b)) return true;
  if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((item, index) => structurallyEqual(item, b[index]));
  }
  const left = a as Record<string, unknown>;
  const right = b as Record<string, unknown>;
  const keys = new Set([...Object.keys(left), ...Object.keys(right)]);
  for (const key of keys) {
    if (!structurallyEqual(left[key], right[key])) return false;
  }
  return true;
}
