// Port of apps/purepoint-macos/purepoint-macos/State/ScheduleState.swift (WIN-8.8).
//
// The Schedule screen's state: view mode, the date it shows, the schedules loaded from the local
// daemon, and the event sheet. Shape (DEC-14): what Swift's `@Observable` publishes lives in a
// Zustand store (`schedule.store`); the class mirrors the Swift methods. Swift prints failures;
// here they also land in `error` so the sheet can say why a save did not take.

import { createStore, type StoreApi } from 'zustand/vanilla';
import { lastPathComponent } from '../model/projectKey';
import {
  addDays,
  addMonths,
  atTime,
  formatDayLong,
  formatIso8601,
  formatMonthYear,
  formatShortDate,
  isSameDay,
  localeFirstWeekday,
  monthDays,
  startOfDay,
  weekDays,
  type Weekday,
} from '../model/scheduleCalendar';
import {
  recurrenceMatches,
  scheduleEventFromInfo,
  type RecurrenceRule,
  type ScheduleEvent,
} from '../model/scheduleEvent';
import type { Response, ScheduleTriggerPayload } from '../protocol';
import { localDaemonClient, type DaemonRequester } from '../services/daemonConnection';

// MARK: - View Mode

export type ScheduleViewMode = 'month' | 'week' | 'day' | 'list';

export const SCHEDULE_VIEW_MODES: readonly ScheduleViewMode[] = ['month', 'week', 'day', 'list'];

export const SCHEDULE_VIEW_MODE_LABEL: Readonly<Record<ScheduleViewMode, string>> = {
  month: 'Month',
  week: 'Week',
  day: 'Day',
  list: 'List',
};

// MARK: - Occurrences

export interface Occurrence {
  readonly date: Date;
  readonly event: ScheduleEvent;
}

/**
 * Swift `walkDays`: one occurrence per calendar day from the later of `start` and the event's
 * day through `end`'s day, at the event's local time of day, kept when it falls in
 * [`start`, `end`]. Days advance by calendar day, so DST changes never skip or repeat one.
 */
function walkDays(
  start: Date,
  end: Date,
  event: ScheduleEvent,
  filter: (day: Date) => boolean = () => true,
): Occurrence[] {
  const results: Occurrence[] = [];
  const from = Math.max(startOfDay(start).getTime(), startOfDay(event.date).getTime());
  const endDay = startOfDay(end).getTime();
  const hour = event.date.getHours();
  const minute = event.date.getMinutes();
  let cursor = startOfDay(new Date(from));
  while (cursor.getTime() <= endDay) {
    if (filter(cursor)) {
      const occurrence = atTime(cursor, hour, minute);
      if (occurrence.getTime() >= start.getTime() && occurrence.getTime() <= end.getTime()) {
        results.push({ date: occurrence, event });
      }
    }
    cursor = startOfDay(new Date(cursor.getFullYear(), cursor.getMonth(), cursor.getDate() + 1));
  }
  return results;
}

/** Swift `expandedOccurrences(from:to:)`: enabled events' occurrences in [start, end], sorted. */
export function expandedOccurrences(
  events: readonly ScheduleEvent[],
  start: Date,
  end: Date,
): Occurrence[] {
  const results: Occurrence[] = [];
  for (const event of events) {
    if (!event.enabled) continue;
    switch (event.recurrence) {
      case 'none':
        if (event.date.getTime() >= start.getTime() && event.date.getTime() <= end.getTime()) {
          results.push({ date: event.date, event });
        }
        break;
      // The calendar shows an hourly schedule once a day, at its start time, like the Mac.
      case 'hourly':
      case 'daily':
        results.push(...walkDays(start, end, event));
        break;
      case 'weekdays':
        results.push(
          ...walkDays(start, end, event, (day) => day.getDay() >= 1 && day.getDay() <= 5),
        );
        break;
      case 'weekly': {
        const weekday = event.date.getDay();
        results.push(...walkDays(start, end, event, (day) => day.getDay() === weekday));
        break;
      }
      case 'monthly': {
        const dayOfMonth = event.date.getDate();
        results.push(...walkDays(start, end, event, (day) => day.getDate() === dayOfMonth));
        break;
      }
    }
  }
  // Stable: occurrences at the same instant keep the event order (name).
  return results.sort((a, b) => a.date.getTime() - b.date.getTime());
}

/**
 * The occurrences on one local day. Swift passes the next midnight as an inclusive end, so a
 * midnight event also showed on the previous day; the end here stops just before it.
 */
export function occurrencesOnDay(events: readonly ScheduleEvent[], day: Date): Occurrence[] {
  const dayStart = startOfDay(day);
  const nextStart = startOfDay(new Date(day.getFullYear(), day.getMonth(), day.getDate() + 1));
  return expandedOccurrences(events, dayStart, new Date(nextStart.getTime() - 1));
}

// MARK: - Sheet

export type ScheduleSheet =
  | { readonly mode: 'create'; readonly prefillDate: Date | null }
  | { readonly mode: 'edit'; readonly event: ScheduleEvent };

export interface SaveScheduleRequest {
  readonly projectRoot: string;
  readonly projectRoots: readonly string[];
  readonly name: string;
  readonly enabled: boolean;
  readonly recurrence: RecurrenceRule;
  readonly startAt: Date;
  readonly trigger: ScheduleTriggerPayload;
  readonly target: string;
  readonly scope: string;
  readonly root?: boolean;
  readonly agentName?: string | null;
}

// MARK: - Schedule State

export interface ScheduleStateFields {
  readonly viewMode: ScheduleViewMode;
  readonly currentDate: Date;
  readonly events: readonly ScheduleEvent[];
  /** Swift `showingCreationSheet` / `showingEditSheet` with `creationPrefillDate` / `selectedEvent`. */
  readonly sheet: ScheduleSheet | null;
  /** The last failed request's message; null once a request succeeds. */
  readonly error: string | null;
}

export interface ScheduleStateOptions {
  readonly client?: DaemonRequester;
  readonly now?: () => Date;
  readonly firstWeekday?: Weekday;
  readonly locale?: string;
}

function failure(response: Response): string | null {
  return response.type === 'error' ? response.message : null;
}

function messageOf(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String(error.message);
  }
  return String(error);
}

export class ScheduleState {
  readonly store: StoreApi<ScheduleStateFields>;
  readonly firstWeekday: Weekday;
  readonly locale: string | undefined;
  private readonly client: DaemonRequester;
  private readonly now: () => Date;
  private loadToken = 0;

  constructor(options: ScheduleStateOptions = {}) {
    this.client = options.client ?? localDaemonClient;
    this.now = options.now ?? (() => new Date());
    this.firstWeekday = options.firstWeekday ?? localeFirstWeekday(options.locale);
    this.locale = options.locale;
    this.store = createStore<ScheduleStateFields>(() => ({
      viewMode: 'week',
      currentDate: this.now(),
      events: [],
      sheet: null,
      error: null,
    }));
  }

  private get s(): ScheduleStateFields {
    return this.store.getState();
  }

  get viewMode(): ScheduleViewMode {
    return this.s.viewMode;
  }

  get currentDate(): Date {
    return this.s.currentDate;
  }

  get events(): readonly ScheduleEvent[] {
    return this.s.events;
  }

  setViewMode(mode: ScheduleViewMode): void {
    this.store.setState({ viewMode: mode });
  }

  // MARK: - Backend Integration

  /** Lists every project's schedules; a global schedule seen from several projects shows once. */
  async loadSchedules(projectRoots: readonly string[]): Promise<void> {
    const token = ++this.loadToken;
    try {
      const merged = new Map<string, ScheduleEvent>();
      for (const root of projectRoots) {
        const response = await this.client.send({ type: 'list_schedules', project_root: root });
        if (response.type === 'error') throw new Error(response.message);
        if (response.type !== 'schedule_list') continue;
        for (const payload of response.schedules) {
          const base = scheduleEventFromInfo(payload, this.now);
          const event: ScheduleEvent =
            payload.scope !== 'global'
              ? { ...base, projectRoot: root, projectName: lastPathComponent(root) }
              : { ...base, projectName: 'Global' };
          if (!merged.has(event.id)) merged.set(event.id, event);
        }
      }
      if (token !== this.loadToken) return;
      const events = [...merged.values()].sort((a, b) =>
        a.name < b.name ? -1 : a.name > b.name ? 1 : 0,
      );
      this.store.setState({ events });
    } catch (error) {
      console.error('Failed to load schedules:', error);
      if (token === this.loadToken) this.store.setState({ error: messageOf(error) });
    }
  }

  /** Swift `saveSchedule`: true when the daemon saved it. */
  async saveSchedule(request: SaveScheduleRequest): Promise<boolean> {
    return this.perform(
      {
        type: 'save_schedule',
        project_root: request.projectRoot,
        name: request.name,
        enabled: request.enabled,
        recurrence: request.recurrence,
        start_at: formatIso8601(request.startAt),
        trigger: request.trigger,
        target: request.target,
        scope: request.scope,
        ...(request.root === undefined ? {} : { root: request.root }),
        ...(request.agentName === undefined ? {} : { agent_name: request.agentName }),
      },
      request.projectRoots,
      'save schedule',
    );
  }

  async deleteSchedule(
    projectRoot: string,
    projectRoots: readonly string[],
    name: string,
    scope: string,
  ): Promise<boolean> {
    return this.perform(
      { type: 'delete_schedule', project_root: projectRoot, name, scope },
      projectRoots,
      'delete schedule',
    );
  }

  /** Swift `toggleSchedule`: disables an enabled schedule, enables a disabled one. */
  async toggleSchedule(
    projectRoot: string,
    projectRoots: readonly string[],
    name: string,
    currentlyEnabled: boolean,
  ): Promise<boolean> {
    return this.perform(
      {
        type: currentlyEnabled ? 'disable_schedule' : 'enable_schedule',
        project_root: projectRoot,
        name,
      },
      projectRoots,
      'toggle schedule',
    );
  }

  private async perform(
    request: Parameters<DaemonRequester['send']>[0],
    projectRoots: readonly string[],
    what: string,
  ): Promise<boolean> {
    let ok = false;
    try {
      const message = failure(await this.client.send(request));
      if (message !== null) throw new Error(message);
      ok = true;
      this.store.setState({ error: null });
    } catch (error) {
      console.error(`Failed to ${what}:`, error);
      this.store.setState({ error: messageOf(error) });
    }
    await this.loadSchedules(projectRoots);
    return ok;
  }

  clearError(): void {
    this.store.setState({ error: null });
  }

  // MARK: - Date Navigation

  get currentMonthYear(): string {
    return formatMonthYear(this.currentDate, this.locale);
  }

  get currentWeekRange(): string {
    const days = this.weekDays;
    const start = days[0] ?? this.currentDate;
    const end = days[6] ?? this.currentDate;
    return `${formatShortDate(start, this.locale)} – ${formatShortDate(end, this.locale)}`;
  }

  get currentDayFormatted(): string {
    return formatDayLong(this.currentDate, this.locale);
  }

  get headerDateLabel(): string {
    switch (this.viewMode) {
      case 'month':
      case 'list':
        return this.currentMonthYear;
      case 'week':
        return this.currentWeekRange;
      case 'day':
        return this.currentDayFormatted;
    }
  }

  goForward(): void {
    this.navigate(1);
  }

  goBackward(): void {
    this.navigate(-1);
  }

  private navigate(value: number): void {
    const date = this.currentDate;
    let next: Date;
    switch (this.viewMode) {
      case 'month':
      case 'list':
        next = addMonths(date, value);
        break;
      case 'week':
        next = addDays(date, 7 * value);
        break;
      case 'day':
        next = addDays(date, value);
        break;
    }
    this.store.setState({ currentDate: next });
  }

  goToToday(): void {
    this.store.setState({ currentDate: this.now() });
  }

  // MARK: - Event Queries

  /** Swift `events(for:)`: enabled events whose rule matches the instant. */
  eventsFor(date: Date): ScheduleEvent[] {
    return this.events.filter(
      (event) => event.enabled && recurrenceMatches(event.recurrence, date, event.date),
    );
  }

  expandedOccurrences(start: Date, end: Date): Occurrence[] {
    return expandedOccurrences(this.events, start, end);
  }

  // MARK: - Interaction

  handleTimeSlotClick(date: Date): void {
    this.store.setState({ sheet: { mode: 'create', prefillDate: date }, error: null });
  }

  handleEventClick(event: ScheduleEvent): void {
    this.store.setState({ sheet: { mode: 'edit', event }, error: null });
  }

  /** The header's New Schedule button: a creation sheet with no prefilled date. */
  showCreationSheet(): void {
    this.store.setState({ sheet: { mode: 'create', prefillDate: null }, error: null });
  }

  dismissSheet(): void {
    this.store.setState({ sheet: null });
  }

  addEvent(event: ScheduleEvent): void {
    this.store.setState({ events: [...this.events, event] });
  }

  // MARK: - Grid Helpers

  get monthDays(): Date[] {
    return monthDays(this.currentDate, this.firstWeekday);
  }

  isCurrentMonth(date: Date): boolean {
    return (
      date.getMonth() === this.currentDate.getMonth() &&
      date.getFullYear() === this.currentDate.getFullYear()
    );
  }

  isToday(date: Date): boolean {
    return isSameDay(date, this.now());
  }

  isTomorrow(date: Date): boolean {
    const today = this.now();
    return isSameDay(date, new Date(today.getFullYear(), today.getMonth(), today.getDate() + 1));
  }

  get weekDays(): Date[] {
    return weekDays(this.currentDate, this.firstWeekday);
  }

  /** The current instant, through the injected clock. */
  nowDate(): Date {
    return this.now();
  }

  /** ScheduleListView: the 30 days from the current date's start. */
  listRange(): { start: Date; end: Date } {
    const start = startOfDay(this.currentDate);
    return { start, end: addDays(start, 30) };
  }
}

// One ScheduleState per window, kept across visits to the screen (Swift `appState.scheduleState`).
const statesByOwner = new WeakMap<object, ScheduleState>();

export function scheduleStateFor(owner: object): ScheduleState {
  let state = statesByOwner.get(owner);
  if (state === undefined) {
    state = new ScheduleState();
    statesByOwner.set(owner, state);
  }
  return state;
}
