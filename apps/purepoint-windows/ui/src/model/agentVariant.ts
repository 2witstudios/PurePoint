// Port of apps/purepoint-macos/purepoint-macos/Models/AgentVariant.swift (WIN-6.6).
//
// The Mac keeps the agents added in Settings › Agents in a mutable static that
// `AgentConfigState` updates; here the caller passes them to `allVariants` explicitly.
import type { SfSymbolName } from '../theme/icons';

export type AgentVariantKind = 'agent' | 'terminal' | 'worktree';

export interface AgentVariant {
  readonly id: string;
  readonly displayName: string;
  /** SF Symbol name; the view maps it to a Lucide icon (DEC-13). */
  readonly icon: SfSymbolName;
  readonly subtitle: string;
  readonly promptPlaceholder: string;
  readonly kind: AgentVariantKind;
}

export const CLAUDE: AgentVariant = {
  id: 'claude',
  displayName: 'Claude',
  icon: 'circle.fill',
  subtitle: 'AI coding agent',
  promptPlaceholder: 'Enter prompt...',
  kind: 'agent',
};

export const CODEX: AgentVariant = {
  id: 'codex',
  displayName: 'Codex',
  icon: 'diamond',
  subtitle: 'OpenAI coding CLI',
  promptPlaceholder: 'Enter prompt...',
  kind: 'agent',
};

export const OPENCODE: AgentVariant = {
  id: 'opencode',
  displayName: 'OpenCode',
  icon: 'diamond',
  subtitle: 'Open-source agent',
  promptPlaceholder: 'Enter prompt...',
  kind: 'agent',
};

export const TERMINAL: AgentVariant = {
  id: 'terminal',
  displayName: 'Terminal',
  icon: 'terminal',
  subtitle: 'Shell session',
  promptPlaceholder: 'Enter initial command (optional)...',
  kind: 'terminal',
};

/** The id matches Claude's: it is passed to the daemon as the agent type to spawn. */
export const WORKTREE: AgentVariant = {
  id: 'claude',
  displayName: 'Worktree',
  icon: 'arrow.triangle.branch',
  subtitle: 'Isolated branch',
  promptPlaceholder: 'Enter prompt...',
  kind: 'worktree',
};

/** An agent the user added: its type is its name, and it runs `command`. */
export function addedAgentVariant(name: string, command: string): AgentVariant {
  return {
    id: name,
    displayName: name,
    icon: 'chevron.left.forwardslash.chevron.right',
    subtitle: command,
    promptPlaceholder: 'Enter prompt...',
    kind: 'agent',
  };
}

/** Built-in agents, then the user's added agents, then Terminal. */
export function allVariants(addedAgents: readonly AgentVariant[] = []): AgentVariant[] {
  return [CLAUDE, CODEX, OPENCODE, ...addedAgents, TERMINAL];
}

export function variantsWithWorktree(addedAgents: readonly AgentVariant[] = []): AgentVariant[] {
  return [...allVariants(addedAgents), WORKTREE];
}
