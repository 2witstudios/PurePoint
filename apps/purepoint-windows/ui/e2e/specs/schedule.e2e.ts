// WIN-8.8: schedules created with `pu.exe schedule create` appear in the Schedule screen's week,
// day, month and list views.
//
// Both schedules start tomorrow (so neither can fire during the run) and target a `terminal`
// agent with an inline prompt, so nothing calls a real AI even if one did.

import { $, $$, expect } from '@wdio/globals';
import { DaemonFixture } from '../fixtures/daemon.ts';

const WEEKLY = 'e2e-weekly';
const ONCE = 'e2e-once';

function pad(n: number): string {
  return String(n).padStart(2, '0');
}

function dayKey(date: Date): string {
  return `${String(date.getFullYear())}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

const tomorrow = new Date();
tomorrow.setDate(tomorrow.getDate() + 1);
const TOMORROW = dayKey(tomorrow);

/** Creates a local, root, inline-prompt schedule for a `terminal` agent at tomorrow `time`. */
async function createSchedule(
  fixture: DaemonFixture,
  name: string,
  recurrence: string,
  time: string,
): Promise<void> {
  await fixture.pu([
    'schedule',
    'create',
    name,
    '--recurrence',
    recurrence,
    '--start-at',
    `${TOMORROW}T${time}`,
    '--trigger',
    'inline-prompt',
    '--trigger-prompt',
    'echo purepoint-e2e',
    '--agent',
    'terminal',
    '--root',
  ]);
}

function byText(text: string, tag = '*') {
  return $(`//${tag}[normalize-space()="${text}"]`);
}

function eventOn(name: string, date: string) {
  return $(`[data-testid="schedule-event"][data-name="${name}"][data-date="${date}"]`);
}

async function selectMode(label: string): Promise<void> {
  await $(`//button[@role="tab" and normalize-space()="${label}"]`).click();
  await expect($(`//button[@role="tab" and normalize-space()="${label}"]`)).toHaveAttribute(
    'aria-selected',
    'true',
  );
}

/** Clicks Next until `selector` exists (at most `max` times). */
async function forwardUntil(selector: string, max = 1): Promise<void> {
  for (let i = 0; i < max && !(await $(selector).isExisting()); i++) {
    await $('[aria-label="Next"]').click();
  }
}

describe('Schedule views', () => {
  const fixture = DaemonFixture.current();

  before(async () => {
    await createSchedule(fixture, WEEKLY, 'weekly', '10:00:00');
    await createSchedule(fixture, ONCE, 'none', '14:00:00');
  });

  it('opens the Schedule screen on the week view', async () => {
    await byText('Schedule', 'button').click();
    await expect($('[data-view="ScheduleView"]')).toBeDisplayed();
    await expect($('[data-view="WeekCalendarView"]')).toBeDisplayed();
    await expect(byText('New Schedule', 'span')).toBeDisplayed();
  });

  it('shows the schedules in the week view', async () => {
    // Tomorrow is next week when today is the last day of the week.
    await forwardUntil(`.schedule-week-day[data-date="${TOMORROW}"]`);
    await expect(eventOn(WEEKLY, TOMORROW)).toBeExisting();
    await expect(eventOn(ONCE, TOMORROW)).toBeExisting();
    await expect(eventOn(WEEKLY, TOMORROW)).toHaveText(expect.stringContaining('Weekly'));
  });

  it('shows the schedules in the day view', async () => {
    await selectMode('Day');
    await $('//button[normalize-space()="Today"]').click();
    await expect($('[data-view="DayCalendarView"]')).toBeDisplayed();
    await $('[aria-label="Next"]').click();
    await expect(eventOn(WEEKLY, TOMORROW)).toBeExisting();
    await expect(eventOn(ONCE, TOMORROW)).toBeExisting();
    expect(await $$('[data-testid="schedule-event"]').length).toBe(2);
  });

  it('shows the schedules in the month view', async () => {
    await selectMode('Month');
    await $('//button[normalize-space()="Today"]').click();
    await expect($('[data-view="MonthCalendarView"]')).toBeDisplayed();
    // The 6-week grid always reaches tomorrow.
    await expect(eventOn(WEEKLY, TOMORROW)).toBeDisplayed();
    await expect(eventOn(ONCE, TOMORROW)).toBeDisplayed();
  });

  it('shows the schedules in the list view', async () => {
    await selectMode('List');
    await $('//button[normalize-space()="Today"]').click();
    await expect($('[data-view="ScheduleListView"]')).toBeDisplayed();
    await expect(byText('Tomorrow')).toBeDisplayed();
    await expect(eventOn(WEEKLY, TOMORROW)).toBeDisplayed();
    await expect(eventOn(ONCE, TOMORROW)).toBeDisplayed();
    // The weekly one repeats within the 30 days; the one-off does not.
    expect(await $$(`[data-testid="schedule-event"][data-name="${WEEKLY}"]`).length).toBe(5);
    expect(await $$(`[data-testid="schedule-event"][data-name="${ONCE}"]`).length).toBe(1);
  });
});
