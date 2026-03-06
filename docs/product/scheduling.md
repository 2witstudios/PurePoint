# Scheduling

**Maturity: SPECIFIED** | ID Prefix: SCHED | Dependencies: daemon-engine, agent-execution

## Purpose

Scheduled agent execution: recurring or time-based task spawning. A nightly security review, a weekly dependency audit — results waiting when you check in.

## Conceptual Model

```
Schedule: named recurring task (timing, trigger, enabled/disabled)
ScheduleDef: YAML file in .pu/schedules/ (local) or ~/.pu/schedules/ (global)
```

## Decisions

! [SCHED-001] Log and continue, no retry — rationale: matches agent execution model where broken agents stay broken; user can re-enable or manually re-trigger

! [SCHED-002] No dependencies between tasks — rationale: YAGNI; adds complexity for a feature no one has asked for

! [SCHED-003] Part of the daemon — rationale: daemon already has tokio runtime, broadcast channels, and direct access to spawn/swarm logic; separate process would require its own IPC

! [SCHED-004] YAML files in `.pu/schedules/` (local) and `~/.pu/schedules/` (global) — rationale: follows established template/agent/swarm pattern exactly

! [SCHED-005] All three: agent def, swarm def, inline prompt — rationale: user requested all three; matches existing spawn flexibility

! [SCHED-006] Engine tracks registered projects (populated by Init and any project-scoped request); scheduler scans those — rationale: daemon already knows about projects; no separate discovery needed

! [SCHED-007] Named presets stored in YAML, no raw cron expressions — rationale: UI uses presets (none/hourly/daily/weekdays/weekly/monthly); cron strings add complexity without value since we control both ends

! [SCHED-008] Yes, one-shot schedules auto-disable — rationale: prevents re-firing after daemon restart; clean lifecycle

## Requirements

### Storage

- `REQ-SCHED-001` Given a schedule def YAML file in `.pu/schedules/`, should deserialize into ScheduleDef struct
- `REQ-SCHED-002` Given local and global schedule dirs with same-named schedule, should prefer local
- `REQ-SCHED-003` Given a new schedule, should save as YAML to the scope-appropriate directory
- `REQ-SCHED-004` Given a schedule name with invalid characters, should reject with validation error
- `REQ-SCHED-005` Given a delete request for existing schedule, should remove YAML file and return true
- `REQ-SCHED-006` Given a delete request for nonexistent schedule, should return false

### Recurrence

- `REQ-SCHED-010` Given recurrence `none` and base time in the future, should return base as next occurrence
- `REQ-SCHED-011` Given recurrence `none` and base time in the past, should return None (one-shot expired)
- `REQ-SCHED-012` Given recurrence `hourly`, should compute next hour matching base minute
- `REQ-SCHED-013` Given recurrence `daily`, should compute next day at base time
- `REQ-SCHED-014` Given recurrence `weekdays` on Friday, should skip to Monday
- `REQ-SCHED-015` Given recurrence `weekdays` on Saturday, should skip to Monday
- `REQ-SCHED-016` Given recurrence `weekly`, should return same weekday next week
- `REQ-SCHED-017` Given recurrence `monthly` on 31st, should skip months with fewer days
- `REQ-SCHED-018` Given recurrence `monthly`, should return same day-of-month at base time

### Protocol

- `REQ-SCHED-020` Given ListSchedules request, should return all schedules for project as ScheduleList response
- `REQ-SCHED-021` Given GetSchedule request with valid name, should return ScheduleDetail
- `REQ-SCHED-022` Given GetSchedule request with unknown name, should return NOT_FOUND error
- `REQ-SCHED-023` Given SaveSchedule request, should persist schedule and return Ok
- `REQ-SCHED-024` Given DeleteSchedule request, should remove schedule and return Ok
- `REQ-SCHED-025` Given EnableSchedule request, should set enabled=true, compute next_run, save
- `REQ-SCHED-026` Given DisableSchedule request, should set enabled=false, clear next_run, save

### Scheduler

- `REQ-SCHED-030` Given enabled schedule with next_run in the past, should fire the trigger
- `REQ-SCHED-031` Given disabled schedule, should not fire regardless of next_run
- `REQ-SCHED-032` Given schedule with AgentDef trigger, should resolve agent def and spawn agent
- `REQ-SCHED-033` Given schedule with SwarmDef trigger, should call RunSwarm
- `REQ-SCHED-034` Given schedule with InlinePrompt trigger, should spawn with prompt text
- `REQ-SCHED-035` Given one-shot schedule after firing, should auto-disable
- `REQ-SCHED-036` Given recurring schedule after firing, should compute and persist next next_run

### CLI

- `REQ-SCHED-040` Given `pu schedule list`, should display all schedules in tabular format
- `REQ-SCHED-041` Given `pu schedule create` with agent-def trigger, should send SaveSchedule request
- `REQ-SCHED-042` Given `pu schedule show <name>`, should display schedule details
- `REQ-SCHED-043` Given `pu schedule delete <name>`, should remove schedule
- `REQ-SCHED-044` Given `pu schedule enable <name>`, should enable schedule
- `REQ-SCHED-045` Given `pu schedule disable <name>`, should disable schedule

### Swift UI

- `REQ-SCHED-050` Given ScheduleState initialized, should load schedules from daemon (no mock data)
- `REQ-SCHED-051` Given schedule creation sheet submitted, should send SaveSchedule to daemon
- `REQ-SCHED-052` Given calendar view displayed, should show daemon-sourced events

## Interfaces

```
ScheduleDef:
  name: String
  enabled: bool (default true)
  recurrence: Recurrence (none|hourly|daily|weekdays|weekly|monthly)
  start_at: DateTime<Utc>
  next_run: Option<DateTime<Utc>>
  trigger: ScheduleTrigger (agent_def|swarm_def|inline_prompt)
  project_root: String
  target: String (path scope, default "")
  scope: String (local|global, skip serialization)
  created_at: DateTime<Utc>

ScheduleTrigger (tagged enum):
  AgentDef { name: String }
  SwarmDef { name: String, vars: HashMap<String,String> }
  InlinePrompt { prompt: String, agent: String }

IPC Requests: ListSchedules, GetSchedule, SaveSchedule, DeleteSchedule, EnableSchedule, DisableSchedule
IPC Responses: ScheduleList, ScheduleDetail (+ existing Ok, Error)

CLI: pu schedule {list|create|show|delete|enable|disable}

Storage: .pu/schedules/{name}.yaml (local), ~/.pu/schedules/{name}.yaml (global)
```

## Edge Cases

- Schedule YAML with unknown fields: serde should ignore (allow forward compat)
- Monthly recurrence on day 31 in February: skip to next month with 31 days (March)
- Daemon restart with schedules whose next_run is in the past: fire immediately on first tick, then advance
- Multiple schedules fire in same tick: fire all, no ordering guarantee
- Schedule references deleted agent def: fire fails, schedule stays enabled, error logged
- Schedule with empty project_root: reject at save time
