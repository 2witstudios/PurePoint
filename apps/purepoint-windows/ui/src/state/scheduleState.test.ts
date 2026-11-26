import { describe, expect, it } from 'vitest';
import { dayKey } from '../model/scheduleCalendar';
import {
  EventColor,
  recurrenceFromBackend,
  recurrenceMatches,
  scheduleEventFromInfo,
  stableHash,
  type ColorStorage,
  type RecurrenceRule,
  type ScheduleEvent,
} from '../model/scheduleEvent';
import type { Request, Response, ScheduleInfo } from '../protocol';
import { inTimeZone } from '../test/timeZone';
import { expandedOccurrences, occurrencesOnDay, ScheduleState } from './scheduleState';

function event(
  name: string,
  date: Date,
  recurrence: RecurrenceRule,
  overrides: Partial<ScheduleEvent> = {},
): ScheduleEvent {
  return {
    id: `local:${name}`,
    name,
    type: 'inlinePrompt',
    date,
    recurrence,
    projectName: 'repo',
    projectRoot: 'C:\\repo',
    target: '',
    colorIndex: 0,
    enabled: true,
    scope: 'local',
    trigger: { type: 'inline_prompt', prompt: 'hi', agent: 'terminal' },
    root: true,
    agentName: null,
    ...overrides,
  };
}

function wall(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${dayKey(date)} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function memoryStorage(): ColorStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => {
      data.set(key, value);
    },
    removeItem: (key) => {
      data.delete(key);
    },
  };
}

function info(overrides: Partial<ScheduleInfo> = {}): ScheduleInfo {
  return {
    name: 'nightly',
    enabled: true,
    recurrence: 'daily',
    start_at: '2026-11-20T14:00:00Z',
    next_run: null,
    trigger: { type: 'inline_prompt', prompt: 'hi', agent: 'terminal' },
    project_root: 'C:\\repo',
    target: '',
    scope: 'local',
    root: true,
    agent_name: null,
    created_at: '2026-11-20T00:00:00Z',
    ...overrides,
  };
}

/** A fake daemon that records requests and answers list_schedules per root. */
function fakeClient(lists: Record<string, ScheduleInfo[]>, reply: Response = { type: 'ok' }) {
  const requests: Request[] = [];
  return {
    requests,
    send(request: Request): Promise<Response> {
      requests.push(request);
      if (request.type === 'list_schedules') {
        return Promise.resolve({
          type: 'schedule_list',
          schedules: lists[request.project_root] ?? [],
        });
      }
      return Promise.resolve(reply);
    },
  };
}

describe('expandedOccurrences', () => {
  const start = new Date(2026, 10, 22); // Sunday
  const end = new Date(new Date(2026, 10, 29).getTime() - 1);

  it('should show a one-off schedule only at its instant', () => {
    const once = event('once', new Date(2026, 10, 24, 10, 30), 'none');
    expect(expandedOccurrences([once], start, end).map((o) => wall(o.date))).toEqual([
      '2026-11-24 10:30',
    ]);
    expect(expandedOccurrences([once], new Date(2026, 10, 25), end)).toEqual([]);
  });

  it('should repeat daily and hourly schedules once a day from their start', () => {
    const daily = event('daily', new Date(2026, 10, 25, 8, 0), 'daily');
    const hourly = event('hourly', new Date(2026, 10, 25, 9, 15), 'hourly');
    expect(expandedOccurrences([daily], start, end).map((o) => wall(o.date))).toEqual([
      '2026-11-25 08:00',
      '2026-11-26 08:00',
      '2026-11-27 08:00',
      '2026-11-28 08:00',
    ]);
    expect(expandedOccurrences([hourly], start, end)).toHaveLength(4);
  });

  it('should skip weekends for weekday schedules', () => {
    const weekdays = event('weekdays', new Date(2026, 10, 20, 6, 0), 'weekdays');
    expect(expandedOccurrences([weekdays], start, end).map((o) => dayKey(o.date))).toEqual([
      '2026-11-23',
      '2026-11-24',
      '2026-11-25',
      '2026-11-26',
      '2026-11-27',
    ]);
  });

  it('should repeat weekly on the start weekday and monthly on the start day', () => {
    const weekly = event('weekly', new Date(2026, 9, 6, 2, 0), 'weekly'); // a Tuesday
    const monthly = event('monthly', new Date(2026, 0, 31, 7, 0), 'monthly');
    expect(expandedOccurrences([weekly], start, end).map((o) => wall(o.date))).toEqual([
      '2026-11-24 02:00',
    ]);
    const year = expandedOccurrences(
      [monthly],
      new Date(2026, 0, 1),
      new Date(2026, 11, 31, 23, 59),
    ).map((o) => dayKey(o.date));
    // Months without a 31st are skipped, as on the Mac.
    expect(year).toEqual([
      '2026-01-31',
      '2026-03-31',
      '2026-05-31',
      '2026-07-31',
      '2026-08-31',
      '2026-10-31',
      '2026-12-31',
    ]);
  });

  it('should leave disabled schedules out and sort by time', () => {
    const late = event('late', new Date(2026, 10, 25, 20, 0), 'none');
    const early = event('early', new Date(2026, 10, 25, 7, 0), 'none');
    const off = event('off', new Date(2026, 10, 25, 9, 0), 'none', { enabled: false });
    expect(expandedOccurrences([late, off, early], start, end).map((o) => o.event.name)).toEqual([
      'early',
      'late',
    ]);
  });

  it('should put a midnight schedule on its own day only', () => {
    const midnight = event('midnight', new Date(2026, 10, 25, 0, 0), 'none');
    expect(occurrencesOnDay([midnight], new Date(2026, 10, 24))).toEqual([]);
    expect(occurrencesOnDay([midnight], new Date(2026, 10, 25))).toHaveLength(1);
  });
});

inTimeZone('America/New_York', () => {
  it('should keep a daily schedule at its wall time across spring forward and fall back', () => {
    const daily = event('daily', new Date(2026, 2, 6, 9, 0), 'daily');
    const march = expandedOccurrences([daily], new Date(2026, 2, 7), new Date(2026, 2, 10)).map(
      (o) => wall(o.date),
    );
    expect(march).toEqual(['2026-03-07 09:00', '2026-03-08 09:00', '2026-03-09 09:00']);
    const november = expandedOccurrences([daily], new Date(2026, 9, 31), new Date(2026, 10, 3)).map(
      (o) => wall(o.date),
    );
    expect(november).toEqual(['2026-10-31 09:00', '2026-11-01 09:00', '2026-11-02 09:00']);
  });

  it('should move an occurrence inside the spring-forward gap to the end of the gap', () => {
    const early = event('early', new Date(2026, 2, 1, 2, 30), 'daily');
    const onGapDay = occurrencesOnDay([early], new Date(2026, 2, 8));
    expect(onGapDay.map((o) => wall(o.date))).toEqual(['2026-03-08 03:00']);
    expect(occurrencesOnDay([early], new Date(2026, 2, 9)).map((o) => wall(o.date))).toEqual([
      '2026-03-09 02:30',
    ]);
  });

  it('should show a 01:30 schedule once on the fall-back day', () => {
    const repeated = event('repeated', new Date(2026, 9, 1, 1, 30), 'daily');
    const day = occurrencesOnDay([repeated], new Date(2026, 10, 1));
    expect(day).toHaveLength(1);
    expect(day[0]?.date.toISOString()).toBe('2026-11-01T05:30:00.000Z');
  });

  it('should place a UTC start_at on the local day and hour', () => {
    // 03:30 UTC on Nov 26 is 22:30 on Nov 25 in New York.
    const parsed = scheduleEventFromInfo(info({ start_at: '2026-11-26T03:30:00Z' }));
    expect(wall(parsed.date)).toBe('2026-11-25 22:30');
    expect(occurrencesOnDay([parsed], new Date(2026, 10, 25))).toHaveLength(1);
  });

  it('should step weeks and days by calendar day over DST', () => {
    const state = new ScheduleState({
      client: fakeClient({}),
      now: () => new Date(2026, 2, 4, 12, 0),
      firstWeekday: 0,
      locale: 'en-US',
    });
    state.goForward(); // week
    expect(wall(state.currentDate)).toBe('2026-03-11 12:00');
    expect(state.weekDays.map(dayKey)[0]).toBe('2026-03-08');
    state.setViewMode('day');
    state.goBackward();
    state.goBackward();
    state.goBackward();
    expect(wall(state.currentDate)).toBe('2026-03-08 12:00');
  });
});

inTimeZone('Australia/Sydney', () => {
  it('should keep weekly occurrences on their weekday when DST ends', () => {
    // Sydney falls back on Sunday 2026-04-05 03:00 → 02:00.
    const weekly = event('weekly', new Date(2026, 2, 22, 2, 30), 'weekly');
    const occurrences = expandedOccurrences(
      [weekly],
      new Date(2026, 2, 28),
      new Date(2026, 3, 13),
    ).map((o) => wall(o.date));
    expect(occurrences).toEqual(['2026-03-29 02:30', '2026-04-05 02:30', '2026-04-12 02:30']);
  });
});

describe('recurrenceMatches', () => {
  const original = new Date(2026, 10, 25, 8, 30); // Wednesday

  it('should match like RecurrenceRule.matches', () => {
    expect(recurrenceMatches('none', new Date(2026, 10, 25, 20, 0), original)).toBe(true);
    expect(recurrenceMatches('none', new Date(2026, 10, 26, 8, 30), original)).toBe(false);
    expect(recurrenceMatches('hourly', new Date(2026, 10, 27, 1, 0), original)).toBe(true);
    expect(recurrenceMatches('daily', new Date(2026, 10, 30, 8, 30), original)).toBe(true);
    expect(recurrenceMatches('daily', new Date(2026, 10, 30, 8, 31), original)).toBe(false);
    expect(recurrenceMatches('weekdays', new Date(2026, 10, 28, 8, 30), original)).toBe(false);
    expect(recurrenceMatches('weekdays', new Date(2026, 10, 30, 8, 30), original)).toBe(true);
    expect(recurrenceMatches('weekly', new Date(2026, 11, 2, 8, 30), original)).toBe(true);
    expect(recurrenceMatches('weekly', new Date(2026, 11, 3, 8, 30), original)).toBe(false);
    expect(recurrenceMatches('monthly', new Date(2026, 11, 25, 8, 30), original)).toBe(true);
    expect(recurrenceMatches('daily', new Date(2026, 10, 24, 8, 30), original)).toBe(false);
  });

  it('should read backend strings, defaulting to none', () => {
    expect(recurrenceFromBackend('weekdays')).toBe('weekdays');
    expect(recurrenceFromBackend('yearly')).toBe('none');
  });
});

describe('EventColor', () => {
  it('should hash names with djb2 as the Mac does', () => {
    expect(stableHash('')).toBe(5381);
    expect(stableHash('a')).toBe(5381 * 33 + 97);
    // Swift UInt wraps at 2^64; only the index (mod 8) is used, which 32-bit wrapping keeps.
    expect(stableHash('a-long-project-name-that-overflows') % 8).toBe(
      Number(
        [...new TextEncoder().encode('a-long-project-name-that-overflows')].reduce(
          (hash, byte) => (hash * 33n + BigInt(byte)) % 2n ** 64n,
          5381n,
        ) % 8n,
      ),
    );
  });

  it('should prefer a saved event color, then a saved project color, then the hash', () => {
    const storage = memoryStorage();
    const hashed = stableHash('repo') % 8;
    expect(EventColor.forEvent('job', 'repo', storage)).toBe(hashed);
    EventColor.save(5, 'repo', storage);
    expect(EventColor.forEvent('job', 'repo', storage)).toBe(5);
    EventColor.save(2, 'job', storage);
    expect(EventColor.forEvent('job', 'repo', storage)).toBe(2);
    expect(storage.data.get('scheduleColor:job')).toBe('2');
    EventColor.remove('job', storage);
    expect(EventColor.savedIndex('job', storage)).toBeNull();
  });
});

describe('ScheduleState', () => {
  const now = () => new Date(2026, 10, 25, 10, 0);

  it('should load every project, scoping local schedules and listing globals once', async () => {
    const client = fakeClient({
      'C:\\a': [info({ name: 'zeta' }), info({ name: 'shared', scope: 'global' })],
      'C:\\b': [info({ name: 'alpha' }), info({ name: 'shared', scope: 'global' })],
    });
    const state = new ScheduleState({ client, now, firstWeekday: 0 });
    await state.loadSchedules(['C:\\a', 'C:\\b']);
    expect(state.events.map((e) => [e.name, e.projectName, e.projectRoot])).toEqual([
      ['alpha', 'b', 'C:\\b'],
      ['shared', 'Global', null],
      ['zeta', 'a', 'C:\\a'],
    ]);
  });

  it('should send save, delete, enable and disable requests and reload', async () => {
    const client = fakeClient({ 'C:\\a': [info()] });
    const state = new ScheduleState({ client, now, firstWeekday: 0 });
    const ok = await state.saveSchedule({
      projectRoot: 'C:\\a',
      projectRoots: ['C:\\a'],
      name: 'nightly',
      enabled: true,
      recurrence: 'weekly',
      startAt: new Date(Date.UTC(2026, 10, 26, 3, 0, 0, 500)),
      trigger: { type: 'inline_prompt', prompt: 'echo', agent: 'terminal' },
      target: '',
      scope: 'local',
    });
    expect(ok).toBe(true);
    expect(client.requests[0]).toEqual({
      type: 'save_schedule',
      project_root: 'C:\\a',
      name: 'nightly',
      enabled: true,
      recurrence: 'weekly',
      start_at: '2026-11-26T03:00:00Z',
      trigger: { type: 'inline_prompt', prompt: 'echo', agent: 'terminal' },
      target: '',
      scope: 'local',
    });
    expect(client.requests[1]?.type).toBe('list_schedules');
    await state.toggleSchedule('C:\\a', ['C:\\a'], 'nightly', true);
    await state.toggleSchedule('C:\\a', ['C:\\a'], 'nightly', false);
    await state.deleteSchedule('C:\\a', ['C:\\a'], 'nightly', 'local');
    expect(client.requests.filter((r) => r.type !== 'list_schedules')).toEqual([
      expect.objectContaining({ type: 'save_schedule' }),
      { type: 'disable_schedule', project_root: 'C:\\a', name: 'nightly' },
      { type: 'enable_schedule', project_root: 'C:\\a', name: 'nightly' },
      { type: 'delete_schedule', project_root: 'C:\\a', name: 'nightly', scope: 'local' },
    ]);
    expect(state.events).toHaveLength(1);
  });

  it('should report a daemon error and keep it until the next success', async () => {
    const client = fakeClient({}, { type: 'error', code: 'invalid', message: 'bad name' });
    const state = new ScheduleState({ client, now, firstWeekday: 0 });
    const ok = await state.deleteSchedule('C:\\a', ['C:\\a'], 'x', 'local');
    expect(ok).toBe(false);
    expect(state.store.getState().error).toBe('bad name');
  });

  it('should navigate months, weeks and days and label the header', () => {
    const state = new ScheduleState({
      client: fakeClient({}),
      now: () => new Date(2026, 0, 31, 9, 0),
      firstWeekday: 0,
      locale: 'en-US',
    });
    expect(state.viewMode).toBe('week');
    expect(state.headerDateLabel).toBe('Jan 25 – Jan 31');
    state.setViewMode('month');
    expect(state.headerDateLabel).toBe('January 2026');
    state.goForward();
    expect(dayKey(state.currentDate)).toBe('2026-02-28');
    state.setViewMode('list');
    expect(state.headerDateLabel).toBe('February 2026');
    state.setViewMode('day');
    expect(state.headerDateLabel).toBe('Saturday, February 28');
    state.goForward();
    expect(dayKey(state.currentDate)).toBe('2026-03-01');
    state.goToToday();
    expect(dayKey(state.currentDate)).toBe('2026-01-31');
    expect(state.isToday(new Date(2026, 0, 31, 23))).toBe(true);
    expect(state.isTomorrow(new Date(2026, 1, 1))).toBe(true);
  });

  it('should open the sheet for a slot, an event and New Schedule', () => {
    const state = new ScheduleState({ client: fakeClient({}), now, firstWeekday: 0 });
    const slot = new Date(2026, 10, 25, 14, 15);
    state.handleTimeSlotClick(slot);
    expect(state.store.getState().sheet).toEqual({ mode: 'create', prefillDate: slot });
    const e = event('x', slot, 'none');
    state.handleEventClick(e);
    expect(state.store.getState().sheet).toEqual({ mode: 'edit', event: e });
    state.showCreationSheet();
    expect(state.store.getState().sheet).toEqual({ mode: 'create', prefillDate: null });
    state.dismissSheet();
    expect(state.store.getState().sheet).toBeNull();
  });
});
