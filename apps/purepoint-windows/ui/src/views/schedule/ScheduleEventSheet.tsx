// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/ScheduleEventSheet.swift (WIN-8.17).
//
// The create / edit sheet: name, trigger (agent definition, swarm definition or inline prompt
// with its agent type), time, recurrence, target, scope and project, color, a next-run preview,
// and Delete (with confirmation), Cancel and Create / Save. Create / Save needs a name.
//
// Windows additions: an Enabled switch while editing (Swift's `toggleSchedule` has no control on
// the Mac), which sends enable_schedule / disable_schedule at once; a failed request keeps the
// sheet open with the daemon's message instead of closing silently; and an edit keeps the
// schedule's root / agent name, which the Mac sheet drops.
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type SyntheticEvent,
} from 'react';
import { useStore } from 'zustand';
import {
  formatTime,
  formatWeekdayAtTime,
  fromDateTimeLocalValue,
  isSameDay,
  toDateTimeLocalValue,
} from '../../model/scheduleCalendar.ts';
import {
  EventColor,
  eventColorCss,
  EVENT_COLOR_NAMES,
  RECURRENCE_LABEL,
  RECURRENCE_RULES,
  SCHEDULE_TYPE_ICON,
  SCHEDULE_TYPE_LABEL,
  SCHEDULE_TYPES,
  type RecurrenceRule,
  type ScheduleEvent,
  type ScheduleType,
} from '../../model/scheduleEvent.ts';
import type { ScheduleTriggerPayload } from '../../protocol';
import type { ScheduleState } from '../../state/scheduleState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { useAppState } from '../shell/appContext.ts';
import './ScheduleEventSheet.css';

/** Swift `nextRunPreview`. */
export function nextRunPreview(
  date: Date,
  recurrence: RecurrenceRule,
  now: Date,
  locale?: string,
): string {
  if (recurrence === 'none') return `Runs once: ${formatWeekdayAtTime(date, locale)}`;
  if (isSameDay(date, now)) return `Next run: Today at ${formatTime(date, locale)}`;
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  if (isSameDay(date, tomorrow)) return `Next run: Tomorrow at ${formatTime(date, locale)}`;
  return `Next run: ${formatWeekdayAtTime(date, locale)}`;
}

/** Swift `buildTrigger`. */
export function buildTrigger(
  type: ScheduleType,
  triggerName: string,
  triggerPrompt: string,
  triggerAgent: string,
): ScheduleTriggerPayload {
  switch (type) {
    case 'agent':
      return { type: 'agent_def', name: triggerName };
    case 'swarm':
      return { type: 'swarm_def', name: triggerName, vars: {} };
    case 'inlinePrompt':
      return { type: 'inline_prompt', prompt: triggerPrompt, agent: triggerAgent };
  }
}

interface FormFields {
  name: string;
  type: ScheduleType;
  date: Date;
  recurrence: RecurrenceRule;
  target: string;
  colorIndex: number | null;
  scope: string;
  projectRoot: string;
  triggerName: string;
  triggerPrompt: string;
  triggerAgent: string;
  enabled: boolean;
}

/** Swift `prefillFields`. */
function prefill(
  editing: ScheduleEvent | null,
  prefillDate: Date | null,
  activeRoot: string,
  now: Date,
): FormFields {
  const fields: FormFields = {
    name: '',
    type: 'agent',
    date: prefillDate ?? now,
    recurrence: 'none',
    target: '',
    colorIndex: null,
    scope: 'global',
    projectRoot: activeRoot,
    triggerName: '',
    triggerPrompt: '',
    triggerAgent: 'claude',
    enabled: true,
  };
  if (editing === null) return fields;
  fields.name = editing.name;
  fields.type = editing.type;
  fields.date = editing.date;
  fields.recurrence = editing.recurrence;
  fields.target = editing.target;
  fields.colorIndex = EventColor.savedIndex(editing.name);
  fields.scope = editing.scope;
  fields.enabled = editing.enabled;
  if (editing.projectRoot !== null) fields.projectRoot = editing.projectRoot;
  const trigger = editing.trigger;
  if (trigger !== null) {
    switch (trigger.type) {
      case 'agent_def':
      case 'swarm_def':
        fields.triggerName = trigger.name;
        break;
      case 'inline_prompt':
        fields.triggerPrompt = trigger.prompt;
        fields.triggerAgent = trigger.agent ?? 'claude';
        break;
    }
  }
  return fields;
}

export interface ScheduleEventSheetProps {
  state: ScheduleState;
  editingEvent: ScheduleEvent | null;
  prefillDate: Date | null;
}

export function ScheduleEventSheet({ state, editingEvent, prefillDate }: ScheduleEventSheetProps) {
  const app = useAppState();
  const projects = useStore(app.store, (s) => s.projects);
  const error = useStore(state.store, (s) => s.error);
  const isEditing = editingEvent !== null;
  const [fields, setFields] = useState<FormFields>(() =>
    prefill(editingEvent, prefillDate, app.activeLocalProjectRoot ?? '', state.nowDate()),
  );
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const panel = useRef<HTMLFormElement>(null);
  const projectRoots = useMemo(() => projects.map((p) => p.projectRoot), [projects]);
  const firstRoot = projects[0]?.projectRoot ?? '';

  useEffect(() => {
    panel.current?.querySelector<HTMLInputElement>('input[name="name"]')?.focus();
  }, []);

  const set = <K extends keyof FormFields>(key: K, value: FormFields[K]) => {
    setFields((current) => ({ ...current, [key]: value }));
  };

  const dismiss = () => {
    state.dismissSheet();
  };

  const canSave = fields.name.trim() !== '' && !busy;

  /** Swift `saveAndDismiss`. */
  const saveAndDismiss = async () => {
    setBusy(true);
    const effectiveName = fields.name === '' ? 'Untitled Schedule' : fields.name;
    const saveRoot = fields.scope === 'local' ? fields.projectRoot : firstRoot;
    let ok = true;

    // The name is the backend key: a rename deletes the old entry first.
    if (editingEvent !== null && editingEvent.name !== effectiveName) {
      ok = await state.deleteSchedule(
        editingEvent.projectRoot ?? saveRoot,
        projectRoots,
        editingEvent.name,
        editingEvent.scope,
      );
      EventColor.remove(editingEvent.name);
    }

    if (fields.colorIndex !== null) {
      EventColor.save(fields.colorIndex, effectiveName);
    } else if (editingEvent !== null) {
      EventColor.remove(editingEvent.name);
      if (editingEvent.name !== effectiveName) EventColor.remove(effectiveName);
    }

    if (ok) {
      ok = await state.saveSchedule({
        projectRoot: saveRoot,
        projectRoots,
        name: effectiveName,
        enabled: fields.enabled,
        recurrence: fields.recurrence,
        startAt: fields.date,
        trigger: buildTrigger(
          fields.type,
          fields.triggerName,
          fields.triggerPrompt,
          fields.triggerAgent,
        ),
        target: fields.target,
        scope: fields.scope,
        ...(editingEvent === null
          ? {}
          : { root: editingEvent.root, agentName: editingEvent.agentName }),
      });
    }
    setBusy(false);
    if (ok) dismiss();
  };

  const deleteEvent = async () => {
    if (editingEvent === null) return;
    setBusy(true);
    EventColor.remove(editingEvent.name);
    const ok = await state.deleteSchedule(
      editingEvent.projectRoot ?? firstRoot,
      projectRoots,
      editingEvent.name,
      editingEvent.scope,
    );
    setBusy(false);
    setShowDeleteConfirm(false);
    if (ok) dismiss();
  };

  const toggleEnabled = async () => {
    if (editingEvent === null) return;
    setBusy(true);
    const ok = await state.toggleSchedule(
      editingEvent.projectRoot ?? firstRoot,
      projectRoots,
      editingEvent.name,
      fields.enabled,
    );
    if (ok) set('enabled', !fields.enabled);
    setBusy(false);
  };

  const onSubmit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (canSave) void saveAndDismiss();
  };

  const title = isEditing ? 'Edit Schedule' : 'New Schedule';

  return (
    <div
      className="schedule-sheet-backdrop"
      onPointerDown={(event) => {
        if (event.target === event.currentTarget && !showDeleteConfirm) dismiss();
      }}
    >
      <form
        className="schedule-sheet"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        data-view="ScheduleEventSheet"
        ref={panel}
        onSubmit={onSubmit}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.stopPropagation();
            if (showDeleteConfirm) setShowDeleteConfirm(false);
            else dismiss();
          }
        }}
      >
        <div className="schedule-sheet-header">{title}</div>
        <div className="schedule-sheet-form">
          <label className="schedule-field">
            <span>Name</span>
            <input
              name="name"
              type="text"
              placeholder="Name"
              value={fields.name}
              onChange={(e) => {
                set('name', e.target.value);
              }}
            />
          </label>

          <div className="schedule-field">
            <span>Trigger</span>
            <div className="segmented schedule-trigger-picker" role="tablist" aria-label="Trigger">
              {SCHEDULE_TYPES.map((type) => (
                <button
                  key={type}
                  type="button"
                  role="tab"
                  aria-selected={fields.type === type}
                  onClick={() => {
                    set('type', type);
                  }}
                >
                  <Icon name={SCHEDULE_TYPE_ICON[type]} size={11} />
                  <span>{SCHEDULE_TYPE_LABEL[type]}</span>
                </button>
              ))}
            </div>
          </div>

          {fields.type === 'inlinePrompt' ? (
            <>
              <label className="schedule-field">
                <span>Prompt</span>
                <input
                  name="prompt"
                  type="text"
                  placeholder="Prompt"
                  value={fields.triggerPrompt}
                  onChange={(e) => {
                    set('triggerPrompt', e.target.value);
                  }}
                />
              </label>
              <label className="schedule-field">
                <span>Agent Type</span>
                <input
                  name="agent"
                  type="text"
                  placeholder="Agent Type"
                  value={fields.triggerAgent}
                  onChange={(e) => {
                    set('triggerAgent', e.target.value);
                  }}
                />
              </label>
            </>
          ) : (
            <label className="schedule-field">
              <span>
                {fields.type === 'agent' ? 'Agent Definition Name' : 'Swarm Definition Name'}
              </span>
              <input
                name="definition"
                type="text"
                placeholder={
                  fields.type === 'agent' ? 'Agent Definition Name' : 'Swarm Definition Name'
                }
                value={fields.triggerName}
                onChange={(e) => {
                  set('triggerName', e.target.value);
                }}
              />
            </label>
          )}

          <label className="schedule-field">
            <span>Time</span>
            <input
              name="time"
              type="datetime-local"
              value={toDateTimeLocalValue(fields.date)}
              onChange={(e) => {
                const date = fromDateTimeLocalValue(e.target.value);
                if (date !== null) set('date', date);
              }}
            />
          </label>

          <label className="schedule-field">
            <span>Repeats</span>
            <select
              name="repeats"
              value={fields.recurrence}
              onChange={(e) => {
                set('recurrence', e.target.value as RecurrenceRule);
              }}
            >
              {RECURRENCE_RULES.map((rule) => (
                <option key={rule} value={rule}>
                  {RECURRENCE_LABEL[rule]}
                </option>
              ))}
            </select>
          </label>

          <label className="schedule-field">
            <span>Target</span>
            <input
              name="target"
              type="text"
              placeholder="Target (path or scope)"
              value={fields.target}
              onChange={(e) => {
                set('target', e.target.value);
              }}
            />
          </label>

          <label className="schedule-field">
            <span>Scope</span>
            <select
              name="scope"
              value={fields.scope}
              onChange={(e) => {
                set('scope', e.target.value);
              }}
            >
              <option value="global">Global</option>
              <option value="local">Local</option>
            </select>
          </label>

          {fields.scope === 'local' && (
            <label className="schedule-field">
              <span>Project</span>
              <select
                name="project"
                value={fields.projectRoot}
                onChange={(e) => {
                  set('projectRoot', e.target.value);
                }}
              >
                {projects.map((project) => (
                  <option key={project.projectRoot} value={project.projectRoot}>
                    {project.projectName}
                  </option>
                ))}
              </select>
            </label>
          )}

          {isEditing && (
            <label className="schedule-field schedule-enabled-field">
              <span>Enabled</span>
              <input
                name="enabled"
                type="checkbox"
                role="switch"
                checked={fields.enabled}
                disabled={busy}
                onChange={() => {
                  void toggleEnabled();
                }}
              />
            </label>
          )}

          <div className="schedule-field">
            <span>Color</span>
            <div className="schedule-color-picker" role="radiogroup" aria-label="Color">
              {EVENT_COLOR_NAMES.map((name, index) => (
                <button
                  key={name}
                  type="button"
                  role="radio"
                  aria-label={name}
                  aria-checked={fields.colorIndex === index}
                  className="schedule-color-swatch"
                  style={{ '--event-color': eventColorCss(index) } as CSSProperties}
                  onClick={() => {
                    set('colorIndex', fields.colorIndex === index ? null : index);
                  }}
                />
              ))}
            </div>
          </div>
        </div>

        <div className="schedule-sheet-preview">
          <Icon name="clock" size={11} />
          <span>
            {nextRunPreview(fields.date, fields.recurrence, state.nowDate(), state.locale)}
          </span>
        </div>

        {error !== null && (
          <div className="schedule-sheet-error" role="alert">
            {error}
          </div>
        )}

        <div className="schedule-sheet-footer">
          {isEditing && (
            <button
              type="button"
              className="schedule-button schedule-destructive"
              disabled={busy}
              onClick={() => {
                setShowDeleteConfirm(true);
              }}
            >
              Delete
            </button>
          )}
          <button type="button" className="schedule-button" onClick={dismiss}>
            Cancel
          </button>
          <span className="schedule-spacer" />
          <button type="submit" className="schedule-button schedule-default" disabled={!canSave}>
            {isEditing ? 'Save' : 'Create'}
          </button>
        </div>

        {showDeleteConfirm && (
          <div
            className="schedule-alert"
            role="alertdialog"
            aria-modal="true"
            aria-label="Delete Schedule"
          >
            <div className="schedule-alert-title">Delete Schedule</div>
            <div className="schedule-alert-message">This schedule will be permanently deleted.</div>
            <div className="schedule-alert-buttons">
              <button
                type="button"
                className="schedule-button"
                onClick={() => {
                  setShowDeleteConfirm(false);
                }}
              >
                Cancel
              </button>
              <button
                type="button"
                className="schedule-button schedule-destructive-filled"
                disabled={busy}
                onClick={() => {
                  void deleteEvent();
                }}
              >
                Delete
              </button>
            </div>
          </div>
        )}
      </form>
    </div>
  );
}
