// Interim agent terminals (WIN-7.3): the detail area's terminals for the agents the sidebar
// selected. The pane grid (WIN-7.4) replaces it; `TerminalPane` is what the grid places.
import { useEffect } from 'react';
import { isAgentStatusAlive } from '../../model/agentStatus.ts';
import { useAppState } from '../shell/appContext.ts';
import { TerminalPane } from './TerminalPane.tsx';
import { configureTerminals } from './terminals.ts';

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
