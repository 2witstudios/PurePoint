// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/DayCalendarView.swift (WIN-8.8).
import { isSameDay } from '../../model/scheduleCalendar.ts';
import type { ScheduleState } from '../../state/scheduleState.ts';
import { TimeGridView } from './TimeGridView.tsx';
import { useScheduleFields } from './useScheduleState.ts';

export function DayCalendarView({ state }: { state: ScheduleState }) {
  const { currentDate } = useScheduleFields(state);
  const isToday = isSameDay(currentDate, state.nowDate());
  return (
    <div className="schedule-calendar" data-view="DayCalendarView">
      <div className="schedule-day-header">
        <span className={`schedule-day-title${isToday ? ' schedule-today-text' : ''}`}>
          {state.currentDayFormatted}
        </span>
      </div>
      <TimeGridView state={state} dates={[currentDate]} />
    </div>
  );
}
