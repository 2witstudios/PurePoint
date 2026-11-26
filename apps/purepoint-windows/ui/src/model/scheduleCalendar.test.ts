import { describe, expect, it } from 'vitest';
import { inTimeZone } from '../test/timeZone';
import {
  addDays,
  addMonths,
  atTime,
  dayKey,
  formatIso8601,
  fromDateTimeLocalValue,
  hourLabel,
  monthDays,
  parseIso8601,
  shortWeekdaySymbols,
  startOfDay,
  startOfNextDay,
  startOfWeek,
  toDateTimeLocalValue,
  weekDays,
} from './scheduleCalendar';

function wall(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${dayKey(date)} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

inTimeZone('America/New_York', () => {
  it('should give the first instant after a spring-forward gap for a missing wall time', () => {
    // 2026-03-08 02:00 EST jumps to 03:00 EDT.
    const result = atTime(new Date(2026, 2, 8), 2, 30);
    expect(wall(result)).toBe('2026-03-08 03:00');
    expect(result.toISOString()).toBe('2026-03-08T07:00:00.000Z');
  });

  it('should keep existing wall times on DST days', () => {
    expect(wall(atTime(new Date(2026, 2, 8), 9, 15))).toBe('2026-03-08 09:15');
    // Fall back: 01:30 happens twice; the first one (EDT) is chosen.
    expect(atTime(new Date(2026, 10, 1), 1, 30).toISOString()).toBe('2026-11-01T05:30:00.000Z');
  });

  it('should add calendar days across DST and keep the time of day', () => {
    const before = new Date(2026, 2, 7, 9, 0);
    const after = addDays(before, 1);
    expect(wall(after)).toBe('2026-03-08 09:00');
    expect(after.getTime() - before.getTime()).toBe(23 * 3_600_000);
    const fall = addDays(new Date(2026, 9, 31, 9, 0), 1);
    expect(wall(fall)).toBe('2026-11-01 09:00');
  });

  it('should start days at midnight on DST change days', () => {
    expect(wall(startOfDay(new Date(2026, 2, 8, 15)))).toBe('2026-03-08 00:00');
    const next = startOfNextDay(new Date(2026, 10, 1, 12));
    expect(wall(next)).toBe('2026-11-02 00:00');
    expect(next.getTime() - startOfDay(new Date(2026, 10, 1)).getTime()).toBe(25 * 3_600_000);
  });

  it('should round-trip datetime-local values in local time', () => {
    const date = fromDateTimeLocalValue('2026-11-25T14:45');
    expect(date?.toISOString()).toBe('2026-11-25T19:45:00.000Z');
    expect(toDateTimeLocalValue(date ?? new Date())).toBe('2026-11-25T14:45');
    expect(wall(fromDateTimeLocalValue('2026-03-08T02:15') ?? new Date())).toBe('2026-03-08 03:00');
    expect(fromDateTimeLocalValue('nope')).toBeNull();
  });
});

inTimeZone('America/Santiago', () => {
  it('should start the day after a midnight spring-forward at 01:00', () => {
    // Chile moves 2026-09-06 00:00 to 01:00.
    expect(wall(startOfDay(new Date(2026, 8, 6, 12)))).toBe('2026-09-06 01:00');
    expect(wall(startOfNextDay(new Date(2026, 8, 5, 12)))).toBe('2026-09-06 01:00');
  });
});

describe('scheduleCalendar', () => {
  it('should clamp month arithmetic to the target month', () => {
    expect(dayKey(addMonths(new Date(2026, 0, 31, 9), 1))).toBe('2026-02-28');
    expect(dayKey(addMonths(new Date(2024, 0, 31, 9), 1))).toBe('2024-02-29');
    expect(dayKey(addMonths(new Date(2026, 2, 31, 9), -1))).toBe('2026-02-28');
    expect(dayKey(addMonths(new Date(2026, 11, 15), 1))).toBe('2027-01-15');
    expect(addMonths(new Date(2026, 0, 31, 9, 30), 1).getHours()).toBe(9);
  });

  it('should find week starts for Sunday- and Monday-first locales', () => {
    const wednesday = new Date(2026, 10, 25, 15);
    expect(dayKey(startOfWeek(wednesday, 0))).toBe('2026-11-22');
    expect(dayKey(startOfWeek(wednesday, 1))).toBe('2026-11-23');
    expect(dayKey(startOfWeek(new Date(2026, 10, 22), 1))).toBe('2026-11-16');
    expect(weekDays(wednesday, 0).map(dayKey)).toEqual([
      '2026-11-22',
      '2026-11-23',
      '2026-11-24',
      '2026-11-25',
      '2026-11-26',
      '2026-11-27',
      '2026-11-28',
    ]);
  });

  it('should lay a month out as 42 cells starting on the week of the 1st', () => {
    const days = monthDays(new Date(2026, 10, 25), 0);
    expect(days).toHaveLength(42);
    expect(dayKey(days[0] ?? new Date())).toBe('2026-11-01'); // Nov 1 2026 is a Sunday
    expect(dayKey(days[41] ?? new Date())).toBe('2026-12-12');
    const mondayFirst = monthDays(new Date(2026, 10, 25), 1);
    expect(dayKey(mondayFirst[0] ?? new Date())).toBe('2026-10-26');
  });

  it('should label weekdays from the first weekday and hours like the Mac', () => {
    expect(shortWeekdaySymbols(0, 'en-US')).toEqual([
      'Sun',
      'Mon',
      'Tue',
      'Wed',
      'Thu',
      'Fri',
      'Sat',
    ]);
    expect(shortWeekdaySymbols(1, 'en-US')[0]).toBe('Mon');
    expect([0, 1, 11, 12, 13, 23].map(hourLabel)).toEqual([
      '12 AM',
      '1 AM',
      '11 AM',
      '12 PM',
      '1 PM',
      '11 PM',
    ]);
  });

  it('should write and read ISO 8601 like ISO8601DateFormatter', () => {
    const date = new Date(Date.UTC(2026, 10, 25, 8, 30, 15, 250));
    expect(formatIso8601(date)).toBe('2026-11-25T08:30:15Z');
    expect(parseIso8601('2026-11-25T08:30:15.123Z')?.getTime()).toBe(
      Date.UTC(2026, 10, 25, 8, 30, 15, 123),
    );
    expect(parseIso8601('2026-11-25T08:30:15+02:00')?.toISOString()).toBe(
      '2026-11-25T06:30:15.000Z',
    );
    expect(parseIso8601('garbage')).toBeNull();
  });
});
