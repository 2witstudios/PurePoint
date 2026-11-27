// WIN-8.8: the Schedule screen's month, week, day and list views over a scripted daemon.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Request, Response, ScheduleInfo } from '../../protocol';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => new Promise<never>(() => undefined)),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));

const { ScheduleState } = await import('../../state/scheduleState.ts');
const { ScheduleView } = await import('./ScheduleView.tsx');
const { AppStateContext } = await import('../shell/appContext.ts');
const { makeTestApp } = await import('../shell/testApp.ts');
const { slotMinute } = await import('./TimeGridView.tsx');

const ROOT = 'C:\\repo\\demo';
// Wednesday 25 November 2026, 10:00 local.
const NOW = new Date(2026, 10, 25, 10, 0);

function localIso(y: number, mo: number, d: number, h: number, mi = 0): string {
  return new Date(y, mo, d, h, mi).toISOString();
}

function info(name: string, recurrence: string, startAt: string): ScheduleInfo {
  return {
    name,
    enabled: true,
    recurrence,
    start_at: startAt,
    next_run: null,
    trigger: { type: 'inline_prompt', prompt: 'echo', agent: 'terminal' },
    project_root: ROOT,
    target: '',
    scope: 'local',
    root: true,
    agent_name: null,
    created_at: startAt,
  };
}

const SCHEDULES = [
  info('standup', 'weekdays', localIso(2026, 10, 2, 8)),
  info('deploy', 'none', localIso(2026, 10, 26, 15, 30)),
];

function setup() {
  const requests: Request[] = [];
  const client = {
    send(request: Request): Promise<Response> {
      requests.push(request);
      return Promise.resolve(
        request.type === 'list_schedules'
          ? { type: 'schedule_list', schedules: SCHEDULES }
          : { type: 'ok' },
      );
    },
  };
  const state = new ScheduleState({ client, now: () => NOW, firstWeekday: 0, locale: 'en-US' });
  const app = makeTestApp();
  app.applyLocalProjects([ROOT]);
  render(
    <AppStateContext.Provider value={app}>
      <ScheduleView state={state} />
    </AppStateContext.Provider>,
  );
  return { state, requests };
}

function eventsNamed(name: string): HTMLElement[] {
  return screen
    .queryAllByTestId('schedule-event')
    .filter((element) => element.dataset.name === name);
}

function selectMode(label: string) {
  fireEvent.click(screen.getByRole('tab', { name: label }));
}

describe('ScheduleView', () => {
  it('should load the project schedules and start on the week view', async () => {
    const { requests } = setup();
    await waitFor(() => {
      expect(eventsNamed('standup')).toHaveLength(5);
    });
    expect(requests).toContainEqual({ type: 'list_schedules', project_root: ROOT });
    expect(screen.getByRole('tab', { name: 'Week' })).toHaveAttribute('aria-selected', 'true');
    expect(screen.getByTestId('schedule-date-label')).toHaveTextContent('Nov 22 – Nov 28');
    expect(eventsNamed('deploy').map((e) => e.dataset.date)).toEqual(['2026-11-26']);
    const standup = eventsNamed('standup')[0];
    expect(standup).toHaveTextContent('standup');
    expect(standup).toHaveTextContent('8:00 AM');
    expect(standup).toHaveTextContent('Weekdays');
    expect(screen.getByTestId('schedule-now-line')).toBeInTheDocument();
  });

  it('should show the day view and navigate by day', async () => {
    setup();
    selectMode('Day');
    await waitFor(() => {
      expect(eventsNamed('standup')).toHaveLength(1);
    });
    expect(screen.getByTestId('schedule-date-label')).toHaveTextContent('Wednesday, November 25');
    expect(eventsNamed('deploy')).toHaveLength(0);
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    expect(eventsNamed('deploy')).toHaveLength(1);
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    fireEvent.click(screen.getByRole('button', { name: 'Next' }));
    // Saturday: no weekday standup.
    expect(screen.getByTestId('schedule-date-label')).toHaveTextContent('Saturday, November 28');
    expect(eventsNamed('standup')).toHaveLength(0);
    fireEvent.click(screen.getByRole('button', { name: 'Today' }));
    expect(screen.getByTestId('schedule-date-label')).toHaveTextContent('Wednesday, November 25');
  });

  it('should show the month view with pills and a 42-day grid', async () => {
    const { state } = setup();
    selectMode('Month');
    await waitFor(() => {
      expect(eventsNamed('deploy')).toHaveLength(1);
    });
    expect(screen.getByTestId('schedule-date-label')).toHaveTextContent('November 2026');
    expect(document.querySelectorAll('.schedule-month-cell')).toHaveLength(42);
    // Weekdays from Nov 2 (its start) through Dec 11 inside the grid (Nov 1 – Dec 12).
    expect(eventsNamed('standup')).toHaveLength(30);
    fireEvent.click(screen.getByRole('button', { name: 'Previous' }));
    expect(screen.getByTestId('schedule-date-label')).toHaveTextContent('October 2026');
    // Clicking a cell starts a schedule at 9:00 that day.
    const cell = document.querySelector<HTMLElement>('[data-date="2026-10-14"]');
    if (cell === null) throw new Error('no cell');
    fireEvent.click(cell);
    const sheet = state.store.getState().sheet;
    expect(sheet?.mode === 'create' ? sheet.prefillDate : null).toEqual(new Date(2026, 9, 14, 9));
  });

  it('should list the next 30 days grouped by day', async () => {
    setup();
    selectMode('List');
    await waitFor(() => {
      expect(screen.getByText('Today')).toBeInTheDocument();
    });
    expect(screen.getByText('Tomorrow')).toBeInTheDocument();
    expect(screen.getByText('Friday, November 27')).toBeInTheDocument();
    const tomorrow = screen.getByText('Tomorrow').closest('section');
    if (tomorrow === null) throw new Error('no section');
    expect(
      within(tomorrow)
        .getAllByTestId('schedule-event')
        .map((e) => e.dataset.name),
    ).toEqual(['standup', 'deploy']);
    expect(within(tomorrow).getByText('3:30 PM')).toBeInTheDocument();
    expect(within(tomorrow).getAllByText('demo')).toHaveLength(2);
  });

  it('should open the sheet from a slot (quarter-hour floor) and from an event', async () => {
    const { state } = setup();
    await waitFor(() => {
      expect(eventsNamed('deploy')).toHaveLength(1);
    });
    expect([0, 14, 15, 44, 59, 200].map(slotMinute)).toEqual([0, 0, 15, 30, 45, 45]);
    const slot = document.querySelector<HTMLElement>('[data-slot="2026-11-27T14"]');
    if (slot === null) throw new Error('no slot');
    act(() => {
      fireEvent.click(slot, { clientY: 0 });
    });
    const created = state.store.getState().sheet;
    expect(created?.mode === 'create' ? created.prefillDate : null).toEqual(
      new Date(2026, 10, 27, 14, 0),
    );
    const deploy = eventsNamed('deploy')[0];
    if (deploy === undefined) throw new Error('no event');
    fireEvent.click(deploy);
    const edited = state.store.getState().sheet;
    expect(edited?.mode === 'edit' ? edited.event.name : null).toBe('deploy');
    fireEvent.click(screen.getByRole('button', { name: 'New Schedule' }));
    expect(state.store.getState().sheet).toEqual({ mode: 'create', prefillDate: null });
  });
});
