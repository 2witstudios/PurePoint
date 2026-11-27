// Calendar math for the Schedule screen (WIN-8.8): the parts of Foundation's `Calendar.current`
// that State/ScheduleState.swift and Models/ScheduleEvent.swift lean on, in the user's local
// time zone.
//
// Every helper works on wall-clock fields (`new Date(y, m, d, h, min)`), so a day is a calendar
// day rather than 24 hours: adding a day across a DST change keeps the time of day. Where a wall
// time does not exist (the spring-forward gap) `atTime` returns the first instant after the gap,
// as `Calendar.date(bySettingHour:minute:second:of:)` does with its default `.nextTime` policy;
// a repeated wall time (fall back) resolves to its first occurrence, as Swift's `.first` does.

/** JS weekday numbers: 0 = Sunday … 6 = Saturday. */
export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6;

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

/** The locale's first day of the week (`Calendar.current.firstWeekday`), Sunday when unknown. */
export function localeFirstWeekday(locale?: string): Weekday {
  try {
    const tag = locale ?? Intl.DateTimeFormat().resolvedOptions().locale;
    const intlLocale = new Intl.Locale(tag) as Intl.Locale & {
      getWeekInfo?: () => { firstDay: number };
      weekInfo?: { firstDay: number };
    };
    const info = intlLocale.getWeekInfo?.() ?? intlLocale.weekInfo;
    if (info !== undefined) return (info.firstDay % 7) as Weekday; // Intl: 1 = Monday … 7 = Sunday
  } catch {
    // Older engines have no week info.
  }
  return 0;
}

/** `calendar.startOfDay(for:)`: the first instant of the date's local day. */
export function startOfDay(date: Date): Date {
  return atTime(date, 0, 0);
}

/** Wall-clock fields as one comparable number (ordering only, not an instant). */
function wallKey(y: number, mo: number, d: number, h: number, mi: number): number {
  return Date.UTC(y, mo, d, h, mi);
}

function wallKeyOf(date: Date): number {
  return wallKey(
    date.getFullYear(),
    date.getMonth(),
    date.getDate(),
    date.getHours(),
    date.getMinutes(),
  );
}

/**
 * `calendar.date(bySettingHour:minute:second: 0, of:)`: the date's local day at `hour:minute`.
 * In a DST gap the result is the first instant after the gap (`.nextTime`).
 */
export function atTime(date: Date, hour: number, minute: number): Date {
  const y = date.getFullYear();
  const mo = date.getMonth();
  const d = date.getDate();
  const result = new Date(y, mo, d, hour, minute, 0, 0);
  if (result.getHours() === hour && result.getMinutes() === minute && result.getDate() === d) {
    return result;
  }
  // The wall time is in a gap and JS pushed it forward by the gap's length. Find the first
  // instant whose wall clock is at or past the requested time: that is where the gap ends.
  const wanted = wallKey(y, mo, d, hour, minute);
  let lo = result.getTime() - 4 * HOUR;
  let hi = result.getTime();
  while (hi - lo > MINUTE) {
    const mid = Math.floor((lo + hi) / 2 / MINUTE) * MINUTE;
    if (mid <= lo || mid >= hi) break;
    if (wallKeyOf(new Date(mid)) >= wanted) hi = mid;
    else lo = mid;
  }
  return new Date(hi);
}

/** `calendar.date(byAdding: .day, value:, to:)`: the same time of day `days` calendar days on. */
export function addDays(date: Date, days: number): Date {
  const shifted = new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
  return atTime(shifted, date.getHours(), date.getMinutes());
}

/** `calendar.date(byAdding: .month, …)`: clamps the day to the target month (Jan 31 → Feb 28). */
export function addMonths(date: Date, months: number): Date {
  const first = new Date(date.getFullYear(), date.getMonth() + months, 1);
  const lastDay = new Date(first.getFullYear(), first.getMonth() + 1, 0).getDate();
  const day = Math.min(date.getDate(), lastDay);
  return atTime(
    new Date(first.getFullYear(), first.getMonth(), day),
    date.getHours(),
    date.getMinutes(),
  );
}

/** `calendar.isDate(_:inSameDayAs:)`. */
export function isSameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() &&
    a.getMonth() === b.getMonth() &&
    a.getDate() === b.getDate()
  );
}

/** The next calendar day's start: the exclusive end of `date`'s day. */
export function startOfNextDay(date: Date): Date {
  return startOfDay(new Date(date.getFullYear(), date.getMonth(), date.getDate() + 1));
}

/** `calendar.dateInterval(of: .weekOfYear, for:).start`. */
export function startOfWeek(date: Date, firstWeekday: Weekday): Date {
  const back = (date.getDay() - firstWeekday + 7) % 7;
  return startOfDay(new Date(date.getFullYear(), date.getMonth(), date.getDate() - back));
}

/** ScheduleState `weekDays`: the seven days of the date's week. */
export function weekDays(date: Date, firstWeekday: Weekday): Date[] {
  const start = startOfWeek(date, firstWeekday);
  return Array.from({ length: 7 }, (_, i) =>
    startOfDay(new Date(start.getFullYear(), start.getMonth(), start.getDate() + i)),
  );
}

/** ScheduleState `monthDays`: a 6-week (42-cell) grid starting on the week of the 1st. */
export function monthDays(date: Date, firstWeekday: Weekday): Date[] {
  const first = new Date(date.getFullYear(), date.getMonth(), 1);
  const leading = (first.getDay() - firstWeekday + 7) % 7;
  return Array.from({ length: 42 }, (_, i) =>
    startOfDay(new Date(first.getFullYear(), first.getMonth(), 1 - leading + i)),
  );
}

/** `Calendar.shortWeekdaySymbols` rotated to start on `firstWeekday` (Sun, Mon, …). */
export function shortWeekdaySymbols(firstWeekday: Weekday, locale?: string): string[] {
  const format = new Intl.DateTimeFormat(locale, { weekday: 'short' });
  // 2023-01-01 was a Sunday.
  return Array.from({ length: 7 }, (_, i) =>
    format.format(new Date(2023, 0, 1 + ((firstWeekday + i) % 7))),
  );
}

/** The local hour and minute of a date. */
export function timeOfDay(date: Date): { hour: number; minute: number } {
  return { hour: date.getHours(), minute: date.getMinutes() };
}

// MARK: - Formatting (Swift DateFormatter patterns, localized names)

/** "MMMM yyyy" */
export function formatMonthYear(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { month: 'long', year: 'numeric' }).format(date);
}

/** "MMM d" */
export function formatShortDate(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { month: 'short', day: 'numeric' }).format(date);
}

/** "EEEE, MMMM d" */
export function formatDayLong(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, {
    weekday: 'long',
    month: 'long',
    day: 'numeric',
  }).format(date);
}

/** "EEE" */
export function formatWeekdayShort(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { weekday: 'short' }).format(date);
}

/** "h:mm a" */
export function formatTime(date: Date, locale?: string): string {
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit' }).format(date);
}

/** "EEEE 'at' h:mm a" */
export function formatWeekdayAtTime(date: Date, locale?: string): string {
  const weekday = new Intl.DateTimeFormat(locale, { weekday: 'long' }).format(date);
  return `${weekday} at ${formatTime(date, locale)}`;
}

/** TimeGridView `hourLabel`: "12 AM", "1 AM" … "11 PM". */
export function hourLabel(hour: number): string {
  const period = hour < 12 ? 'AM' : 'PM';
  const display = hour === 0 ? 12 : hour > 12 ? hour - 12 : hour;
  return `${String(display)} ${period}`;
}

/** A stable local-day key, `YYYY-MM-DD`. */
export function dayKey(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** `ISO8601DateFormatter` with `.withInternetDateTime`: UTC, seconds, `Z`. */
export function formatIso8601(date: Date): string {
  return date.toISOString().replace(/\.\d{3}Z$/, 'Z');
}

/** Parses RFC 3339 with or without fractional seconds; null when it is not a date. */
export function parseIso8601(text: string): Date | null {
  const time = Date.parse(text);
  return Number.isNaN(time) ? null : new Date(time);
}

/** `<input type="datetime-local">` value for a local date: `YYYY-MM-DDTHH:MM`. */
export function toDateTimeLocalValue(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${dayKey(date)}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Parses a `datetime-local` value as local time (gap times move past the gap). */
export function fromDateTimeLocalValue(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})/.exec(value);
  if (match === null) return null;
  const [, y, mo, d, h, mi] = match.map(Number) as [number, number, number, number, number, number];
  return atTime(new Date(y, mo - 1, d), h, mi);
}
