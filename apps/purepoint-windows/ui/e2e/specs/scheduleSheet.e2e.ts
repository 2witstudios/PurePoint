// WIN-8.17: create a schedule from a week-view time slot with the event sheet, see it in
// `pu.exe schedule list --json`, then disable and delete it from the sheet.
//
// The schedule starts tomorrow and targets a `terminal` agent with an inline prompt, so nothing
// calls a real AI even if it fired.

import { browser, $, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

const NAME = 'e2e-sheet';

interface ListedSchedule {
  name: string;
  enabled: boolean;
  recurrence: string;
  scope: string;
  project_root: string;
  start_at: string;
  trigger: { type: string; prompt?: string; agent?: string };
}

function pad(n: number): string {
  return String(n).padStart(2, '0');
}

function dayKey(date: Date): string {
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

const tomorrow = new Date();
tomorrow.setDate(tomorrow.getDate() + 1);
const TOMORROW = dayKey(tomorrow);
/** The `HH:MM` the clicked slot gave the new schedule. */
let startTime = '';

async function listSchedules(fixture: DaemonFixture): Promise<ListedSchedule[]> {
  const { stdout } = await fixture.pu(['schedule', 'list', '--json']);
  const parsed = JSON.parse(stdout) as ListedSchedule[] | { schedules: ListedSchedule[] };
  return Array.isArray(parsed) ? parsed : parsed.schedules;
}

async function listed(fixture: DaemonFixture): Promise<ListedSchedule | undefined> {
  return (await listSchedules(fixture)).find((schedule) => schedule.name === NAME);
}

function sheetField(name: string) {
  return $(`[data-view="ScheduleEventSheet"] [name="${name}"]`);
}

/** Replaces a controlled input's text the way a user does (select all, then type). */
async function typeInto(name: string, text: string): Promise<void> {
  const input = sheetField(name);
  await input.click();
  await browser.keys(['Control', 'a']);
  await browser.keys('Backspace');
  await input.addValue(text);
  await expect(input).toHaveValue(text);
}

function sheetButton(label: string) {
  return $(`//form[@data-view="ScheduleEventSheet"]//button[normalize-space()="${label}"]`);
}

describe('Schedule event sheet', () => {
  const fixture = DaemonFixture.current();

  it('opens the week view', async () => {
    await $('//button[normalize-space()="Schedule"]').click();
    await expect($('[data-view="WeekCalendarView"]')).toBeDisplayed();
    // Tomorrow is next week when today is the last day of the week.
    if (!(await $(`.schedule-week-day[data-date="${TOMORROW}"]`).isExisting())) {
      await $('[aria-label="Next"]').click();
    }
    await expect($(`.schedule-week-day[data-date="${TOMORROW}"]`)).toBeExisting();
  });

  it('creates a schedule from a time slot', async () => {
    const slot = $(`[data-slot="${TOMORROW}T11"]`);
    await slot.scrollIntoView();
    await slot.click();
    await expect($('[data-view="ScheduleEventSheet"]')).toBeDisplayed();
    await expect($('[data-view="ScheduleEventSheet"]')).toHaveAttribute(
      'aria-label',
      'New Schedule',
    );
    // WebDriver clicks the middle of the slot's visible part: some quarter of 11 AM.
    const time = await sheetField('time').getValue();
    expect(time).toMatch(new RegExp(`^${TOMORROW}T11:(00|15|30|45)$`));
    startTime = time.slice(-5);
    await expect(sheetButton('Create')).toBeDisabled();

    await typeInto('name', NAME);
    await sheetButton('Inline Prompt').click();
    await typeInto('prompt', 'echo purepoint-e2e');
    await typeInto('agent', 'terminal');
    await sheetField('repeats').selectByAttribute('value', 'weekly');
    await sheetField('scope').selectByAttribute('value', 'local');
    await expect(sheetField('project')).toHaveValue(fixture.projectRoot);
    await $('[data-view="ScheduleEventSheet"] [role="radio"][aria-label="green"]').click();
    await expect($('.schedule-sheet-preview')).toHaveText(
      expect.stringContaining(
        `Next run: Tomorrow at ${String(Number(startTime.slice(0, 2)))}:${startTime.slice(3)}`,
      ),
    );
    await sheetButton('Create').click();
    await expect($('[data-view="ScheduleEventSheet"]')).not.toBeExisting();
    await expect(
      $(`[data-testid="schedule-event"][data-name="${NAME}"][data-date="${TOMORROW}"]`),
    ).toBeExisting();
  });

  it('lists the new schedule in pu schedule list --json', async () => {
    const schedule = await listed(fixture);
    expect(schedule).toBeDefined();
    expect(schedule?.enabled).toBe(true);
    expect(schedule?.recurrence).toBe('weekly');
    expect(schedule?.scope).toBe('local');
    expect(schedule?.trigger).toEqual({
      type: 'inline_prompt',
      prompt: 'echo purepoint-e2e',
      agent: 'terminal',
    });
    const start = new Date(schedule?.start_at ?? '');
    expect(dayKey(start)).toBe(TOMORROW);
    expect(`${pad(start.getHours())}:${pad(start.getMinutes())}`).toBe(startTime);
  });

  it('disables the schedule from the sheet', async () => {
    await $(`[data-testid="schedule-event"][data-name="${NAME}"][data-date="${TOMORROW}"]`).click();
    await expect($('[data-view="ScheduleEventSheet"]')).toHaveAttribute(
      'aria-label',
      'Edit Schedule',
    );
    await expect(sheetField('name')).toHaveValue(NAME);
    await expect(sheetField('agent')).toHaveValue('terminal');
    const enabled = sheetField('enabled');
    await expect(enabled).toBeSelected();
    await enabled.click();
    await expect(enabled).not.toBeSelected();
    await browser.waitUntil(async () => (await listed(fixture))?.enabled === false, {
      timeout: 15_000,
      timeoutMsg: 'pu schedule list still shows the schedule enabled',
    });
  });

  it('deletes the schedule after confirming', async () => {
    await sheetButton('Delete').click();
    const confirm = $('[role="alertdialog"][aria-label="Delete Schedule"]');
    await expect(confirm).toBeDisplayed();
    await confirm.$('.//button[normalize-space()="Delete"]').click();
    await expect($('[data-view="ScheduleEventSheet"]')).not.toBeExisting();
    await browser.waitUntil(async () => (await listed(fixture)) === undefined, {
      timeout: 15_000,
      timeoutMsg: 'pu schedule list still shows the deleted schedule',
    });
  });
});
