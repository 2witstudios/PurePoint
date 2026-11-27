// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/EventPillView.swift (WIN-8.8).
//
// One occurrence in a month cell: type icon, name and recurrence badge on a tinted pill.
import type { CSSProperties } from 'react';
import { dayKey } from '../../model/scheduleCalendar.ts';
import {
  eventColorCss,
  recurrenceBadge,
  SCHEDULE_TYPE_ICON,
  type ScheduleEvent,
} from '../../model/scheduleEvent.ts';
import { Icon } from '../../theme/Icon.tsx';

export interface EventPillViewProps {
  event: ScheduleEvent;
  occurrence: Date;
  onClick: () => void;
}

export function EventPillView({ event, occurrence, onClick }: EventPillViewProps) {
  const badge = recurrenceBadge(event.recurrence);
  return (
    <button
      type="button"
      className="schedule-pill"
      data-testid="schedule-event"
      data-name={event.name}
      data-date={dayKey(occurrence)}
      style={{ '--event-color': eventColorCss(event.colorIndex) } as CSSProperties}
      onClick={(e) => {
        // The cell behind the pill creates a schedule; the pill opens this one.
        e.stopPropagation();
        onClick();
      }}
    >
      <Icon name={SCHEDULE_TYPE_ICON[event.type]} size={8} />
      <span className="schedule-pill-name">{event.name}</span>
      {badge !== null && <span className="schedule-pill-badge">{badge}</span>}
    </button>
  );
}
