// The sidebar outline's rows (WIN-7.2), by the stable test ids the outline gives them:
// `sidebar-row-<kind>-<name>` (a project by its folder name, a worktree by its name, an agent's
// workspace by its title), and an agent's workspace row also by `data-agent-id`.

import { $ } from '@wdio/globals';

export function projectRow(name: string) {
  return $(`[data-testid="sidebar-row-project-${name}"]`);
}

export function worktreeRow(name: string) {
  return $(`[data-testid="sidebar-row-worktree-${name}"]`);
}

/** The CSS selector of the workspace row whose first pane holds `agentId`. */
export function agentRowSelector(agentId: string): string {
  return `[data-testid^="sidebar-row-agent-"][data-agent-id="${agentId}"]`;
}

export function agentRow(agentId: string) {
  return $(agentRowSelector(agentId));
}
