// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/ScheduleView.swift (WIN-8.8).
//
// The Schedule screen: header, then the month, week, day or list view. It loads every local
// project's schedules when it appears and again when the project list changes.
import { useEffect } from 'react';
import { useStore } from 'zustand';
import { scheduleStateFor, type ScheduleState } from '../../state/scheduleState.ts';
import { useAppState } from '../shell/appContext.ts';
import { DayCalendarView } from './DayCalendarView.tsx';
import { MonthCalendarView } from './MonthCalendarView.tsx';
import { ScheduleHeaderView } from './ScheduleHeaderView.tsx';
import { ScheduleListView } from './ScheduleListView.tsx';
import { useScheduleFields } from './useScheduleState.ts';
import { WeekCalendarView } from './WeekCalendarView.tsx';
import './schedule.css';

function CalendarContent({ state }: { state: ScheduleState }) {
  const { viewMode } = useScheduleFields(state);
  switch (viewMode) {
    case 'month':
      return <MonthCalendarView state={state} />;
    case 'week':
      return <WeekCalendarView state={state} />;
    case 'day':
      return <DayCalendarView state={state} />;
    case 'list':
      return <ScheduleListView state={state} />;
  }
}

export function ScheduleView({ state: given }: { state?: ScheduleState }) {
  const app = useAppState();
  const state = given ?? scheduleStateFor(app);
  // Swift reloads on `appState.projects.count`; the roots themselves are the sharper key.
  const rootsKey = useStore(app.store, (s) => s.projects.map((p) => p.projectRoot).join('\n'));

  useEffect(() => {
    void state.loadSchedules(rootsKey === '' ? [] : rootsKey.split('\n'));
  }, [state, rootsKey]);

  return (
    <div className="schedule-view" data-view="ScheduleView">
      <ScheduleHeaderView state={state} />
      <CalendarContent state={state} />
    </div>
  );
}
