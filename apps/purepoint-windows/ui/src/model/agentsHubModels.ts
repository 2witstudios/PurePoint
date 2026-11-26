// Port of apps/purepoint-macos/purepoint-macos/Models/AgentsHubModels.swift (WIN-8.7), plus the
// hub's tab enum (`AgentsHubTab`, from Views/Agents/AgentsHubView.swift).
//
// The Swift structs are value types built from the daemon's list payloads (`TemplateInfo`,
// `AgentDefInfo`, `SwarmDefInfo`) or by hand; here each is a plain object with a factory for each
// Swift initializer. Ids are `source:name` / `scope:name`, as on the Mac.
import type { AgentDefInfo, SwarmDefInfo, TemplateInfo } from '../protocol';

// MARK: - SavedPrompt

/** Swift `SavedPrompt`: a prompt template. */
export interface SavedPrompt {
  readonly id: string;
  readonly name: string;
  readonly description: string;
  readonly agent: string;
  readonly body: string;
  readonly source: string;
  readonly variables: readonly string[];
  readonly command: string | null;
  readonly projectRoot: string | null;
  readonly projectName: string | null;
}

/** Swift `SavedPrompt(from:)`. A list entry carries no body. */
export function savedPromptFromInfo(info: TemplateInfo): SavedPrompt {
  return {
    id: `${info.source}:${info.name}`,
    name: info.name,
    description: info.description,
    agent: info.agent,
    body: '',
    source: info.source,
    variables: [...info.variables],
    command: info.command ?? null,
    projectRoot: null,
    projectName: null,
  };
}

/** Swift `SavedPrompt(name:description:agent:body:source:variables:command:)`. */
export function makeSavedPrompt(fields: {
  name: string;
  description: string;
  agent: string;
  body: string;
  source: string;
  variables: readonly string[];
  command?: string | null;
}): SavedPrompt {
  return {
    id: `${fields.source}:${fields.name}`,
    name: fields.name,
    description: fields.description,
    agent: fields.agent,
    body: fields.body,
    source: fields.source,
    variables: [...fields.variables],
    command: fields.command ?? null,
    projectRoot: null,
    projectName: null,
  };
}

// MARK: - AgentDefinition

/** Swift `AgentDefinition`: a saved agent the command dialog can launch. */
export interface AgentDefinition {
  readonly id: string;
  readonly name: string;
  readonly agentType: string;
  readonly template: string | null;
  readonly inlinePrompt: string | null;
  readonly tags: readonly string[];
  readonly scope: string;
  readonly availableInCommandDialog: boolean;
  readonly icon: string | null;
  readonly command: string | null;
  readonly projectRoot: string | null;
  readonly projectName: string | null;
}

/** Swift `AgentDefinition(from:)`. */
export function agentDefinitionFromInfo(info: AgentDefInfo): AgentDefinition {
  return {
    id: `${info.scope}:${info.name}`,
    name: info.name,
    agentType: info.agent_type,
    template: info.template,
    inlinePrompt: info.inline_prompt,
    tags: [...info.tags],
    scope: info.scope,
    availableInCommandDialog: info.available_in_command_dialog,
    icon: info.icon,
    command: info.command ?? null,
    projectRoot: null,
    projectName: null,
  };
}

/** Swift `AgentDefinition(name:agentType:...)` with its defaults. */
export function makeAgentDefinition(fields: {
  name: string;
  agentType?: string;
  template?: string | null;
  inlinePrompt?: string | null;
  tags?: readonly string[];
  scope?: string;
  availableInCommandDialog?: boolean;
  icon?: string | null;
  command?: string | null;
}): AgentDefinition {
  const scope = fields.scope ?? 'local';
  return {
    id: `${scope}:${fields.name}`,
    name: fields.name,
    agentType: fields.agentType ?? 'claude',
    template: fields.template ?? null,
    inlinePrompt: fields.inlinePrompt ?? null,
    tags: [...(fields.tags ?? [])],
    scope,
    availableInCommandDialog: fields.availableInCommandDialog ?? true,
    icon: fields.icon ?? null,
    command: fields.command ?? null,
    projectRoot: null,
    projectName: null,
  };
}

// MARK: - SwarmDefinition

/** Swift `SwarmRosterItem`: `id` is a fresh UUID, so list rows keep their identity while edited. */
export interface SwarmRosterItem {
  readonly id: string;
  readonly agentDef: string;
  readonly role: string;
  readonly quantity: number;
}

let rosterCounter = 0;

function newRosterId(): string {
  rosterCounter += 1;
  return typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
    ? crypto.randomUUID()
    : `roster-${String(rosterCounter)}`;
}

export function makeSwarmRosterItem(fields: {
  agentDef: string;
  role: string;
  quantity: number;
}): SwarmRosterItem {
  return { id: newRosterId(), ...fields };
}

/** Swift `SwarmDefinition`: worktrees, each running the whole roster. */
export interface SwarmDefinition {
  readonly id: string;
  readonly name: string;
  readonly worktreeCount: number;
  readonly worktreeTemplate: string;
  readonly roster: readonly SwarmRosterItem[];
  readonly includeTerminal: boolean;
  readonly scope: string;
  readonly projectRoot: string | null;
  readonly projectName: string | null;
}

/** Swift `SwarmDefinition.totalAgents`. */
export function swarmDefinitionTotalAgents(swarm: SwarmDefinition): number {
  return swarm.worktreeCount * swarm.roster.reduce((sum, item) => sum + item.quantity, 0);
}

/** Swift `SwarmDefinition(from:)`. A roster entry without a quantity launches one agent. */
export function swarmDefinitionFromInfo(info: SwarmDefInfo): SwarmDefinition {
  return {
    id: `${info.scope}:${info.name}`,
    name: info.name,
    worktreeCount: info.worktree_count,
    worktreeTemplate: info.worktree_template,
    roster: info.roster.map((entry) =>
      makeSwarmRosterItem({
        agentDef: entry.agent_def,
        role: entry.role,
        quantity: entry.quantity ?? 1,
      }),
    ),
    includeTerminal: info.include_terminal,
    scope: info.scope,
    projectRoot: null,
    projectName: null,
  };
}

/** Swift `SwarmDefinition(name:worktreeCount:...)` with its defaults. */
export function makeSwarmDefinition(fields: {
  name: string;
  worktreeCount?: number;
  worktreeTemplate?: string;
  roster?: readonly SwarmRosterItem[];
  includeTerminal?: boolean;
  scope?: string;
}): SwarmDefinition {
  const scope = fields.scope ?? 'local';
  return {
    id: `${scope}:${fields.name}`,
    name: fields.name,
    worktreeCount: fields.worktreeCount ?? 1,
    worktreeTemplate: fields.worktreeTemplate ?? '',
    roster: [...(fields.roster ?? [])],
    includeTerminal: fields.includeTerminal ?? false,
    scope,
    projectRoot: null,
    projectName: null,
  };
}

// MARK: - Choices

/** Swift `PromptScopeChoice`. */
export const PROMPT_SCOPE_CHOICES = ['global', 'project'] as const;
export type PromptScopeChoice = (typeof PROMPT_SCOPE_CHOICES)[number];

export const PromptScope = {
  /** `rawValue.capitalized`. */
  title: (choice: PromptScopeChoice): string => (choice === 'global' ? 'Global' : 'Project'),
  /** The daemon's scope: a project's definitions are `local`. */
  wireValue: (choice: PromptScopeChoice): string => (choice === 'global' ? 'global' : 'local'),
} as const;

/** Swift `AgentTypes`: the types a prompt or agent can name; `''` is "Any". */
export const AgentTypes = {
  all: ['claude', 'codex', 'opencode', 'terminal'] as readonly string[],
  withAny: ['', 'claude', 'codex', 'opencode', 'terminal'] as readonly string[],
} as const;

/** Swift `AgentPromptSourceMode`. */
export const AGENT_PROMPT_SOURCE_MODES = ['library', 'inline'] as const;
export type AgentPromptSourceMode = (typeof AGENT_PROMPT_SOURCE_MODES)[number];

export function agentPromptSourceModeTitle(mode: AgentPromptSourceMode): string {
  return mode === 'library' ? 'From Prompts' : 'Inline';
}

/** Swift `AgentsHubTab` (AgentsHubView.swift). */
export const AGENTS_HUB_TABS = ['prompts', 'agents', 'swarms', 'triggers'] as const;
export type AgentsHubTab = (typeof AGENTS_HUB_TABS)[number];

export function agentsHubTabTitle(tab: AgentsHubTab): string {
  return tab.charAt(0).toUpperCase() + tab.slice(1);
}

/** The badge a list row shows for a definition's scope: Global, else its project's name. */
export function scopeBadge(scope: string, projectName: string | null): string {
  return scope === 'global' ? 'Global' : (projectName ?? 'Project');
}

/** The comma-separated tag field as the tags it names (AgentCreationSheet). */
export function parseTags(text: string): string[] {
  return text
    .split(',')
    .map((tag) => tag.trim())
    .filter((tag) => tag !== '');
}

/** `URL(fileURLWithPath:).lastPathComponent` for either separator. */
export function projectNameOf(root: string): string {
  const parts = root.split(/[\\/]/).filter((part) => part !== '');
  return parts.at(-1) ?? root;
}
