// Test builders mirroring the Mac's `AgentDefinition(...)` / `SwarmDefinition(...)` initializers
// and their defaults, producing the daemon payloads the palette works with.
import type { AgentDefInfo, SwarmDefInfo } from '../protocol';

export function agentDef(
  name: string,
  overrides: Partial<Omit<AgentDefInfo, 'name'>> = {},
): AgentDefInfo {
  return {
    name,
    agent_type: 'claude',
    template: null,
    inline_prompt: null,
    tags: [],
    scope: 'local',
    available_in_command_dialog: true,
    icon: null,
    command: null,
    ...overrides,
  };
}

export function swarmDef(
  name: string,
  overrides: Partial<Omit<SwarmDefInfo, 'name'>> = {},
): SwarmDefInfo {
  return {
    name,
    worktree_count: 1,
    worktree_template: '',
    roster: [],
    include_terminal: false,
    scope: 'local',
    ...overrides,
  };
}
