// Port of apps/purepoint-macos/purepoint-macos/Models/ScheduleEvent.swift (WIN-8.8).
//
// A schedule as the Schedule screen draws it: its trigger type, recurrence rule, color and the
// daemon's `ScheduleInfo` fields. Recurrence matching uses the local calendar
// (model/scheduleCalendar.ts).

import type { ScheduleInfo, ScheduleTriggerPayload } from '../protocol';
import type { SfSymbolName } from '../theme/icons';
import { isSameDay, parseIso8601 } from './scheduleCalendar';

// MARK: - Schedule Type

export type ScheduleType = 'swarm' | 'agent' | 'inlinePrompt';

export const SCHEDULE_TYPES: readonly ScheduleType[] = ['swarm', 'agent', 'inlinePrompt'];

export const SCHEDULE_TYPE_ICON: Readonly<Record<ScheduleType, SfSymbolName>> = {
  swarm: 'person.3.fill',
  agent: 'cpu',
  inlinePrompt: 'text.bubble',
};

export const SCHEDULE_TYPE_LABEL: Readonly<Record<ScheduleType, string>> = {
  swarm: 'Swarm',
  agent: 'Agent',
  inlinePrompt: 'Inline Prompt',
};

// MARK: - Recurrence Rule

/** Swift `RecurrenceRule`; the value is its backend string (the Rust `recurrence` field). */
export type RecurrenceRule = 'none' | 'hourly' | 'daily' | 'weekdays' | 'weekly' | 'monthly';

export const RECURRENCE_RULES: readonly RecurrenceRule[] = [
  'none',
  'hourly',
  'daily',
  'weekdays',
  'weekly',
  'monthly',
];

/** Swift `rawValue`: the picker title. */
export const RECURRENCE_LABEL: Readonly<Record<RecurrenceRule, string>> = {
  none: 'Does not repeat',
  hourly: 'Every hour',
  daily: 'Every day',
  weekdays: 'Every weekday',
  weekly: 'Every week',
  monthly: 'Every month',
};

const RECURRENCE_BADGE: Readonly<Record<RecurrenceRule, string | null>> = {
  none: null,
  hourly: 'Hourly',
  daily: 'Daily',
  weekdays: 'Weekdays',
  weekly: 'Weekly',
  monthly: 'Monthly',
};

export function recurrenceBadge(rule: RecurrenceRule): string | null {
  return RECURRENCE_BADGE[rule];
}

/** Swift `init(backendString:)`: anything unknown is `none`. */
export function recurrenceFromBackend(value: string): RecurrenceRule {
  return (RECURRENCE_RULES as readonly string[]).includes(value)
    ? (value as RecurrenceRule)
    : 'none';
}

function timesMatch(a: Date, b: Date): boolean {
  return a.getHours() === b.getHours() && a.getMinutes() === b.getMinutes();
}

function isWeekday(date: Date): boolean {
  const day = date.getDay();
  return day >= 1 && day <= 5;
}

/** Swift `RecurrenceRule.matches(date:originalDate:calendar:)`. */
export function recurrenceMatches(rule: RecurrenceRule, date: Date, originalDate: Date): boolean {
  if (date.getTime() < originalDate.getTime()) return false;
  switch (rule) {
    case 'none':
      return isSameDay(date, originalDate);
    case 'hourly':
      return true;
    case 'daily':
      return timesMatch(date, originalDate);
    case 'weekdays':
      return isWeekday(date) && timesMatch(date, originalDate);
    case 'weekly':
      return date.getDay() === originalDate.getDay() && timesMatch(date, originalDate);
    case 'monthly':
      return date.getDate() === originalDate.getDate() && timesMatch(date, originalDate);
  }
}

// MARK: - Event Color

/** Swift `EventColor.palette`: .blue, .purple, .teal, .indigo, .orange, .pink, .green, .yellow. */
export const EVENT_COLOR_NAMES = [
  'blue',
  'purple',
  'teal',
  'indigo',
  'orange',
  'pink',
  'green',
  'yellow',
] as const;

export type EventColorName = (typeof EVENT_COLOR_NAMES)[number];

/** The CSS color for a palette index (light and dark values live in schedule.css). */
export function eventColorCss(index: number): string {
  const name = EVENT_COLOR_NAMES[index % EVENT_COLOR_NAMES.length] ?? 'blue';
  return `var(--schedule-color-${name})`;
}

/**
 * Stable djb2 hash, as Swift computes it on `UInt` (64-bit, wrapping). Only `hash % 8` is ever
 * used and 8 divides both 2^32 and 2^64, so 32-bit wrapping gives the same index.
 */
export function stableHash(text: string): number {
  let hash = 5381;
  for (const byte of new TextEncoder().encode(text)) {
    hash = (Math.imul(hash, 33) + byte) >>> 0;
  }
  return hash;
}

/** Where colors are kept (Swift `UserDefaults.standard`); `localStorage` in the app. */
export interface ColorStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

function defaultStorage(): ColorStorage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

function colorKey(eventName: string): string {
  return `scheduleColor:${eventName}`;
}

export const EventColor = {
  paletteSize: EVENT_COLOR_NAMES.length,

  savedIndex(eventName: string, storage = defaultStorage()): number | null {
    try {
      const raw = storage?.getItem(colorKey(eventName)) ?? null;
      if (raw === null) return null;
      const index = Number.parseInt(raw, 10);
      return Number.isNaN(index) ? 0 : index;
    } catch {
      return null;
    }
  },

  save(index: number, eventName: string, storage = defaultStorage()): void {
    try {
      storage?.setItem(colorKey(eventName), String(index));
    } catch {
      // Storage may be unavailable; the color falls back to the hash.
    }
  },

  remove(eventName: string, storage = defaultStorage()): void {
    try {
      storage?.removeItem(colorKey(eventName));
    } catch {
      // As above.
    }
  },

  /** Swift `forProject`: a saved color under the project's name first, else the name's hash. */
  forProject(projectName: string, storage = defaultStorage()): number {
    const saved = EventColor.savedIndex(projectName, storage);
    if (saved !== null) return saved % EVENT_COLOR_NAMES.length;
    return stableHash(projectName) % EVENT_COLOR_NAMES.length;
  },

  /** Swift `forEvent`: the event's own saved color, then the project's. */
  forEvent(eventName: string, projectName: string, storage = defaultStorage()): number {
    const saved = EventColor.savedIndex(eventName, storage);
    if (saved !== null) return saved % EVENT_COLOR_NAMES.length;
    return EventColor.forProject(projectName, storage);
  },
};

// MARK: - Schedule Event

export interface ScheduleEvent {
  /** `<scope>:<name>`: the daemon's key for a schedule, stable across reloads. */
  readonly id: string;
  readonly name: string;
  readonly type: ScheduleType;
  readonly date: Date;
  readonly recurrence: RecurrenceRule;
  readonly projectName: string;
  /** The project the schedule was listed from; null for global schedules. */
  readonly projectRoot: string | null;
  readonly target: string;
  /** Palette index (Swift stores the resolved `Color`). */
  readonly colorIndex: number;
  readonly enabled: boolean;
  readonly scope: string;
  readonly trigger: ScheduleTriggerPayload | null;
  /** Kept so an edit saves them back (the Mac sheet drops them). */
  readonly root: boolean;
  readonly agentName: string | null;
}

export function scheduleTypeOf(trigger: ScheduleTriggerPayload): ScheduleType {
  switch (trigger.type) {
    case 'agent_def':
      return 'agent';
    case 'swarm_def':
      return 'swarm';
    case 'inline_prompt':
      return 'inlinePrompt';
  }
}

/** Swift `ScheduleEvent(from: ScheduleInfoPayload)`. */
export function scheduleEventFromInfo(
  payload: ScheduleInfo,
  now: () => Date = () => new Date(),
  storage = defaultStorage(),
): ScheduleEvent {
  return {
    id: `${payload.scope}:${payload.name}`,
    name: payload.name,
    enabled: payload.enabled,
    recurrence: recurrenceFromBackend(payload.recurrence),
    projectName: payload.project_root,
    projectRoot: null,
    target: payload.target,
    scope: payload.scope,
    colorIndex: EventColor.forEvent(payload.name, payload.project_root, storage),
    trigger: payload.trigger,
    date: parseIso8601(payload.start_at) ?? now(),
    type: scheduleTypeOf(payload.trigger),
    root: payload.root,
    agentName: payload.agent_name,
  };
}
