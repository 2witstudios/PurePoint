// Port of apps/purepoint-macos/purepoint-macos/Views/Triggers/TriggerDetailView.swift (WIN-8.9),
// with an Assign section the Mac lacks: it gives a live agent the trigger (`assign_trigger`).
import { useState } from 'react';
import { useStore } from 'zustand';
import type { TriggerActionPayload } from '../../protocol';
import { TriggerEvents, type TriggerItem } from '../../model/triggerItem.ts';
import type { AppState } from '../../state/appState.ts';
import type { TriggersState } from '../../state/triggersState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { SectionLabel, Select } from '../agents/hubComponents.tsx';
import { useAppState } from '../shell/appContext.ts';

/** A live agent a trigger can be assigned to. */
export interface AssignableAgent {
  readonly projectRoot: string;
  readonly agentId: string;
  readonly title: string;
}

/**
 * The agents of the projects a trigger applies to: its own project for a project trigger,
 * every local project for a global one.
 */
export function assignableAgents(app: AppState, trigger: TriggerItem): AssignableAgent[] {
  const projects = app.projects.filter(
    (p) => trigger.projectRoot === null || p.projectRoot === trigger.projectRoot,
  );
  return projects.flatMap((project) =>
    [...project.rootAgents, ...project.worktrees.flatMap((wt) => wt.agents)].map((agent) => ({
      projectRoot: project.projectRoot,
      agentId: agent.id,
      title: `${agent.name === '' ? agent.id : agent.name} (${agent.agentType}${
        projects.length > 1 ? `, ${project.projectName}` : ''
      })`,
    })),
  );
}

export function TriggerDetailView({
  trigger,
  state,
  projectRoots,
}: {
  trigger: TriggerItem;
  state: TriggersState;
  projectRoots: readonly string[];
}) {
  const app = useAppState();
  const tint = `trigger-tint-${TriggerEvents.tint(trigger.event)}`;
  const variables = Object.entries(trigger.variables).sort(([a], [b]) =>
    a < b ? -1 : a > b ? 1 : 0,
  );
  return (
    <div className="hub-detail-scroll" data-testid="trigger-detail">
      <header className="trigger-header">
        <div className="hub-row-line">
          <Icon name={TriggerEvents.icon(trigger.event)} size={16} className={tint} />
          <h3 className="trigger-title">{trigger.name}</h3>
          <span className="trigger-scope">{trigger.scope}</span>
        </div>
        {trigger.description !== null && trigger.description !== '' && (
          <p className="trigger-description">{trigger.description}</p>
        )}
        <div className="hub-row-line trigger-tertiary">
          <Icon name="bolt" size={11} />
          <span>Fires on {TriggerEvents.label(trigger.event)}</span>
        </div>
      </header>
      <hr className="hub-divider" />
      <section className="hub-code-section" aria-label="Sequence">
        <SectionLabel>Sequence</SectionLabel>
        {trigger.sequence.map((action, index) => (
          // The sequence has no ids; Swift keys its rows by offset too.
          <StepRow key={index} index={index} action={action} />
        ))}
      </section>
      {variables.length > 0 && (
        <>
          <hr className="hub-divider" />
          <section className="hub-code-section" aria-label="Variables">
            <SectionLabel>Variables</SectionLabel>
            {variables.map(([key, value]) => (
              <div className="trigger-variable" key={key}>
                <span className="trigger-variable-key">{key}</span>
                <span className="trigger-tertiary">=</span>
                <span className="hub-secondary">{value}</span>
              </div>
            ))}
          </section>
        </>
      )}
      <hr className="hub-divider" />
      <AssignSection key={trigger.id} trigger={trigger} state={state} app={app} />
      <hr className="hub-divider" />
      <div className="hub-actions">
        <span className="hub-spacer" />
        <button
          type="button"
          className="hub-button hub-destructive"
          onClick={() => {
            const deleteRoot = trigger.projectRoot ?? app.projects[0]?.projectRoot ?? '';
            void state.deleteTrigger({
              projectRoot: deleteRoot,
              projectRoots,
              name: trigger.name,
              scope: trigger.scope,
            });
          }}
        >
          Delete Trigger
        </button>
      </div>
    </div>
  );
}

function StepRow({ index, action }: { index: number; action: TriggerActionPayload }) {
  return (
    <div className="trigger-step" data-testid="trigger-step-row">
      <span className="trigger-step-index">{index + 1}</span>
      <div className="trigger-step-body">
        {action.inject != null && (
          <span className="hub-row-line">
            <Icon name="text.cursor" size={10} className="trigger-tint-blue" />
            <code className="trigger-code">{action.inject}</code>
          </span>
        )}
        {action.gate != null && (
          <span className="hub-row-line">
            <Icon name="shield.checkered" size={10} className="trigger-tint-orange" />
            <code className="trigger-code">{action.gate.run}</code>
            {action.gate.expect_exit != null && (
              <span className="trigger-tertiary trigger-tiny">exit={action.gate.expect_exit}</span>
            )}
          </span>
        )}
        {action.max_retries != null && (
          <span className="trigger-tertiary trigger-tiny">max retries: {action.max_retries}</span>
        )}
      </div>
    </div>
  );
}

function AssignSection({
  trigger,
  state,
  app,
}: {
  trigger: TriggerItem;
  state: TriggersState;
  app: AppState;
}) {
  // Agents come and go with each project's status; follow them.
  useStore(app.store, (s) => s.projectRevision);
  const lastAssignment = useStore(state.store, (s) => s.lastAssignment);
  const agents = assignableAgents(app, trigger);
  const [picked, setPicked] = useState<string | null>(null);
  const [assigning, setAssigning] = useState(false);
  const selected = agents.find((a) => a.agentId === picked) ?? agents[0] ?? null;
  const confirmed =
    lastAssignment !== null &&
    lastAssignment.triggerName === trigger.name &&
    agents.some((a) => a.agentId === lastAssignment.agentId)
      ? lastAssignment
      : null;

  return (
    <section className="hub-code-section" aria-label="Assign">
      <SectionLabel>Assign to agent</SectionLabel>
      {agents.length === 0 ? (
        <span className="hub-secondary hub-small">
          No running agents to assign this trigger to.
        </span>
      ) : (
        <div className="hub-actions">
          <Select
            label="Agent"
            value={selected?.agentId ?? ''}
            options={agents.map((a) => ({ value: a.agentId, title: a.title }))}
            onChange={setPicked}
          />
          <button
            type="button"
            className="hub-button"
            disabled={selected === null || assigning}
            onClick={() => {
              if (selected === null) return;
              setAssigning(true);
              void state
                .assignTrigger({
                  projectRoot: selected.projectRoot,
                  agentId: selected.agentId,
                  triggerName: trigger.name,
                })
                .then(() => {
                  setAssigning(false);
                });
            }}
          >
            Assign
          </button>
          {confirmed !== null && (
            <span className="hub-secondary hub-small" data-testid="trigger-assigned">
              Assigned to {confirmed.agentId} ({confirmed.sequenceLength}{' '}
              {confirmed.sequenceLength === 1 ? 'step' : 'steps'})
            </span>
          )}
        </div>
      )}
    </section>
  );
}
