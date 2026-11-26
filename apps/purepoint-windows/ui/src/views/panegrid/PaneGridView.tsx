// Port of apps/purepoint-macos/purepoint-macos/Views/PaneGrid/PaneGridView.swift (WIN-7.4): renders a
// workspace's split tree (model/paneSplit.ts) as nested draggable splits with a pane cell per leaf.
//
// Differences from Swift: SwiftUI needs every root wrapped in the same `DraggableSplit` type so a
// 2 -> 1 pane close does not rebuild the hierarchy; React reconciles by element type and key, so
// each subtree is keyed by its first leaf instead and a lone leaf renders bare.
import { type ReactNode, useEffect } from 'react';
import { useStore } from 'zustand';
import { isAgentStatusAlive } from '../../model/agentStatus.ts';
import { firstLeafId, type PaneSplitNode } from '../../model/paneSplit.ts';
import type { WorkspaceRegistry } from '../../state/workspaceRegistry.ts';
import { useAppState } from '../shell/appContext.ts';
import { configureTerminals } from '../terminal/terminals.ts';
import { DraggableSplit } from './DraggableSplit.tsx';
import { PaneCellView } from './PaneCellView.tsx';
import './panegrid.css';

export interface PaneGridViewProps {
  workspaceId: string;
  registry: WorkspaceRegistry;
}

export function PaneGridView({ workspaceId, registry }: PaneGridViewProps) {
  const app = useAppState();
  const workspace = useStore(registry.store, () => registry.workspace(workspaceId));

  // The terminals read agents' live status and owning machine from the app state.
  useEffect(() => {
    configureTerminals({
      isAgentAlive: (agentId) => {
        const agent = app.agent(agentId);
        return agent !== null && isAgentStatusAlive(agent.status);
      },
    });
  }, [app]);

  if (workspace === null) return <div className="pane-grid" />;

  const nodeView = (node: PaneSplitNode): ReactNode => {
    if (node.type === 'leaf') {
      return (
        <PaneCellView
          key={node.id}
          workspaceId={workspaceId}
          leafId={node.id}
          isFocused={node.id === workspace.focusedLeafId}
          registry={registry}
        />
      );
    }
    const splitId = firstLeafId(node.first);
    return (
      <DraggableSplit
        key={`split-${String(splitId)}`}
        axis={node.axis}
        ratio={node.ratio}
        onRatioChanged={(ratio) => {
          registry.setRatio(ratio, workspaceId, splitId);
        }}
        first={nodeView(node.first)}
        second={nodeView(node.second)}
      />
    );
  };

  return (
    <div className="pane-grid" data-testid="pane-grid" data-workspace-id={workspaceId}>
      {nodeView(workspace.root)}
    </div>
  );
}
