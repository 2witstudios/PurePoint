// WIN-8.17: the schedule event sheet's fields, validation and actions over a scripted daemon.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Request, Response, ScheduleInfo } from '../../protocol';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => new Promise<never>(() => undefined)),
}));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => undefined)) }));

const { ScheduleState } = await import('../../state/scheduleState.ts');
const { ScheduleView } = await import('./ScheduleView.tsx');
const { buildTrigger, nextRunPreview } = await import('./ScheduleEventSheet.tsx');
const { AppStateContext } = await import('../shell/appContext.ts');
const { makeTestApp } = await import('../shell/testApp.ts');
const { formatIso8601 } = await import('../../model/scheduleCalendar.ts');

const ROOT = 'C:\\repo\\demo';
const OTHER = 'C:\\repo\\other';
const NOW = new Date(2026, 10, 25, 10, 0);

function info(overrides: Partial<ScheduleInfo> = {}): ScheduleInfo {
  return {
    name: 'nightly',
    enabled: true,
    recurrence: 'daily',
    start_at: new Date(2026, 10, 20, 9, 0).toISOString(),
    next_run: null,
    trigger: { type: 'inline_prompt', prompt: 'echo hi', agent: 'terminal' },
    project_root: ROOT,
    target: 'src',
    scope: 'local',
    root: false,
    agent_name: 'night-branch',
    created_at: '2026-11-20T00:00:00Z',
    ...overrides,
  };
}

function setup(schedules: ScheduleInfo[] = [], failWith: string | null = null) {
  const requests: Request[] = [];
  let current = [...schedules];
  const client = {
    send(request: Request): Promise<Response> {
      requests.push(request);
      if (request.type === 'list_schedules') {
        return Promise.resolve({ type: 'schedule_list', schedules: current });
      }
      if (failWith !== null) {
        return Promise.resolve({ type: 'error', code: 'INVALID_INPUT', message: failWith });
      }
      if (request.type === 'disable_schedule' || request.type === 'enable_schedule') {
        current = current.map((s) =>
          s.name === request.name ? { ...s, enabled: request.type === 'enable_schedule' } : s,
        );
      }
      if (request.type === 'delete_schedule') {
        current = current.filter((s) => s.name !== request.name);
      }
      return Promise.resolve({ type: 'ok' });
    },
  };
  const state = new ScheduleState({ client, now: () => NOW, firstWeekday: 0, locale: 'en-US' });
  const app = makeTestApp();
  app.applyLocalProjects([ROOT, OTHER]);
  render(
    <AppStateContext.Provider value={app}>
      <ScheduleView state={state} />
    </AppStateContext.Provider>,
  );
  const actions = () => requests.filter((r) => r.type !== 'list_schedules');
  return { state, requests, actions };
}

function dialog() {
  return screen.getByRole('dialog');
}

function field(name: string): HTMLInputElement | HTMLSelectElement {
  const element = dialog().querySelector<HTMLInputElement | HTMLSelectElement>(`[name="${name}"]`);
  if (element === null) throw new Error(`no field ${name}`);
  return element;
}

function type(name: string, value: string) {
  fireEvent.change(field(name), { target: { value } });
}

describe('nextRunPreview and buildTrigger', () => {
  it('should describe the next run like the Mac', () => {
    const now = new Date(2026, 10, 25, 10, 0);
    expect(nextRunPreview(new Date(2026, 10, 27, 8, 0), 'none', now, 'en-US')).toBe(
      'Runs once: Friday at 8:00 AM',
    );
    expect(nextRunPreview(new Date(2026, 10, 25, 16, 5), 'daily', now, 'en-US')).toBe(
      'Next run: Today at 4:05 PM',
    );
    expect(nextRunPreview(new Date(2026, 10, 26, 7, 0), 'weekly', now, 'en-US')).toBe(
      'Next run: Tomorrow at 7:00 AM',
    );
    expect(nextRunPreview(new Date(2026, 11, 1, 7, 0), 'weekly', now, 'en-US')).toBe(
      'Next run: Tuesday at 7:00 AM',
    );
  });

  it('should build each trigger payload', () => {
    expect(buildTrigger('agent', 'reviewer', 'p', 'claude')).toEqual({
      type: 'agent_def',
      name: 'reviewer',
    });
    expect(buildTrigger('swarm', 'team', 'p', 'claude')).toEqual({
      type: 'swarm_def',
      name: 'team',
      vars: {},
    });
    expect(buildTrigger('inlinePrompt', 'x', 'do it', 'terminal')).toEqual({
      type: 'inline_prompt',
      prompt: 'do it',
      agent: 'terminal',
    });
  });
});

describe('ScheduleEventSheet', () => {
  it('should open from New Schedule with the Mac defaults and require a name', () => {
    setup();
    fireEvent.click(screen.getByRole('button', { name: 'New Schedule' }));
    expect(dialog()).toHaveAccessibleName('New Schedule');
    expect(within(dialog()).getByRole('tab', { name: 'Agent' })).toHaveAttribute(
      'aria-selected',
      'true',
    );
    expect(field('definition')).toHaveAttribute('placeholder', 'Agent Definition Name');
    expect(field('repeats')).toHaveValue('none');
    expect(field('scope')).toHaveValue('global');
    expect(dialog().querySelector('[name="project"]')).toBeNull();
    expect(dialog().querySelector('[name="enabled"]')).toBeNull();
    expect(within(dialog()).getAllByRole('radio')).toHaveLength(8);
    expect(within(dialog()).getByText('Runs once: Wednesday at 10:00 AM')).toBeInTheDocument();
    const create = within(dialog()).getByRole('button', { name: 'Create' });
    expect(create).toBeDisabled();
    type('name', '   ');
    expect(create).toBeDisabled();
    type('name', 'nightly');
    expect(create).toBeEnabled();
    expect(within(dialog()).queryByRole('button', { name: 'Delete' })).toBeNull();
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('should create a local inline-prompt schedule from a time slot', async () => {
    const { actions, state } = setup();
    act(() => {
      state.handleTimeSlotClick(new Date(2026, 10, 27, 14, 15));
    });
    expect(field('time')).toHaveValue('2026-11-27T14:15');
    type('name', 'e2e-sheet');
    fireEvent.click(within(dialog()).getByRole('tab', { name: 'Inline Prompt' }));
    expect(field('agent')).toHaveValue('claude');
    type('prompt', 'echo hi');
    type('agent', 'terminal');
    type('repeats', 'weekly');
    type('target', 'src');
    type('scope', 'local');
    type('project', OTHER);
    fireEvent.click(within(dialog()).getByRole('radio', { name: 'green' }));
    expect(within(dialog()).getByText('Next run: Friday at 2:15 PM')).toBeInTheDocument();
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Create' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(actions()).toEqual([
      {
        type: 'save_schedule',
        project_root: OTHER,
        name: 'e2e-sheet',
        enabled: true,
        recurrence: 'weekly',
        start_at: formatIso8601(new Date(2026, 10, 27, 14, 15)),
        trigger: { type: 'inline_prompt', prompt: 'echo hi', agent: 'terminal' },
        target: 'src',
        scope: 'local',
      },
    ]);
    expect(localStorage.getItem('scheduleColor:e2e-sheet')).toBe('6');
  });

  it('should save a global schedule in the first project', async () => {
    const { actions } = setup();
    fireEvent.click(screen.getByRole('button', { name: 'New Schedule' }));
    type('name', 'global-one');
    type('definition', 'reviewer');
    fireEvent.submit(dialog());
    await waitFor(() => {
      expect(actions()).toHaveLength(1);
    });
    expect(actions()[0]).toMatchObject({
      type: 'save_schedule',
      project_root: ROOT,
      scope: 'global',
      trigger: { type: 'agent_def', name: 'reviewer' },
    });
  });

  it('should prefill an edit, keep root and agent name, and delete the old name on rename', async () => {
    const { actions } = setup([info()]);
    await waitFor(() => {
      expect(screen.getAllByTestId('schedule-event').length).toBeGreaterThan(0);
    });
    fireEvent.click(screen.getAllByTestId('schedule-event')[0] as HTMLElement);
    expect(dialog()).toHaveAccessibleName('Edit Schedule');
    expect(field('name')).toHaveValue('nightly');
    expect(field('prompt')).toHaveValue('echo hi');
    expect(field('agent')).toHaveValue('terminal');
    expect(field('repeats')).toHaveValue('daily');
    expect(field('target')).toHaveValue('src');
    expect(field('scope')).toHaveValue('local');
    expect(field('project')).toHaveValue(ROOT);
    expect(field('enabled')).toBeChecked();
    type('name', 'nightly-2');
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Save' }));
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(actions()).toEqual([
      { type: 'delete_schedule', project_root: ROOT, name: 'nightly', scope: 'local' },
      expect.objectContaining({
        type: 'save_schedule',
        name: 'nightly-2',
        root: false,
        agent_name: 'night-branch',
      }),
    ]);
  });

  it('should disable, re-enable from the list, and delete after confirming', async () => {
    const { actions } = setup([info()]);
    await waitFor(() => {
      expect(screen.getAllByTestId('schedule-event').length).toBeGreaterThan(0);
    });
    fireEvent.click(screen.getAllByTestId('schedule-event')[0] as HTMLElement);
    fireEvent.click(field('enabled'));
    await waitFor(() => {
      expect(field('enabled')).not.toBeChecked();
    });
    expect(actions()).toEqual([{ type: 'disable_schedule', project_root: ROOT, name: 'nightly' }]);
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Cancel' }));
    // Disabled schedules leave the calendar; the list keeps them in a Disabled section.
    expect(screen.queryAllByTestId('schedule-event')).toHaveLength(0);
    fireEvent.click(screen.getByRole('tab', { name: 'List' }));
    const disabled = screen.getByText('Disabled').closest('section');
    if (disabled === null) throw new Error('no Disabled section');
    fireEvent.click(within(disabled).getByTestId('schedule-event'));
    expect(field('enabled')).not.toBeChecked();
    fireEvent.click(field('enabled'));
    await waitFor(() => {
      expect(field('enabled')).toBeChecked();
    });
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Delete' }));
    const confirm = screen.getByRole('alertdialog', { name: 'Delete Schedule' });
    expect(confirm).toHaveTextContent('This schedule will be permanently deleted.');
    fireEvent.click(within(confirm).getByRole('button', { name: 'Cancel' }));
    expect(screen.queryByRole('alertdialog')).toBeNull();
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Delete' }));
    fireEvent.click(
      within(screen.getByRole('alertdialog')).getByRole('button', { name: 'Delete' }),
    );
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(actions().slice(1)).toEqual([
      { type: 'enable_schedule', project_root: ROOT, name: 'nightly' },
      { type: 'delete_schedule', project_root: ROOT, name: 'nightly', scope: 'local' },
    ]);
    expect(screen.queryAllByTestId('schedule-event')).toHaveLength(0);
  });

  it('should keep the sheet open and show the daemon error when a save fails', async () => {
    setup([], 'invalid schedule name');
    fireEvent.click(screen.getByRole('button', { name: 'New Schedule' }));
    type('name', 'bad name!');
    fireEvent.click(within(dialog()).getByRole('button', { name: 'Create' }));
    await waitFor(() => {
      expect(screen.getByRole('alert')).toHaveTextContent('invalid schedule name');
    });
    expect(dialog()).toBeInTheDocument();
  });
});
