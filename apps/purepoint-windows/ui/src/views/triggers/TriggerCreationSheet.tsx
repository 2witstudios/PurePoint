// Port of apps/purepoint-macos/purepoint-macos/Views/Triggers/TriggerCreationSheet.swift (WIN-8.9).
import { useState } from 'react';
import {
  buildTriggerSequence,
  EMPTY_STEP,
  TRIGGER_EVENTS,
  TriggerEvents,
  triggerPreviewText,
  type TriggerEvent,
  type TriggerStepDraft,
} from '../../model/triggerItem.ts';
import type { TriggersState } from '../../state/triggersState.ts';
import { Icon } from '../../theme/Icon.tsx';
import {
  Field,
  ProjectPicker,
  SectionLabel,
  Segmented,
  Select,
  Sheet,
  TextInput,
} from '../agents/hubComponents.tsx';
import { useAppState } from '../shell/appContext.ts';

export function TriggerCreationSheet({
  state,
  onDismiss,
}: {
  state: TriggersState;
  onDismiss: () => void;
}) {
  const app = useAppState();
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [event, setEvent] = useState<TriggerEvent>('agent_idle');
  const [scope, setScope] = useState('global');
  const [steps, setSteps] = useState<TriggerStepDraft[]>([EMPTY_STEP]);
  const [projectRoot, setProjectRoot] = useState(() => app.activeLocalProjectRoot ?? '');

  const updateStep = (index: number, change: Partial<TriggerStepDraft>) => {
    setSteps((all) => all.map((step, i) => (i === index ? { ...step, ...change } : step)));
  };

  const create = () => {
    const saveRoot = scope === 'local' ? projectRoot : (app.projects[0]?.projectRoot ?? '');
    void (async () => {
      await state.saveTrigger({
        projectRoot: saveRoot,
        projectRoots: app.libraryProjectRoots,
        name: name === '' ? 'Untitled Trigger' : name,
        description: description === '' ? null : description,
        on: event,
        sequence: buildTriggerSequence(steps),
        variables: {},
        scope,
      });
      onDismiss();
    })();
  };

  return (
    <Sheet
      title="New Trigger"
      canSubmit={name.trim() !== ''}
      onCancel={onDismiss}
      onSubmit={create}
      footerNote={
        <div className="hub-sheet-preview" data-testid="trigger-preview">
          <Icon
            name={TriggerEvents.icon(event)}
            size={11}
            className={`trigger-tint-${TriggerEvents.tint(event)}`}
          />
          <span>{triggerPreviewText(event, steps.length, scope)}</span>
        </div>
      }
    >
      <TextInput label="Name" value={name} onChange={setName} />
      <TextInput
        label="Description"
        placeholder="Description (optional)"
        value={description}
        onChange={setDescription}
      />
      <Field label="Event">
        <Segmented
          label="Event"
          value={event}
          options={TRIGGER_EVENTS}
          title={TriggerEvents.label}
          onChange={setEvent}
        />
      </Field>
      <Field label="Scope">
        <Select
          label="Scope"
          value={scope}
          options={[
            { value: 'global', title: 'Global' },
            { value: 'local', title: 'Local' },
          ]}
          onChange={setScope}
        />
      </Field>
      {scope === 'local' && (
        <ProjectPicker projects={app.projects} value={projectRoot} onChange={setProjectRoot} />
      )}
      <div className="hub-roster" aria-label="Sequence">
        <SectionLabel>Sequence</SectionLabel>
        {steps.map((step, index) => {
          const n = String(index + 1);
          return (
            // Steps have no identity of their own (Swift keys them by offset too).
            <div className="trigger-step-editor" key={index} data-testid="trigger-step">
              <div className="hub-row-line">
                <span className="trigger-step-title">Step {n}</span>
                {steps.length > 1 && (
                  <button
                    type="button"
                    className="hub-icon-button hub-destructive"
                    aria-label={`Remove step ${n}`}
                    onClick={() => {
                      setSteps((all) => all.filter((_, i) => i !== index));
                    }}
                  >
                    <Icon name="minus.circle" size={11} />
                  </button>
                )}
              </div>
              <TextInput
                label={`Inject text ${n}`}
                placeholder="Inject text"
                value={step.inject}
                onChange={(inject) => {
                  updateStep(index, { inject });
                }}
              />
              <div className="hub-roster-row">
                <TextInput
                  label={`Gate command ${n}`}
                  placeholder="Gate command (optional)"
                  value={step.gateRun}
                  onChange={(gateRun) => {
                    updateStep(index, { gateRun });
                  }}
                />
                <TextInput
                  label={`Exit ${n}`}
                  placeholder="Exit"
                  className="trigger-narrow"
                  value={step.gateExitCode}
                  onChange={(gateExitCode) => {
                    updateStep(index, { gateExitCode });
                  }}
                />
                <TextInput
                  label={`Retries ${n}`}
                  placeholder="Retries"
                  className="trigger-narrow"
                  value={step.maxRetries}
                  onChange={(maxRetries) => {
                    updateStep(index, { maxRetries });
                  }}
                />
              </div>
            </div>
          );
        })}
        <button
          type="button"
          className="hub-link-button"
          onClick={() => {
            setSteps((all) => [...all, EMPTY_STEP]);
          }}
        >
          <Icon name="plus.circle" size={12} />
          Add Step
        </button>
      </div>
    </Sheet>
  );
}
