// Port of apps/purepoint-macos/purepoint-macos/State/AgentsHubState.swift (WIN-8.7).
//
// The Agents hub's library: prompt templates, agent definitions and swarm definitions merged
// across the library's project roots, the selection in each list, and the create sheets.
//
// Shape (DEC-14): what Swift's `@Observable` publishes lives in a Zustand store (`hub.store`) the
// views subscribe to; the class mirrors the Swift methods. Requests go through a
// `DaemonRequester`. Swift's client throws only on transport failures and silently drops an
// `error` reply; here an `error` reply sets `error` too, so a refused save is not silent.

import { createStore, type StoreApi } from 'zustand/vanilla';
import type { Request, Response } from '../protocol';
import {
  agentDefinitionFromInfo,
  makeSavedPrompt,
  projectNameOf,
  savedPromptFromInfo,
  swarmDefinitionFromInfo,
  type AgentDefinition,
  type SavedPrompt,
  type SwarmDefinition,
} from '../model/agentsHubModels';
import { localDaemonClient, type DaemonRequester } from '../services/daemonConnection';

export interface AgentsHubStateFields {
  readonly prompts: readonly SavedPrompt[];
  readonly agents: readonly AgentDefinition[];
  readonly swarms: readonly SwarmDefinition[];
  readonly selectedPromptId: string | null;
  readonly selectedAgentId: string | null;
  readonly selectedSwarmId: string | null;
  readonly showingCreatePrompt: boolean;
  readonly showingCreateAgent: boolean;
  readonly showingCreateSwarm: boolean;
  readonly isLoading: boolean;
  readonly error: string | null;
  /** The agents the last swarm run or spawn started (Swift prints them). */
  readonly lastSpawnedAgents: readonly string[];
}

const INITIAL: AgentsHubStateFields = {
  prompts: [],
  agents: [],
  swarms: [],
  selectedPromptId: null,
  selectedAgentId: null,
  selectedSwarmId: null,
  showingCreatePrompt: false,
  showingCreateAgent: false,
  showingCreateSwarm: false,
  isLoading: false,
  error: null,
  lastSpawnedAgents: [],
};

// Derived values, for views that select from the store.
export const selectedPromptOf = (s: AgentsHubStateFields): SavedPrompt | null =>
  s.prompts.find((p) => p.id === s.selectedPromptId) ?? null;
export const selectedAgentOf = (s: AgentsHubStateFields): AgentDefinition | null =>
  s.agents.find((a) => a.id === s.selectedAgentId) ?? null;
export const selectedSwarmOf = (s: AgentsHubStateFields): SwarmDefinition | null =>
  s.swarms.find((w) => w.id === s.selectedSwarmId) ?? null;

/** A failed request: a transport rejection or the daemon's `error` reply. */
class HubRequestError extends Error {}

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    const message = error.message;
    if (typeof message === 'string') return message;
  }
  return String(error);
}

const byName = <T extends { name: string }>(a: T, b: T): number =>
  a.name < b.name ? -1 : a.name > b.name ? 1 : 0;

/**
 * Merge one kind of definition across roots: a project-scoped entry records the root it came from;
 * the first root to list a key wins (a global entry is listed from every root).
 */
function mergeAcrossRoots<T extends { id: string; name: string }>(
  lists: readonly { root: string; items: readonly T[]; isGlobal: (item: T) => boolean }[],
  withProject: (item: T, root: string, projectName: string) => T,
): T[] {
  const merged = new Map<string, T>();
  for (const { root, items, isGlobal } of lists) {
    const projectName = projectNameOf(root);
    for (const item of items) {
      const placed = isGlobal(item) ? item : withProject(item, root, projectName);
      if (!merged.has(placed.id)) merged.set(placed.id, placed);
    }
  }
  return [...merged.values()].sort(byName);
}

export class AgentsHubState {
  readonly store: StoreApi<AgentsHubStateFields>;
  private readonly client: DaemonRequester;

  constructor(client: DaemonRequester = localDaemonClient) {
    this.client = client;
    this.store = createStore<AgentsHubStateFields>()(() => INITIAL);
  }

  // MARK: Observable accessors

  private get s(): AgentsHubStateFields {
    return this.store.getState();
  }

  private set(partial: Partial<AgentsHubStateFields>): void {
    this.store.setState(partial);
  }

  get prompts(): readonly SavedPrompt[] {
    return this.s.prompts;
  }
  set prompts(value: readonly SavedPrompt[]) {
    this.set({ prompts: value });
  }
  get agents(): readonly AgentDefinition[] {
    return this.s.agents;
  }
  set agents(value: readonly AgentDefinition[]) {
    this.set({ agents: value });
  }
  get swarms(): readonly SwarmDefinition[] {
    return this.s.swarms;
  }
  set swarms(value: readonly SwarmDefinition[]) {
    this.set({ swarms: value });
  }
  get selectedPromptId(): string | null {
    return this.s.selectedPromptId;
  }
  set selectedPromptId(value: string | null) {
    this.set({ selectedPromptId: value });
  }
  get selectedAgentId(): string | null {
    return this.s.selectedAgentId;
  }
  set selectedAgentId(value: string | null) {
    this.set({ selectedAgentId: value });
  }
  get selectedSwarmId(): string | null {
    return this.s.selectedSwarmId;
  }
  set selectedSwarmId(value: string | null) {
    this.set({ selectedSwarmId: value });
  }
  get showingCreatePrompt(): boolean {
    return this.s.showingCreatePrompt;
  }
  set showingCreatePrompt(value: boolean) {
    this.set({ showingCreatePrompt: value });
  }
  get showingCreateAgent(): boolean {
    return this.s.showingCreateAgent;
  }
  set showingCreateAgent(value: boolean) {
    this.set({ showingCreateAgent: value });
  }
  get showingCreateSwarm(): boolean {
    return this.s.showingCreateSwarm;
  }
  set showingCreateSwarm(value: boolean) {
    this.set({ showingCreateSwarm: value });
  }
  get isLoading(): boolean {
    return this.s.isLoading;
  }
  get error(): string | null {
    return this.s.error;
  }
  set error(value: string | null) {
    this.set({ error: value });
  }

  // Event handlers call these (React's rules keep views from assigning to a hook value).
  selectPrompt(id: string | null): void {
    this.selectedPromptId = id;
  }
  selectAgent(id: string | null): void {
    this.selectedAgentId = id;
  }
  selectSwarm(id: string | null): void {
    this.selectedSwarmId = id;
  }
  /** Show or hide a create sheet. */
  setShowingCreate(kind: 'prompt' | 'agent' | 'swarm', shown: boolean): void {
    if (kind === 'prompt') this.showingCreatePrompt = shown;
    else if (kind === 'agent') this.showingCreateAgent = shown;
    else this.showingCreateSwarm = shown;
  }
  dismissError(): void {
    this.error = null;
  }

  get selectedPrompt(): SavedPrompt | null {
    return selectedPromptOf(this.s);
  }
  get selectedAgent(): AgentDefinition | null {
    return selectedAgentOf(this.s);
  }
  get selectedSwarm(): SwarmDefinition | null {
    return selectedSwarmOf(this.s);
  }

  // MARK: Requests

  /** One request; rejects with the daemon's message on an `error` reply. */
  private async send(request: Request): Promise<Response> {
    const response = await this.client.send(request);
    if (response.type === 'error') throw new HubRequestError(response.message);
    return response;
  }

  // MARK: Loading

  async loadAll(projectRoots: readonly string[]): Promise<void> {
    this.set({ isLoading: true, error: null });
    await Promise.all([
      this.loadTemplates(projectRoots),
      this.loadAgentDefs(projectRoots),
      this.loadSwarmDefs(projectRoots),
    ]);
    this.set({ isLoading: false });
  }

  async loadTemplates(projectRoots: readonly string[]): Promise<void> {
    try {
      const lists = [];
      for (const root of projectRoots) {
        const response = await this.send({ type: 'list_templates', project_root: root });
        if (response.type !== 'template_list') continue;
        lists.push({
          root,
          items: response.templates.map(savedPromptFromInfo),
          isGlobal: (p: SavedPrompt) => p.source === 'global',
        });
      }
      const prompts = mergeAcrossRoots(lists, (p, root, projectName) => ({
        ...p,
        projectRoot: root,
        projectName,
      }));
      this.set({ prompts });
      if (this.selectedPromptId === null && prompts[0] !== undefined) {
        this.selectedPromptId = prompts[0].id;
      }
    } catch (error) {
      this.error = `Failed to load templates: ${describeError(error)}`;
    }
  }

  async loadAgentDefs(projectRoots: readonly string[]): Promise<void> {
    try {
      const lists = [];
      for (const root of projectRoots) {
        const response = await this.send({ type: 'list_agent_defs', project_root: root });
        if (response.type !== 'agent_def_list') continue;
        lists.push({
          root,
          items: response.agent_defs.map(agentDefinitionFromInfo),
          isGlobal: (d: AgentDefinition) => d.scope === 'global',
        });
      }
      const agents = mergeAcrossRoots(lists, (d, root, projectName) => ({
        ...d,
        projectRoot: root,
        projectName,
      }));
      this.set({ agents });
      if (this.selectedAgentId === null && agents[0] !== undefined) {
        this.selectedAgentId = agents[0].id;
      }
    } catch (error) {
      this.error = `Failed to load agent defs: ${describeError(error)}`;
    }
  }

  async loadSwarmDefs(projectRoots: readonly string[]): Promise<void> {
    try {
      const lists = [];
      for (const root of projectRoots) {
        const response = await this.send({ type: 'list_swarm_defs', project_root: root });
        if (response.type !== 'swarm_def_list') continue;
        lists.push({
          root,
          items: response.swarm_defs.map(swarmDefinitionFromInfo),
          isGlobal: (d: SwarmDefinition) => d.scope === 'global',
        });
      }
      const swarms = mergeAcrossRoots(lists, (d, root, projectName) => ({
        ...d,
        projectRoot: root,
        projectName,
      }));
      this.set({ swarms });
      if (this.selectedSwarmId === null && swarms[0] !== undefined) {
        this.selectedSwarmId = swarms[0].id;
      }
    } catch (error) {
      this.error = `Failed to load swarm defs: ${describeError(error)}`;
    }
  }

  /** Fill in the body (and the rest) of the listed prompt the template resolves to. */
  async loadPromptDetail(projectRoot: string, name: string): Promise<void> {
    try {
      const response = await this.send({ type: 'get_template', project_root: projectRoot, name });
      if (response.type !== 'template_detail') return;
      this.set({
        prompts: this.prompts.map((p) =>
          p.name === response.name && p.source === response.source
            ? {
                ...p,
                body: response.body,
                description: response.description,
                agent: response.agent,
                variables: [...response.variables],
                command: response.command ?? null,
              }
            : p,
        ),
      });
    } catch (error) {
      this.error = `Failed to load prompt detail: ${describeError(error)}`;
    }
  }

  /**
   * Templates as listed from one root, without touching the hub's own list or selection.
   * Null when the daemon can't be reached.
   */
  async listTemplates(root: string): Promise<SavedPrompt[] | null> {
    try {
      const response = await this.send({ type: 'list_templates', project_root: root });
      return response.type === 'template_list' ? response.templates.map(savedPromptFromInfo) : null;
    } catch {
      return null;
    }
  }

  /** One template with its body, as resolved from `root`, without touching the hub's own list. */
  async template(root: string, name: string): Promise<SavedPrompt | null> {
    try {
      const response = await this.send({ type: 'get_template', project_root: root, name });
      if (response.type !== 'template_detail') return null;
      return makeSavedPrompt({
        name: response.name,
        description: response.description,
        agent: response.agent,
        body: response.body,
        source: response.source,
        variables: response.variables,
        command: response.command ?? null,
      });
    } catch {
      return null;
    }
  }

  // MARK: Templates

  async saveTemplate(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    name: string;
    description: string;
    agent: string;
    body: string;
    scope: string;
    command?: string | null;
  }): Promise<void> {
    try {
      await this.send({
        type: 'save_template',
        project_root: options.projectRoot,
        name: options.name,
        description: options.description,
        agent: options.agent,
        body: options.body,
        scope: options.scope,
        command: options.command ?? null,
      });
      await this.loadTemplates(options.projectRoots);
      await this.loadPromptDetail(options.projectRoot, options.name);
    } catch (error) {
      this.error = `Failed to save template: ${describeError(error)}`;
    }
  }

  async deleteTemplate(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    name: string;
    scope: string;
  }): Promise<void> {
    try {
      await this.send({
        type: 'delete_template',
        project_root: options.projectRoot,
        name: options.name,
        scope: options.scope,
      });
      await this.loadTemplates(options.projectRoots);
    } catch (error) {
      this.error = `Failed to delete template: ${describeError(error)}`;
    }
  }

  // MARK: Agent definitions

  async saveAgentDef(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    def: AgentDefinition;
  }): Promise<void> {
    const { def } = options;
    try {
      await this.send({
        type: 'save_agent_def',
        project_root: options.projectRoot,
        name: def.name,
        agent_type: def.agentType,
        template: def.template,
        inline_prompt: def.inlinePrompt,
        tags: [...def.tags],
        scope: def.scope,
        available_in_command_dialog: def.availableInCommandDialog,
        icon: def.icon,
        command: def.command,
      });
      await this.loadAgentDefs(options.projectRoots);
    } catch (error) {
      this.error = `Failed to save agent def: ${describeError(error)}`;
    }
  }

  async deleteAgentDef(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    name: string;
    scope: string;
  }): Promise<void> {
    try {
      await this.send({
        type: 'delete_agent_def',
        project_root: options.projectRoot,
        name: options.name,
        scope: options.scope,
      });
      await this.loadAgentDefs(options.projectRoots);
    } catch (error) {
      this.error = `Failed to delete agent def: ${describeError(error)}`;
    }
  }

  // MARK: Swarm definitions

  async saveSwarmDef(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    def: SwarmDefinition;
  }): Promise<void> {
    const { def } = options;
    try {
      await this.send({
        type: 'save_swarm_def',
        project_root: options.projectRoot,
        name: def.name,
        worktree_count: def.worktreeCount,
        worktree_template: def.worktreeTemplate,
        roster: def.roster.map((item) => ({
          agent_def: item.agentDef,
          role: item.role,
          quantity: item.quantity,
        })),
        include_terminal: def.includeTerminal,
        scope: def.scope,
      });
      await this.loadSwarmDefs(options.projectRoots);
    } catch (error) {
      this.error = `Failed to save swarm def: ${describeError(error)}`;
    }
  }

  async deleteSwarmDef(options: {
    projectRoot: string;
    projectRoots: readonly string[];
    name: string;
    scope: string;
  }): Promise<void> {
    try {
      await this.send({
        type: 'delete_swarm_def',
        project_root: options.projectRoot,
        name: options.name,
        scope: options.scope,
      });
      await this.loadSwarmDefs(options.projectRoots);
    } catch (error) {
      this.error = `Failed to delete swarm def: ${describeError(error)}`;
    }
  }

  /** Run a swarm; resolves with the agents it started (also when it stopped part way). */
  async runSwarm(
    projectRoot: string,
    name: string,
    vars: Readonly<Record<string, string>> = {},
  ): Promise<string[]> {
    try {
      const response = await this.send({
        type: 'run_swarm',
        project_root: projectRoot,
        swarm_name: name,
        vars: { ...vars },
      });
      if (response.type === 'run_swarm_result') {
        this.set({ lastSpawnedAgents: response.spawned_agents });
        return response.spawned_agents;
      }
      if (response.type === 'run_swarm_partial') {
        this.set({
          lastSpawnedAgents: response.spawned_agents,
          error: `Failed to run swarm: ${response.error_message}`,
        });
        return response.spawned_agents;
      }
      return [];
    } catch (error) {
      this.error = `Failed to run swarm: ${describeError(error)}`;
      return [];
    }
  }

  // MARK: Spawning (Windows: the Mac launches these from the command palette)

  /**
   * Start a root agent from a prompt: its body (fetched when not loaded yet), its agent type
   * (`claude` when it names none) and its command. Resolves with the new agent's id.
   */
  async spawnFromPrompt(projectRoot: string, prompt: SavedPrompt): Promise<string | null> {
    const resolved =
      prompt.body === '' ? ((await this.template(projectRoot, prompt.name)) ?? prompt) : prompt;
    return this.spawn(
      {
        type: 'spawn',
        project_root: projectRoot,
        prompt: resolved.body,
        agent: resolved.agent === '' ? 'claude' : resolved.agent,
        root: true,
        command: resolved.command,
      },
      'Failed to spawn from prompt',
    );
  }

  /**
   * Start a root agent from a definition. Its prompt is the inline prompt, else its template's
   * body; its command is its own, else the template's (as `run_swarm` resolves them).
   */
  async spawnAgentDef(projectRoot: string, def: AgentDefinition): Promise<string | null> {
    let prompt = def.inlinePrompt ?? '';
    let command = def.command;
    if (def.template !== null && def.template !== '' && def.inlinePrompt === null) {
      const template = await this.template(projectRoot, def.template);
      if (template === null) {
        this.error = `Failed to spawn agent: template '${def.template}' not found`;
        return null;
      }
      prompt = template.body;
      command ??= template.command;
    }
    return this.spawn(
      {
        type: 'spawn',
        project_root: projectRoot,
        prompt,
        agent: def.agentType,
        root: true,
        command,
      },
      'Failed to spawn agent',
    );
  }

  private async spawn(request: Request, failure: string): Promise<string | null> {
    try {
      const response = await this.send(request);
      if (response.type !== 'spawn_result') return null;
      this.set({ lastSpawnedAgents: [response.agent_id] });
      return response.agent_id;
    } catch (error) {
      this.error = `${failure}: ${describeError(error)}`;
      return null;
    }
  }
}

const hubs = new WeakMap<object, AgentsHubState>();

/** The hub state of one app window (Swift `appState.agentsHubState`), made on first use. */
export function agentsHubStateFor(owner: object): AgentsHubState {
  let hub = hubs.get(owner);
  if (hub === undefined) {
    hub = new AgentsHubState();
    hubs.set(owner, hub);
  }
  return hub;
}
