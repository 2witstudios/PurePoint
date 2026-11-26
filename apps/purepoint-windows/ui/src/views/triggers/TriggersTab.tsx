// The Agents hub's Triggers tab (WIN-8.9): Swift `AgentsHubView.triggersContent` and
// `TriggerListRow`, with the creation sheet it presents.
import { useStore } from 'zustand';
import { scopeBadge } from '../../model/agentsHubModels.ts';
import { stepCountLabel, TriggerEvents, type TriggerItem } from '../../model/triggerItem.ts';
import { selectedTriggerOf, type TriggersState } from '../../state/triggersState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { Badge, MasterDetail } from '../agents/hubComponents.tsx';
import { InlineErrorBanner } from '../shell/DaemonErrorBanner.tsx';
import { TriggerCreationSheet } from './TriggerCreationSheet.tsx';
import { TriggerDetailView } from './TriggerDetailView.tsx';
import './triggers.css';

export function TriggersTab({
  state,
  projectRoots,
}: {
  state: TriggersState;
  projectRoots: readonly string[];
}) {
  const triggers = useStore(state.store, (s) => s.triggers);
  const selectedId = useStore(state.store, (s) => s.selectedTriggerId);
  const selected = useStore(state.store, selectedTriggerOf);
  const showingSheet = useStore(state.store, (s) => s.showingCreationSheet);
  const error = useStore(state.store, (s) => s.error);
  return (
    <>
      {error !== null && (
        <InlineErrorBanner
          message={error}
          onDismiss={() => {
            state.dismissError();
          }}
        />
      )}
      <MasterDetail
        title="Triggers"
        items={triggers}
        selectedId={selectedId}
        onSelect={(id) => {
          state.select(id);
        }}
        onCreate={() => {
          state.setShowingCreationSheet(true);
        }}
        createLabel="New trigger"
        emptyListMessage="No triggers yet"
        emptyDetailMessage="Select a trigger or create one to get started."
        row={(trigger) => <TriggerListRow trigger={trigger} />}
        detail={
          selected === null ? null : (
            <TriggerDetailView trigger={selected} state={state} projectRoots={projectRoots} />
          )
        }
      />
      {showingSheet && (
        <TriggerCreationSheet
          state={state}
          onDismiss={() => {
            state.setShowingCreationSheet(false);
          }}
        />
      )}
    </>
  );
}

function TriggerListRow({ trigger }: { trigger: TriggerItem }) {
  return (
    <>
      <span className="hub-row-line">
        <Icon
          name={TriggerEvents.icon(trigger.event)}
          size={12}
          className={`trigger-tint-${TriggerEvents.tint(trigger.event)}`}
        />
        <span className="hub-row-name">{trigger.name}</span>
        <Badge
          text={scopeBadge(trigger.scope, trigger.projectName)}
          tint={trigger.scope === 'global' ? 'blue' : 'green'}
        />
      </span>
      <span className="hub-row-subtitle">
        {TriggerEvents.label(trigger.event)} · {stepCountLabel(trigger.sequence.length)}
      </span>
    </>
  );
}
