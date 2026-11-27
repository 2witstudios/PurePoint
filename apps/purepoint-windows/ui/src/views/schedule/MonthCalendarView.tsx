// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/MonthCalendarView.swift (WIN-8.8).
//
// A 6-week grid: each cell shows its day number, up to three event pills and "+N more"; clicking
// the cell creates a schedule at 9:00 AM that day.
import { atTime, dayKey, shortWeekdaySymbols } from '../../model/scheduleCalendar.ts';
import { occurrencesOnDay, type ScheduleState } from '../../state/scheduleState.ts';
import { EventPillView } from './EventPillView.tsx';
import { useScheduleFields } from './useScheduleState.ts';

const MAX_VISIBLE = 3;

export function MonthCalendarView({ state }: { state: ScheduleState }) {
  const { events } = useScheduleFields(state);
  const symbols = shortWeekdaySymbols(state.firstWeekday, state.locale);
  return (
    <div className="schedule-calendar" data-view="MonthCalendarView">
      <div className="schedule-month-weekdays">
        {symbols.map((symbol) => (
          <span key={symbol}>{symbol}</span>
        ))}
      </div>
      <div className="schedule-month-grid">
        {state.monthDays.map((date) => {
          const isToday = state.isToday(date);
          const isCurrentMonth = state.isCurrentMonth(date);
          const dayEvents = occurrencesOnDay(events, date);
          return (
            <div
              key={dayKey(date)}
              className={`schedule-month-cell${isToday ? ' schedule-month-today' : ''}`}
              data-date={dayKey(date)}
              onClick={() => {
                state.handleTimeSlotClick(atTime(date, 9, 0));
              }}
            >
              <span
                className={`schedule-month-day${isToday ? ' schedule-today-badge' : ''}${
                  isCurrentMonth ? '' : ' schedule-month-day-outside'
                }`}
              >
                {date.getDate()}
              </span>
              {dayEvents.slice(0, MAX_VISIBLE).map(({ date: occurrence, event }) => (
                <EventPillView
                  key={`${event.id}-${String(occurrence.getTime())}`}
                  event={event}
                  occurrence={occurrence}
                  onClick={() => {
                    state.handleEventClick(event);
                  }}
                />
              ))}
              {dayEvents.length > MAX_VISIBLE && (
                <span className="schedule-month-more">
                  {`+${String(dayEvents.length - MAX_VISIBLE)} more`}
                </span>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
