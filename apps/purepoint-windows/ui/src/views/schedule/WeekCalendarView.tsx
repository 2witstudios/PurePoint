// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/WeekCalendarView.swift (WIN-8.8).
import { dayKey, formatWeekdayShort } from '../../model/scheduleCalendar.ts';
import type { ScheduleState } from '../../state/scheduleState.ts';
import { GUTTER_WIDTH, TimeGridView } from './TimeGridView.tsx';
import { useScheduleFields } from './useScheduleState.ts';

export function WeekCalendarView({ state }: { state: ScheduleState }) {
  useScheduleFields(state);
  const days = state.weekDays;
  return (
    <div className="schedule-calendar" data-view="WeekCalendarView">
      <div
        className="schedule-week-header"
        style={{ gridTemplateColumns: `${String(GUTTER_WIDTH)}px repeat(7, minmax(0, 1fr))` }}
      >
        <span />
        {days.map((date) => {
          const isToday = state.isToday(date);
          return (
            <div key={dayKey(date)} className="schedule-week-day" data-date={dayKey(date)}>
              <span className="schedule-week-day-name">
                {formatWeekdayShort(date, state.locale).toUpperCase()}
              </span>
              <span className={`schedule-week-day-number${isToday ? ' schedule-today-badge' : ''}`}>
                {date.getDate()}
              </span>
            </div>
          );
        })}
      </div>
      <TimeGridView state={state} dates={days} />
    </div>
  );
}
