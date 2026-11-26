// Port of apps/purepoint-macos/purepoint-macos/Models/AgentStatus.swift (WIN-6.0).
//
// The daemon writes only `running` and `broken` (the generated `AgentStatus`); the other raw values
// exist in pre-v1 manifests and status reports, and normalize to those two as in Swift.

import type { AgentStatus } from '../protocol';

/** Every raw status Swift's `AgentStatus` decodes, current first, then the legacy values. */
export const AGENT_STATUS_RAW_VALUES = [
  'running',
  'broken',
  'streaming',
  'waiting',
  'idle',
  'completed',
  'failed',
  'killed',
  'spawning',
  'lost',
  'suspended',
] as const;

export type AgentStatusRawValue = (typeof AGENT_STATUS_RAW_VALUES)[number];

const ALIVE_RAW: ReadonlySet<string> = new Set([
  'running',
  'streaming',
  'waiting',
  'spawning',
  'idle',
  'suspended',
]);

/**
 * Swift `AgentStatus(rawValue:) ?? .lost`, then `normalized`: an unknown raw value is `lost`,
 * which is `broken`.
 */
export function normalizeAgentStatus(raw: string): AgentStatus {
  return ALIVE_RAW.has(raw) ? 'running' : 'broken';
}

/** Swift `AgentStatus.isAlive`. */
export function isAgentStatusAlive(raw: string): boolean {
  return normalizeAgentStatus(raw) === 'running';
}

/** Swift `nsColor`: the AppKit system color the status dot uses (the views map it, DEC-13). */
export function agentStatusSystemColor(raw: string): 'systemGreen' | 'systemRed' {
  return normalizeAgentStatus(raw) === 'running' ? 'systemGreen' : 'systemRed';
}
