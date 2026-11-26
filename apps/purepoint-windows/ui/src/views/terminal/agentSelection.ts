// Which agents' terminals the detail area shows (WIN-7.3). Until the pane grid (WIN-7.4) and the
// sidebar outline (WIN-7.2) select agents through workspaces, the interim sidebar agent list
// selects them here: a click shows one agent, Ctrl+click adds or removes one from the set.
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
