// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/TimeGridView.swift (WIN-8.8).
//
// The 24-hour grid behind the day and week views: hour labels in the gutter, one column per date,
// click-to-create on an empty slot (rounded down to the quarter hour), event blocks at their time
// of day, and the current-time line when today is on screen. It opens scrolled to two hours
// before now, as the Swift `ScrollViewReader` does.
import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent } from 'react';
import { atTime, dayKey, hourLabel, isSameDay } from '../../model/scheduleCalendar.ts';
import { occurrencesOnDay, type ScheduleState } from '../../state/scheduleState.ts';
import { EventBlockView } from './EventBlockView.tsx';
import { useScheduleFields } from './useScheduleState.ts';

/** PurePointTheme.calendarHourHeight and calendarTimeGutterWidth (tokens.css). */
export const HOUR_HEIGHT = 60;
export const GUTTER_WIDTH = 56;
const TOTAL_HOURS = 24;
const HOURS = Array.from({ length: TOTAL_HOURS }, (_, hour) => hour);

/** The minute a click `offsetY` pixels into an hour cell means, floored to the quarter hour. */
export function slotMinute(offsetY: number): number {
  const clamped = Math.min(Math.max(offsetY, 0), HOUR_HEIGHT - 1);
  const minute = Math.floor((clamped / HOUR_HEIGHT) * 60);
  return Math.floor(minute / 15) * 15;
}

function useMinuteClock(state: ScheduleState): Date {
  const [now, setNow] = useState(() => state.nowDate());
  useEffect(() => {
    const timer = window.setInterval(() => {
      setNow(state.nowDate());
    }, 60_000);
    return () => {
      window.clearInterval(timer);
    };
  }, [state]);
  return now;
}

export interface TimeGridViewProps {
  state: ScheduleState;
  dates: readonly Date[];
}

export function TimeGridView({ state, dates }: TimeGridViewProps) {
  const { events } = useScheduleFields(state);
  const now = useMinuteClock(state);
  const scroller = useRef<HTMLDivElement>(null);
  const columns = Math.max(dates.length, 1);
  const blockHeight = Math.max(HOUR_HEIGHT * 0.75, 30);
  const showsToday = dates.some((date) => isSameDay(date, now));
  const nowY = (now.getHours() + now.getMinutes() / 60) * HOUR_HEIGHT;

  useLayoutEffect(() => {
    const element = scroller.current;
    if (element === null) return;
    element.scrollTop = Math.max(0, (state.nowDate().getHours() - 2) * HOUR_HEIGHT);
  }, [state]);

  const clickSlot = (date: Date, hour: number) => (event: MouseEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    state.handleTimeSlotClick(atTime(date, hour, slotMinute(event.clientY - rect.top)));
  };

  return (
    <div className="schedule-time-grid" ref={scroller}>
      <div
        className="schedule-time-grid-content"
        style={{
          height: TOTAL_HOURS * HOUR_HEIGHT,
          gridTemplateColumns: `${String(GUTTER_WIDTH)}px repeat(${String(columns)}, minmax(0, 1fr))`,
        }}
      >
        <div className="schedule-gutter">
          {HOURS.map((hour) => (
            <span key={hour} className="schedule-hour-label" style={{ top: hour * HOUR_HEIGHT }}>
              {hourLabel(hour)}
            </span>
          ))}
        </div>
        {dates.map((date) => (
          <div key={dayKey(date)} className="schedule-column" data-date={dayKey(date)}>
            {HOURS.map((hour) => (
              <div
                key={hour}
                className={`schedule-slot${hour % 6 === 0 ? ' schedule-slot-major' : ''}`}
                data-slot={`${dayKey(date)}T${String(hour).padStart(2, '0')}`}
                style={{ height: HOUR_HEIGHT }}
                onClick={clickSlot(date, hour)}
              />
            ))}
            {occurrencesOnDay(events, date).map(({ date: occurrence, event }) => (
              <EventBlockView
                key={`${event.id}-${String(occurrence.getTime())}`}
                event={event}
                occurrence={occurrence}
                height={blockHeight}
                style={{
                  top: (occurrence.getHours() + occurrence.getMinutes() / 60) * HOUR_HEIGHT,
                }}
                onClick={() => {
                  state.handleEventClick(event);
                }}
              />
            ))}
          </div>
        ))}
        {showsToday && (
          <div
            className="schedule-now-line"
            data-testid="schedule-now-line"
            style={{ top: nowY, left: GUTTER_WIDTH - 4 }}
          >
            <span className="schedule-now-dot" />
          </div>
        )}
      </div>
    </div>
  );
}
