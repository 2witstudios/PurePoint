// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/EventBlockView.swift (WIN-8.8).
//
// One occurrence in the day and week time grids: accent bar, name, then (when the block is tall
// enough) the type icon and time, the project and target, and the recurrence badge.
import type { CSSProperties } from 'react';
import { dayKey, formatTime } from '../../model/scheduleCalendar.ts';
import {
  eventColorCss,
  recurrenceBadge,
  SCHEDULE_TYPE_ICON,
  type ScheduleEvent,
} from '../../model/scheduleEvent.ts';
import { Icon } from '../../theme/Icon.tsx';

export interface EventBlockViewProps {
  event: ScheduleEvent;
  occurrence: Date;
  height: number;
  style?: CSSProperties;
  onClick: () => void;
}

export function eventSubtitle(event: ScheduleEvent): string {
  return event.target === '' ? event.projectName : `${event.projectName} · ${event.target}`;
}

export function EventBlockView({ event, occurrence, height, style, onClick }: EventBlockViewProps) {
  const badge = recurrenceBadge(event.recurrence);
  return (
    <button
      type="button"
      className="schedule-block"
      data-testid="schedule-event"
      data-name={event.name}
      data-date={dayKey(occurrence)}
      title={event.name}
      style={
        { ...style, height, '--event-color': eventColorCss(event.colorIndex) } as CSSProperties
      }
      onClick={(e) => {
        e.stopPropagation();
        onClick();
      }}
    >
      <span className="schedule-block-bar" />
      <span className="schedule-block-body">
        <span className="schedule-block-name">{event.name}</span>
        {height > 36 && (
          <span className="schedule-block-time">
            <Icon name={SCHEDULE_TYPE_ICON[event.type]} size={9} />
            <span>{formatTime(occurrence)}</span>
          </span>
        )}
        {height > 52 && <span className="schedule-block-subtitle">{eventSubtitle(event)}</span>}
      </span>
      {badge !== null && height > 36 && <span className="schedule-block-badge">{badge}</span>}
    </button>
  );
}
