// Port of apps/purepoint-macos/purepoint-macos/State/AgentConfigState.swift (WIN-8.7).
//
// Agent commands and launch args, for one project (`get_config` / `update_agent_config`) and for
// the whole machine (`get_global_agent_settings` / `update_global_agent_settings`), plus the
// agents the user added. Settings › Agents (WIN-8.11) draws it.
//
// Shape (DEC-14): published values live in a Zustand store (`config.store`). The Mac publishes the
// added agents through the mutable static `AgentVariant.addedAgents`; here they are the store's
// `addedAgents`, which callers pass to `allVariants`.

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { AgentConfigInfo, Request, Response } from '../protocol';
import { addedAgentVariant, type AgentVariant } from '../model/agentVariant';
import { daemonHealth } from '../services/daemon';
import { localDaemonClient } from '../services/daemonConnection';

// MARK: - Protocol helpers

/** Swift `AgentConfigPayload.agentType`. */
export const agentTypeOf = (agent: AgentConfigInfo): string => agent.id ?? agent.name;

export type UpdateGlobalAgentSettings = Extract<Request, { type: 'update_global_agent_settings' }>;
export type UpdateAgentConfig = Extract<Request, { type: 'update_agent_config' }>;

/**
 * Swift `DaemonRequest.updateGlobalAgentSettings(...)`: only the fields given are encoded, so the
 * daemon leaves the others alone.
 */
export function updateGlobalAgentSettingsRequest(fields: {
  agentStatusTracking?: boolean;
  launchArgs?: Record<string, string>;
  commands?: Record<string, string>;
  removeAgents?: string[];
}): UpdateGlobalAgentSettings {
  return {
    type: 'update_global_agent_settings',
    ...(fields.agentStatusTracking === undefined
      ? {}
      : { agent_status_tracking: fields.agentStatusTracking }),
    ...(fields.launchArgs === undefined ? {} : { launch_args: fields.launchArgs }),
    ...(fields.commands === undefined ? {} : { commands: fields.commands }),
    ...(fields.removeAgents === undefined ? {} : { remove_agents: fields.removeAgents }),
  };
}

/**
 * Swift `DaemonRequest.updateAgentConfig(...)`: `launch_args` is always sent (null clears the
 * project value); a left-out command keeps it; `remove` only when set.
 */
export function updateAgentConfigRequest(fields: {
  projectRoot: string;
  agentName: string;
  launchArgs: string | null;
  command?: string;
  remove?: boolean;
}): UpdateAgentConfig {
  return {
    type: 'update_agent_config',
    project_root: fields.projectRoot,
    agent_name: fields.agentName,
    launch_args: fields.launchArgs,
    ...(fields.command === undefined ? {} : { command: fields.command }),
    ...(fields.remove === true ? { remove: true } : {}),
  };
}

/** A `global_agent_settings_report`, with what an older daemon leaves out as null. */
export interface GlobalAgentSettings {
  readonly agentStatusTracking: boolean;
  readonly launchArgs: Readonly<Record<string, string>> | null;
  readonly commands: Readonly<Record<string, string>> | null;
}

/** Older daemons always track status, and have no machine launch args or commands. */
export function readGlobalAgentSettings(response: object): GlobalAgentSettings {
  const report = response as Record<string, unknown>;
  const tracking = report.agent_status_tracking;
  const record = (value: unknown) =>
    typeof value === 'object' && value !== null ? (value as Record<string, string>) : null;
  return {
    agentStatusTracking: typeof tracking === 'boolean' ? tracking : true,
    launchArgs: record(report.launch_args),
    commands: record(report.commands),
  };
}

// MARK: - AgentLaunchDefaults

/** What PurePoint knows about agent commands before the user changes anything. */
export const AgentLaunchDefaults = {
  /** Agent types PurePoint ships, in the order Settings lists them. */
  builtIns: ['claude', 'codex', 'opencode'] as readonly string[],

  /**
   * An agent type for a new agent: the program the command runs (`npx aider` → `aider`,
   * `scripts/agent-launch.sh goose` → `goose`), made unique among `taken`.
   */
  agentName(command: string, taken: ReadonlySet<string> | readonly string[]): string {
    const takenSet = new Set(taken);
    const words = command
      .split(/\s+/)
      .filter((word) => word !== '')
      .map((word) => {
        const parts = word.split('/').filter((part) => part !== '');
        return parts.at(-1) ?? word;
      });
    const program =
      words.find((w) => !w.startsWith('-') && !RUNNERS.has(w) && !w.endsWith('.sh')) ??
      words[0] ??
      '';
    const dot = program.lastIndexOf('.');
    const stem = (dot > 0 ? program.slice(0, dot) : program).toLowerCase();
    let slug = Array.from(stem, (ch) => (/[a-z0-9_-]/.test(ch) ? ch : '-')).join('');
    slug = slug.replace(/^[-_]+/, '').slice(0, 60);
    if (slug === '') slug = 'agent';
    if (!takenSet.has(slug)) return slug;
    let n = 2;
    while (takenSet.has(`${slug}-${String(n)}`)) n += 1;
    return `${slug}-${String(n)}`;
  },
} as const;

/** Words that start another program rather than name the agent. */
const RUNNERS = new Set([
  'env',
  'exec',
  'npx',
  'pnpx',
  'bunx',
  'uvx',
  'pipx',
  'node',
  'bun',
  'deno',
  'python',
  'python3',
]);

// MARK: - AgentConfigState

export interface AgentConfigStateFields {
  readonly agents: readonly AgentConfigInfo[];
  readonly defaultAgent: string;
  readonly isLoading: boolean;
  readonly error: string | null;
  readonly agentStatusTracking: boolean;
  /** Machine-level ("all projects") launch args string per agent type. */
  readonly machineLaunchArgs: Readonly<Record<string, string>>;
  /** Machine-level ("all projects") command per agent type. */
  readonly machineCommands: Readonly<Record<string, string>>;
  readonly globalSettingsLoaded: boolean;
  readonly isSavingGlobalSettings: boolean;
  readonly globalError: string | null;
  /** Swift `AgentVariant.addedAgents`: the agents the user added, wherever one can be started. */
  readonly addedAgents: readonly AgentVariant[];
}

const INITIAL: AgentConfigStateFields = {
  agents: [],
  defaultAgent: 'claude',
  isLoading: false,
  error: null,
  agentStatusTracking: true,
  machineLaunchArgs: {},
  machineCommands: {},
  globalSettingsLoaded: false,
  isSavingGlobalSettings: false,
  globalError: null,
  addedAgents: [],
};

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = error.message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}

const oldDaemonMessage = (action: string): string =>
  `Update the PurePoint daemon to ${action}. Restart it after updating.`;

export interface AgentConfigStateOptions {
  /** Start the local daemon if it is not running (the backend's `daemon_health`). */
  ensureDaemon?: () => Promise<unknown>;
  sendRequest?: (request: Request) => Promise<Response>;
}

export class AgentConfigState {
  readonly store: StoreApi<AgentConfigStateFields>;
  private readonly ensureDaemon: () => Promise<unknown>;
  private readonly sendRequest: (request: Request) => Promise<Response>;

  constructor(options: AgentConfigStateOptions = {}) {
    this.ensureDaemon = options.ensureDaemon ?? daemonHealth;
    this.sendRequest =
      options.sendRequest ??
      ((request) => {
        return localDaemonClient.send(request);
      });
    this.store = createStore<AgentConfigStateFields>()(() => INITIAL);
  }

  private get s(): AgentConfigStateFields {
    return this.store.getState();
  }

  private set(partial: Partial<AgentConfigStateFields>): void {
    this.store.setState(partial);
  }

  get agents(): readonly AgentConfigInfo[] {
    return this.s.agents;
  }
  get defaultAgent(): string {
    return this.s.defaultAgent;
  }
  get isLoading(): boolean {
    return this.s.isLoading;
  }
  get error(): string | null {
    return this.s.error;
  }
  get agentStatusTracking(): boolean {
    return this.s.agentStatusTracking;
  }
  get machineLaunchArgs(): Readonly<Record<string, string>> {
    return this.s.machineLaunchArgs;
  }
  get machineCommands(): Readonly<Record<string, string>> {
    return this.s.machineCommands;
  }
  get globalSettingsLoaded(): boolean {
    return this.s.globalSettingsLoaded;
  }
  get isSavingGlobalSettings(): boolean {
    return this.s.isSavingGlobalSettings;
  }
  get globalError(): string | null {
    return this.s.globalError;
  }
  get addedAgents(): readonly AgentVariant[] {
    return this.s.addedAgents;
  }

  async loadGlobalSettings(): Promise<void> {
    await this.sendGlobalSettings({ type: 'get_global_agent_settings' });
  }

  /** The agents in the All Projects list: built-ins, then the ones the user added. */
  get machineAgentNames(): string[] {
    const builtIns = AgentLaunchDefaults.builtIns;
    const added = new Set([
      ...Object.keys(this.machineCommands),
      ...Object.keys(this.machineLaunchArgs),
    ]);
    for (const name of builtIns) added.delete(name);
    return [...builtIns, ...[...added].sort()];
  }

  /** Save one agent's machine-level command and launch args; blank strings remove them. */
  async updateMachineAgent(options: {
    agentName: string;
    command: string;
    launchArgs: string;
    projectRoot: string | null;
  }): Promise<void> {
    await this.saveMachineSettings(
      updateGlobalAgentSettingsRequest({
        launchArgs: { [options.agentName]: options.launchArgs },
        commands: { [options.agentName]: options.command },
      }),
      options.projectRoot,
    );
  }

  /** Add an agent for every project; its type is named after the command. */
  async addMachineAgent(options: {
    command: string;
    launchArgs: string;
    projectRoot: string | null;
  }): Promise<void> {
    await this.updateMachineAgent({ agentName: this.newAgentName(options.command), ...options });
  }

  /** A type for a new agent that no agent at either level uses yet. */
  private newAgentName(command: string): string {
    return AgentLaunchDefaults.agentName(command, [
      ...this.machineAgentNames,
      ...this.agents.map(agentTypeOf),
    ]);
  }

  async removeMachineAgent(agentName: string, projectRoot: string | null): Promise<void> {
    await this.saveMachineSettings(
      updateGlobalAgentSettingsRequest({ removeAgents: [agentName] }),
      projectRoot,
    );
  }

  private async saveMachineSettings(
    request: UpdateGlobalAgentSettings,
    projectRoot: string | null,
  ): Promise<void> {
    if (this.isSavingGlobalSettings) return;
    this.set({ isSavingGlobalSettings: true });
    await this.sendGlobalSettings(request);
    if (projectRoot !== null) await this.load(projectRoot);
    this.set({ isSavingGlobalSettings: false });
  }

  async updateAgentStatusTracking(enabled: boolean): Promise<void> {
    if (this.isSavingGlobalSettings) return;
    this.set({ isSavingGlobalSettings: true });
    await this.sendGlobalSettings(
      updateGlobalAgentSettingsRequest({ agentStatusTracking: enabled }),
    );
    this.set({ isSavingGlobalSettings: false });
  }

  private async sendGlobalSettings(request: Request): Promise<void> {
    this.set({ globalError: null });
    try {
      await this.ensureDaemon();
      const response = await this.sendRequest(request);
      switch (response.type) {
        case 'global_agent_settings_report': {
          const report = readGlobalAgentSettings(response);
          this.set({ agentStatusTracking: report.agentStatusTracking, globalSettingsLoaded: true });
          const update = request.type === 'update_global_agent_settings' ? request : null;
          if (report.launchArgs !== null) {
            this.set({ machineLaunchArgs: report.launchArgs });
          } else if (update?.launch_args != null) {
            this.set({ globalError: oldDaemonMessage('save launch args for all projects') });
          }
          if (report.commands !== null) {
            this.set({ machineCommands: report.commands });
            this.publishAddedAgents();
          } else if (update !== null && (update.commands != null || update.remove_agents != null)) {
            this.set({ globalError: oldDaemonMessage('edit agent commands for all projects') });
          }
          break;
        }
        case 'error':
          this.set({ globalError: response.message });
          break;
        default:
          this.set({
            globalError: 'Unexpected response. Update the PurePoint daemon to use global settings.',
          });
      }
    } catch (error) {
      this.set({ globalError: describeError(error) });
    }
  }

  async load(projectRoot: string): Promise<void> {
    this.set({ isLoading: true, error: null });
    await this.sendProjectRequest({ type: 'get_config', project_root: projectRoot });
    this.set({ isLoading: false });
  }

  /**
   * Save one agent's project command and launch args; a blank value clears it so the All
   * Projects value applies.
   */
  async updateProjectAgent(options: {
    projectRoot: string;
    agentName: string;
    command: string;
    launchArgs: string;
  }): Promise<void> {
    this.set({ error: null });
    await this.sendProjectRequest(
      updateAgentConfigRequest({
        projectRoot: options.projectRoot,
        agentName: options.agentName,
        launchArgs: this.projectLaunchArgs(options.agentName, options.launchArgs),
        command: options.command,
      }),
    );
  }

  /**
   * The project launch args to store for a blank or typed field. A stored `launchArgs: ""`
   * launches the bare command and looks blank in Settings, so a blank field keeps it.
   */
  projectLaunchArgs(agentName: string, text: string): string | null {
    const args = text.trim();
    if (args !== '') return args;
    return this.agentConfig(agentName)?.project_launch_args === '' ? '' : null;
  }

  /** Add an agent to this project only; its type is named after the command. */
  async addProjectAgent(options: {
    projectRoot: string;
    command: string;
    launchArgs: string;
  }): Promise<void> {
    await this.updateProjectAgent({ agentName: this.newAgentName(options.command), ...options });
  }

  async removeProjectAgent(projectRoot: string, agentName: string): Promise<void> {
    this.set({ error: null });
    await this.sendProjectRequest(
      updateAgentConfigRequest({ projectRoot, agentName, launchArgs: null, remove: true }),
    );
  }

  private async sendProjectRequest(request: Request): Promise<void> {
    try {
      const response = await this.sendRequest(request);
      switch (response.type) {
        case 'config_report':
          this.applyConfigReport(response.default_agent, response.agents);
          break;
        case 'error':
          this.set({ error: response.message });
          break;
        default:
          this.set({ error: 'Unexpected response' });
      }
    } catch (error) {
      this.set({ error: describeError(error) });
    }
  }

  private applyConfigReport(defaultAgent: string, agents: readonly AgentConfigInfo[]): void {
    this.set({ defaultAgent, agents });
    this.publishAddedAgents();
    // Daemons before editable commands omit `inherited_command`.
    if (agents.some((agent) => agent.inherited_command == null)) {
      this.set({ error: oldDaemonMessage('edit agent commands') });
    }
  }

  /** Offer the agents the user added wherever a new agent can be started. */
  private publishAddedAgents(): void {
    const builtIns = new Set(AgentLaunchDefaults.builtIns);
    const commands = new Map<string, string>();
    for (const agent of this.agents) {
      const type = agentTypeOf(agent);
      if (!builtIns.has(type)) commands.set(type, agent.command);
    }
    for (const name of this.machineAgentNames) {
      if (!builtIns.has(name) && !commands.has(name)) {
        commands.set(name, this.machineCommands[name] ?? name);
      }
    }
    this.set({
      addedAgents: [...commands.keys()]
        .sort()
        .map((name) => addedAgentVariant(name, commands.get(name) ?? name)),
    });
  }

  agentConfig(name: string): AgentConfigInfo | null {
    return this.agents.find((agent) => agentTypeOf(agent) === name) ?? null;
  }
}
