// Port of apps/purepoint-macos/purepoint-macos/Models/TriggerItem.swift (WIN-8.9), plus the
// creation sheet's step drafts and preview (Views/Triggers/TriggerCreationSheet.swift) as pure
// functions.
import type { TriggerActionPayload, TriggerInfo } from '../protocol';
import type { SfSymbolName } from '../theme/icons';
import { projectNameOf } from './agentsHubModels';

// MARK: - Trigger Event

/** Swift `TriggerEvent`: the events a trigger can fire on (its raw value is the wire name). */
export const TRIGGER_EVENTS = ['agent_idle', 'pre_commit', 'pre_push'] as const;
export type TriggerEvent = (typeof TRIGGER_EVENTS)[number];

/** The Mac's `Color` per event, as a CSS class suffix (agents.css). */
export type TriggerTint = 'blue' | 'orange' | 'purple';

export const TriggerEvents = {
  label: (event: TriggerEvent): string =>
    event === 'agent_idle' ? 'Agent Idle' : event === 'pre_commit' ? 'Pre-Commit' : 'Pre-Push',
  icon: (event: TriggerEvent): SfSymbolName =>
    event === 'agent_idle'
      ? 'moon.zzz'
      : event === 'pre_commit'
        ? 'checkmark.circle'
        : 'arrow.up.circle',
  tint: (event: TriggerEvent): TriggerTint =>
    event === 'agent_idle' ? 'blue' : event === 'pre_commit' ? 'orange' : 'purple',
  /** `TriggerEvent(rawValue:) ?? .agentIdle`. */
  parse: (raw: string): TriggerEvent =>
    (TRIGGER_EVENTS as readonly string[]).includes(raw) ? (raw as TriggerEvent) : 'agent_idle',
} as const;

// MARK: - Trigger Item

/** Swift `TriggerItem`. */
export interface TriggerItem {
  /**
   * `scope:name`. The Mac uses the bare name, so a global and a project trigger of the same name
   * share one selection; here each is its own row.
   */
  readonly id: string;
  readonly name: string;
  readonly description: string | null;
  readonly event: TriggerEvent;
  readonly sequence: readonly TriggerActionPayload[];
  readonly variables: Readonly<Record<string, string>>;
  readonly scope: string;
  readonly projectRoot: string | null;
  readonly projectName: string | null;
}

/** Swift `TriggerItem(from:)`, placed in `root` when it is not global. */
export function triggerItemFromInfo(info: TriggerInfo, root: string | null = null): TriggerItem {
  const placed = root !== null && info.scope !== 'global';
  return {
    id: `${info.scope}:${info.name}`,
    name: info.name,
    description: info.description,
    event: TriggerEvents.parse(info.on),
    sequence: info.sequence,
    variables: info.variables,
    scope: info.scope,
    projectRoot: placed ? root : null,
    projectName: placed ? projectNameOf(root) : null,
  };
}

/** "1 step" / "3 steps", as the list row and the sheet preview count them. */
export function stepCountLabel(count: number): string {
  return `${String(count)} step${count === 1 ? '' : 's'}`;
}

// MARK: - Creation sheet

/** Swift `StepDraft`: one step as typed. */
export interface TriggerStepDraft {
  readonly inject: string;
  readonly gateRun: string;
  readonly gateExitCode: string;
  readonly maxRetries: string;
}

export const EMPTY_STEP: TriggerStepDraft = {
  inject: '',
  gateRun: '',
  gateExitCode: '',
  maxRetries: '',
};

/** Swift's `Int32(_:)` / `UInt32(_:)`: a whole number in range, else null. */
function parseInteger(text: string, min: number, max: number): number | null {
  if (!/^[+-]?\d+$/.test(text)) return null;
  const value = Number(text);
  return value >= min && value <= max ? value : null;
}

/** Swift `buildSequence()`: blank fields are left out; a gate needs a command. */
export function buildTriggerSequence(steps: readonly TriggerStepDraft[]): TriggerActionPayload[] {
  return steps.map((step) => ({
    inject: step.inject === '' ? null : step.inject,
    gate:
      step.gateRun === ''
        ? null
        : {
            run: step.gateRun,
            expect_exit: parseInteger(step.gateExitCode, -(2 ** 31), 2 ** 31 - 1),
          },
    max_retries: parseInteger(step.maxRetries, 0, 2 ** 32 - 1),
  }));
}

/** Swift `previewText`: "On Agent Idle → 2 steps (global)". */
export function triggerPreviewText(event: TriggerEvent, stepCount: number, scope: string): string {
  return `On ${TriggerEvents.label(event)} → ${stepCountLabel(stepCount)} (${scope})`;
}
