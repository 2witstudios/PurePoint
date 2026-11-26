// Interim agent terminals (WIN-7.3): the sidebar's agent list and the detail area's terminals for
// the selected agents. The sidebar outline (WIN-7.2) and the pane grid (WIN-7.4) replace both;
// `TerminalPane` is what the grid places.
import { useEffect } from 'react';
import { useStore } from 'zustand';
import { isAgentStatusAlive } from '../../model/agentStatus.ts';
import { useAppState } from '../shell/appContext.ts';
import { useAgentSelection } from './agentSelection.ts';
import { TerminalPane } from './TerminalPane.tsx';
import { configureTerminals } from './terminals.ts';

/** Every agent of every project, for the interim sidebar (the e2e tests click these). */
export function AgentListPlaceholder() {
  const app = useAppState();
  useStore(app.store, (state) => state.projectRevision);
  const selected = useAgentSelection((state) => state.agentIds);
  const agents = app
    .projectSnapshots()
    .flatMap((project) => [
      ...project.rootAgents,
      ...project.worktrees.flatMap((worktree) => worktree.agents),
    ]);
  if (agents.length === 0) return null;
  return (
    <ul data-testid="agent-list" className="sidebar-agent-list" aria-label="Agents">
      {agents.map((agent) => (
        <li key={agent.id}>
          <button
            type="button"
            className="sidebar-nav-row"
            data-agent-id={agent.id}
            aria-pressed={selected.includes(agent.id)}
            title={agent.id}
            onClick={(event) => {
              useAgentSelection.getState().select(agent.id, event.ctrlKey);
            }}
          >
            {agent.name === '' ? agent.id : agent.name}
          </button>
        </li>
      ))}
    </ul>
  );
}

/** The selected agents' terminals, in a grid of roughly square cells. */
export function AgentTerminalsView({ agentIds }: { agentIds: readonly string[] }) {
  const app = useAppState();
  useEffect(() => {
    configureTerminals({
      isAgentAlive: (agentId) => {
        const agent = app.agent(agentId);
        return agent !== null && isAgentStatusAlive(agent.status);
      },
    });
  }, [app]);
  const columns = Math.ceil(Math.sqrt(agentIds.length));
  const last = agentIds.at(-1);
  return (
    <div
      className="agent-terminals"
      data-testid="agent-terminals"
      style={{ gridTemplateColumns: `repeat(${String(columns)}, minmax(0, 1fr))` }}
    >
      {agentIds.map((agentId) => (
        <TerminalPane key={agentId} agentId={agentId} autoFocus={agentId === last} />
      ))}
    </div>
  );
}
