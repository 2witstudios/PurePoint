// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/ScheduleView.swift (WIN-8.8).
//
// The Schedule screen: header, then the month, week, day or list view. It loads every local
// project's schedules when it appears and again when the project list changes. The event sheet
// (WIN-8.17) opens over it.
import { useEffect } from 'react';
import { useStore } from 'zustand';
import { scheduleStateFor, type ScheduleState } from '../../state/scheduleState.ts';
import { useAppState } from '../shell/appContext.ts';
import { DayCalendarView } from './DayCalendarView.tsx';
import { MonthCalendarView } from './MonthCalendarView.tsx';
import { ScheduleEventSheet } from './ScheduleEventSheet.tsx';
import { ScheduleHeaderView } from './ScheduleHeaderView.tsx';
import { ScheduleListView } from './ScheduleListView.tsx';
import { useScheduleFields } from './useScheduleState.ts';
import { WeekCalendarView } from './WeekCalendarView.tsx';
import './schedule.css';

/** Swift's two `.sheet`s: creation (with the clicked slot's date) and edit. */
function SheetHost({ state }: { state: ScheduleState }) {
  const { sheet } = useScheduleFields(state);
  if (sheet === null) return null;
  return sheet.mode === 'create' ? (
    <ScheduleEventSheet state={state} editingEvent={null} prefillDate={sheet.prefillDate} />
  ) : (
    <ScheduleEventSheet
      key={sheet.event.id}
      state={state}
      editingEvent={sheet.event}
      prefillDate={null}
    />
  );
}

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
      <SheetHost state={state} />
    </div>
  );
}
