// Port of apps/purepoint-macos/purepoint-macos/Views/Schedule/ScheduleHeaderView.swift (WIN-8.8).
//
// The view-mode picker (Month, Week, Day, List), the back / Today / forward navigation with the
// date label, and New Schedule.
import {
  SCHEDULE_VIEW_MODE_LABEL,
  SCHEDULE_VIEW_MODES,
  type ScheduleState,
} from '../../state/scheduleState.ts';
import { Icon } from '../../theme/Icon.tsx';
import { useScheduleFields } from './useScheduleState.ts';

export function ScheduleHeaderView({ state }: { state: ScheduleState }) {
  const { viewMode } = useScheduleFields(state);
  return (
    <div className="schedule-header">
      <div className="segmented schedule-mode-picker" role="tablist" aria-label="Schedule view">
        {SCHEDULE_VIEW_MODES.map((mode) => (
          <button
            key={mode}
            type="button"
            role="tab"
            aria-selected={viewMode === mode}
            onClick={() => {
              state.setViewMode(mode);
            }}
          >
            {SCHEDULE_VIEW_MODE_LABEL[mode]}
          </button>
        ))}
      </div>
      <span className="schedule-spacer" />
      <div className="schedule-navigation">
        <button
          type="button"
          className="schedule-borderless"
          aria-label="Previous"
          title="Previous"
          onClick={() => {
            state.goBackward();
          }}
        >
          <Icon name="chevron.left" size={11} strokeWidth={2.5} />
        </button>
        <button
          type="button"
          className="schedule-borderless schedule-today-button"
          onClick={() => {
            state.goToToday();
          }}
        >
          Today
        </button>
        <button
          type="button"
          className="schedule-borderless"
          aria-label="Next"
          title="Next"
          onClick={() => {
            state.goForward();
          }}
        >
          <Icon name="chevron.right" size={11} strokeWidth={2.5} />
        </button>
        <span className="schedule-date-label" data-testid="schedule-date-label">
          {state.headerDateLabel}
        </span>
      </div>
      <span className="schedule-spacer" />
      <button
        type="button"
        className="schedule-borderless schedule-new-button"
        onClick={() => {
          state.showCreationSheet();
        }}
      >
        <Icon name="plus" size={12} />
        <span>New Schedule</span>
      </button>
    </div>
  );
}
