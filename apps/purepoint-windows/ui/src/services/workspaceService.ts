// Port of apps/purepoint-macos/purepoint-macos/Services/{WorkspaceService,DaemonWorkspaceService}.swift
// (WIN-6.0).
//
// The abstraction boundary for a project's workspace data: the daemon's `status` report mapped to
// the sidebar's `AgentModel`/`WorktreeModel`. Requests and replies are the generated protocol
// types; the daemon is reached through a `DaemonRequester` (the local daemon by default).

import type { AgentStatusReport, Request, Response, WorktreeEntry } from '../protocol';
import { normalizeAgentStatus } from '../model/agentStatus';
import {
  agentModelFromEntry,
  makeAgentModel,
  makeWorktreeModel,
  type AgentModel,
  type WorktreeModel,
} from '../model/workspaceModel';
import { localDaemonClient, type DaemonRequester } from './daemonConnection';

/** Swift `WorkspaceSnapshot`. */
export interface WorkspaceSnapshot {
  readonly worktrees: WorktreeModel[];
  readonly rootAgents: AgentModel[];
}

/** Swift `WorkspaceService`. */
export interface WorkspaceService {
  loadWorkspace(projectRoot: string): Promise<WorkspaceSnapshot>;
  manifestPath(projectRoot: string): string;
}

/** Swift `DaemonWorkspaceError`. */
export class DaemonWorkspaceError extends Error {
  readonly kind: 'daemonError' | 'unexpectedResponse';

  private constructor(kind: 'daemonError' | 'unexpectedResponse', message: string) {
    super(message);
    this.name = 'DaemonWorkspaceError';
    this.kind = kind;
  }

  static daemonError(message: string): DaemonWorkspaceError {
    return new DaemonWorkspaceError('daemonError', message);
  }

  static unexpectedResponse(detail: string): DaemonWorkspaceError {
    return new DaemonWorkspaceError(
      'unexpectedResponse',
      `Unexpected response from daemon: ${detail}`,
    );
  }
}

function separatorOf(root: string): string {
  return root.includes('/') && !root.includes('\\') ? '/' : '\\';
}

/** `<projectRoot>/.pu/manifest.json`, with the root's own separator. */
export function manifestPath(projectRoot: string): string {
  const separator = separatorOf(projectRoot);
  return [projectRoot.replace(/[\\/]+$/, ''), '.pu', 'manifest.json'].join(separator);
}

/**
 * The root agents of a status report. A root agent's signal must name no worktree: one that does
 * belongs to a worktree agent of the same id and is dropped.
 */
export function rootAgentModels(reports: readonly AgentStatusReport[]): AgentModel[] {
  return reports.map((report) =>
    makeAgentModel({
      id: report.id,
      name: report.name,
      agentType: report.agent_type,
      status: normalizeAgentStatus(report.status),
      prompt: report.prompt ?? '',
      startedAt: report.started_at,
      sessionId: report.session_id ?? null,
      suspended: report.suspended,
      signal: report.signal?.identity.worktree_id == null ? (report.signal ?? null) : null,
      placement: report.placement ?? null,
    }),
  );
}

/**
 * Swift `DaemonWorkspaceService.parseWorktrees`: each worktree's agents oldest first, keeping a
 * signal only when it names this worktree.
 */
export function parseWorktrees(entries: readonly WorktreeEntry[]): WorktreeModel[] {
  return entries.map((entry) => {
    const agents = Object.values(entry.agents)
      .map((agent) => {
        const model = agentModelFromEntry(agent);
        return model.signal !== null && model.signal.identity.worktree_id !== entry.id
          ? { ...model, signal: null }
          : model;
      })
      .sort((a, b) => (a.startedAt < b.startedAt ? -1 : a.startedAt > b.startedAt ? 1 : 0));
    return makeWorktreeModel({
      id: entry.id,
      name: entry.name,
      path: entry.path,
      branch: entry.branch,
      status: entry.status,
      agents,
      baseBranch: entry.baseBranch,
    });
  });
}

/** A `status_report` or `status_event` payload as a snapshot. */
export function workspaceSnapshot(
  worktrees: readonly WorktreeEntry[],
  agents: readonly AgentStatusReport[],
): WorkspaceSnapshot {
  return { worktrees: parseWorktrees(worktrees), rootAgents: rootAgentModels(agents) };
}

function describe(response: Response): string {
  return JSON.stringify(response);
}

/** Swift `DaemonWorkspaceService`: the project's `status` from its daemon. */
export class DaemonWorkspaceService implements WorkspaceService {
  constructor(readonly client: DaemonRequester = localDaemonClient) {}

  manifestPath(projectRoot: string): string {
    return manifestPath(projectRoot);
  }

  async loadWorkspace(projectRoot: string): Promise<WorkspaceSnapshot> {
    const request: Request = { type: 'status', project_root: projectRoot };
    const response = await this.client.send(request);
    switch (response.type) {
      case 'status_report':
        return workspaceSnapshot(response.worktrees, response.agents);
      case 'error':
        throw DaemonWorkspaceError.daemonError(response.message);
      default:
        throw DaemonWorkspaceError.unexpectedResponse(describe(response));
    }
  }

  /** Swift's static `parseWorktrees`. */
  static parseWorktrees(entries: readonly WorktreeEntry[]): WorktreeModel[] {
    return parseWorktrees(entries);
  }
}
