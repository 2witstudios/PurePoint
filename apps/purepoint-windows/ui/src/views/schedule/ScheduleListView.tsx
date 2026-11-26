// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/ScheduleListView.swift (WIN-8.8).
//
// The next 30 days of occurrences from the current date, grouped by day ("Today", "Tomorrow",
// then "EEEE, MMMM d"), one row each: time, color bar, type icon, name, badge, project · target.
import type { CSSProperties } from 'react';
import { dayKey, formatDayLong, formatTime, startOfDay } from '../../model/scheduleCalendar.ts';
import {
  eventColorCss,
  recurrenceBadge,
  SCHEDULE_TYPE_ICON,
  type ScheduleEvent,
} from '../../model/scheduleEvent.ts';
import { Icon } from '../../theme/Icon.tsx';
import type { Occurrence, ScheduleState } from '../../state/scheduleState.ts';
import { eventSubtitle } from './EventBlockView.tsx';
import { useScheduleFields } from './useScheduleState.ts';

interface DayGroup {
  readonly key: string;
  readonly label: string;
  readonly items: readonly Occurrence[];
}

export function groupedOccurrences(state: ScheduleState): DayGroup[] {
  const { start, end } = state.listRange();
  const groups = new Map<string, { day: Date; items: Occurrence[] }>();
  for (const occurrence of state.expandedOccurrences(start, end)) {
    const day = startOfDay(occurrence.date);
    const key = dayKey(day);
    const group = groups.get(key) ?? { day, items: [] };
    group.items.push(occurrence);
    groups.set(key, group);
  }
  return [...groups.entries()]
    .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
    .map(([key, { day, items }]) => ({
      key,
      label: state.isToday(day)
        ? 'Today'
        : state.isTomorrow(day)
          ? 'Tomorrow'
          : formatDayLong(day, state.locale),
      items,
    }));
}

function EventRow({
  event,
  date,
  onClick,
}: {
  event: ScheduleEvent;
  date: Date | null;
  onClick: () => void;
}) {
  const badge = recurrenceBadge(event.recurrence);
  return (
    <button
      type="button"
      className="schedule-list-row"
      data-testid="schedule-event"
      data-name={event.name}
      {...(date === null ? {} : { 'data-date': dayKey(date) })}
      style={{ '--event-color': eventColorCss(event.colorIndex) } as CSSProperties}
      onClick={onClick}
    >
      <span className="schedule-list-time">{date === null ? '' : formatTime(date)}</span>
      <span className="schedule-list-bar" />
      <span className="schedule-list-details">
        <span className="schedule-list-title">
          <Icon name={SCHEDULE_TYPE_ICON[event.type]} size={10} className="schedule-list-icon" />
          <span className="schedule-list-name">{event.name}</span>
          {badge !== null && <span className="schedule-list-badge">{badge}</span>}
        </span>
        <span className="schedule-list-subtitle">{eventSubtitle(event)}</span>
      </span>
    </button>
  );
}

export function ScheduleListView({ state }: { state: ScheduleState }) {
  useScheduleFields(state);
  const groups = groupedOccurrences(state);
  return (
    <div className="schedule-list" data-view="ScheduleListView">
      {groups.map((group) => (
        <section key={group.key} className="schedule-list-section">
          <div className="schedule-list-header">{group.label}</div>
          {group.items.map(({ date, event }) => (
            <EventRow
              key={`${event.id}-${String(date.getTime())}`}
              event={event}
              date={date}
              onClick={() => {
                state.handleEventClick(event);
              }}
            />
          ))}
        </section>
      ))}
    </div>
  );
}
