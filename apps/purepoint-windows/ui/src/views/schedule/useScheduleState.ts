// Subscribes a schedule view to its ScheduleState store (Swift `@Bindable var state`).
import { useStore } from 'zustand';
import type { ScheduleState, ScheduleStateFields } from '../../state/scheduleState.ts';

export function useScheduleFields(state: ScheduleState): ScheduleStateFields {
  return useStore(state.store);
}
