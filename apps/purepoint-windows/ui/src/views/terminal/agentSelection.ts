// Which agents' terminals the detail area shows (WIN-7.3). Until the pane grid (WIN-7.4) shows a
// workspace's agents itself, the sidebar's workspace rows select them here (ContentView's
// `selectSidebarAgents`): a click shows one row's agents, Ctrl+click adds or removes them.
import { create } from 'zustand';

export interface AgentSelectionState {
  readonly agentIds: readonly string[];
  select(agentId: string, additive?: boolean): void;
  clear(): void;
}

export const useAgentSelection = create<AgentSelectionState>()((set, get) => ({
  agentIds: [],
  select(agentId, additive = false) {
    const current = get().agentIds;
    if (!additive) {
      set({ agentIds: [agentId] });
    } else if (current.includes(agentId)) {
      set({ agentIds: current.filter((id) => id !== agentId) });
    } else {
      set({ agentIds: [...current, agentId] });
    }
  },
  clear() {
    if (get().agentIds.length > 0) set({ agentIds: [] });
  },
}));
