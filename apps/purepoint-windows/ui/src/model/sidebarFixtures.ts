// Test fixtures shared by the sidebar suites: the Swift tests' `makeAgent`, `makeProject` and
// `MachineState(machine:appState:nil,registry:nil)`, as plain snapshots.

import type { AgentStatus } from '../protocol';
import { makeAgentModel, makeWorktreeModel, type AgentModel } from './workspaceModel.ts';
import { projectKey, projectKeyId } from './projectKey.ts';
import type { MachineSnapshot, ProjectSnapshot } from './sidebarItem.ts';

export function makeAgent(id: string, status: AgentStatus = 'running'): AgentModel {
  return makeAgentModel({ id, name: id, agentType: 'claude', status, prompt: '', startedAt: '' });
}

/** The machine every test project lives on, so a root names the same project each time. */
export const TEST_MACHINE_ID = '6f1c2b0e-3d4a-4b5c-9e8f-0a1b2c3d4e5f';

export interface ProjectFixture {
  readonly worktreeCount?: number;
  readonly status?: AgentStatus;
  /** Null makes a local project. */
  readonly machineId?: string | null;
  readonly rootAgentIds?: readonly string[];
  readonly hasLoadedWorktrees?: boolean;
}

/** A project with `wt-<i>` worktrees on `branch-<i>`, each holding one `agent-<i>`. */
export function makeProject(root: string, fixture: ProjectFixture = {}): ProjectSnapshot {
  const machineId = fixture.machineId === undefined ? TEST_MACHINE_ID : fixture.machineId;
  const status = fixture.status ?? 'running';
  return {
    projectId: projectKeyId(projectKey(root, machineId)),
    projectRoot: root,
    rootAgents: (fixture.rootAgentIds ?? []).map((id) => makeAgent(id, status)),
    worktrees: Array.from({ length: fixture.worktreeCount ?? 0 }, (_, index) =>
      makeWorktreeModel({
        id: `wt-${String(index)}`,
        name: `wt-${String(index)}`,
        path: `/tmp/wt-${String(index)}`,
        branch: `branch-${String(index)}`,
        status: 'active',
        agents: [makeAgent(`agent-${String(index)}`, status)],
      }),
    ),
    hasLoadedWorktrees: fixture.hasLoadedWorktrees ?? false,
    isUnavailable: false,
    channelUnread: 0,
    layoutError: null,
  };
}

let machineCounter = 0;

/** A freshly added machine: disconnected, nothing loaded, no projects. */
export function makeMachine(
  name: string,
  overrides: Partial<MachineSnapshot> = {},
): MachineSnapshot {
  machineCounter += 1;
  return {
    id: `00000000-0000-4000-8000-${machineCounter.toString(16).padStart(12, '0')}`,
    name,
    sshHost: name,
    state: { kind: 'disconnected' },
    statusText: 'Disconnected',
    actionError: null,
    isStartingDaemon: false,
    hiddenProjectCount: 0,
    hiddenCountRevision: 0,
    hasLoadedProjects: false,
    projects: [],
    ...overrides,
  };
}

/** An in-memory `Storage`, standing in for a private `UserDefaults` suite. */
export class MemoryStorage implements Storage {
  private readonly items = new Map<string, string>();
  writes = 0;

  get length(): number {
    return this.items.size;
  }

  clear(): void {
    this.items.clear();
  }

  getItem(key: string): string | null {
    return this.items.get(key) ?? null;
  }

  key(index: number): string | null {
    return [...this.items.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.items.delete(key);
  }

  setItem(key: string, value: string): void {
    this.writes += 1;
    this.items.set(key, value);
  }
}
