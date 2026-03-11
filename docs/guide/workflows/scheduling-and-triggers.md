# Scheduling and Triggers

Automate agent workflows with scheduled tasks and event-driven triggers.

## Schedules

Schedules spawn agents or swarms on a recurring cadence.

### Creating a schedule

```sh
# One-shot: run once at a specific time
pu schedule create morning-check \
  --start-at "2026-03-09T08:00:00" \
  --root \
  --trigger inline-prompt \
  --trigger-prompt "Check for overnight issues"

# Recurring: run daily at 3am using an agent def
pu schedule create nightly-review \
  --start-at "2026-03-09T03:00:00" \
  --recurrence daily \
  --root \
  --trigger agent-def \
  --trigger-name security-review

# Weekly swarm run
pu schedule create weekly-audit \
  --start-at "2026-03-10T09:00:00" \
  --recurrence weekly \
  --root \
  --trigger swarm-def \
  --trigger-name full-audit
```

### Recurrence types

| Type | Behavior |
|---|---|
| `none` | One-shot; fires once, then auto-disables |
| `hourly` | Every hour at the same minute |
| `daily` | Every day at the same time |
| `weekdays` | Monday through Friday only |
| `weekly` | Same day each week |
| `monthly` | Same day each month (skips months where day doesn't exist) |

### Trigger types

| Type | Flag | Description |
|---|---|---|
| `agent-def` | `--trigger-name <name>` | Resolve and spawn an agent def |
| `swarm-def` | `--trigger-name <name>` | Spawn a swarm with optional `--var` |
| `inline-prompt` | `--trigger-prompt <text>` | Spawn with raw prompt text (uses `--agent` type, default: claude) |

### Spawn modes

By default, scheduled agents spawn into a worktree. Use `--root` for project-root agents:

```sh
# Worktree spawn (needs --name for the branch)
pu schedule create overnight-build \
  --start-at "2026-03-09T22:30:00" \
  --name overnight-build \
  --trigger inline-prompt \
  --trigger-prompt "build a feature"

# Root spawn (for read-only tasks)
pu schedule create morning-status \
  --start-at "2026-03-09T08:00:00" \
  --root \
  --trigger inline-prompt \
  --trigger-prompt "scan recent commits"
```

### Managing schedules

```sh
pu schedule list                  # list all schedules
pu schedule show nightly-review   # show details
pu schedule enable nightly-review
pu schedule disable nightly-review
pu schedule delete nightly-review
```

### How it works

The daemon runs a scheduler loop every 30 seconds. When `next_run` is in the past, the schedule fires: the trigger is resolved and an agent/swarm is spawned. One-shot schedules auto-disable after firing. Recurring schedules advance to the next occurrence.

## Triggers

Triggers are event-driven automations. When an event fires, a sequence of actions executes in order.

### Events

| Event | When it fires |
|---|---|
| `agent_idle` | Agent finishes work and awaits input (status changes to `waiting`) |
| `pre_commit` | Git pre-commit hook runs in an agent's worktree |
| `pre_push` | Git pre-push hook runs in an agent's worktree |

### Actions

Each trigger has an ordered **sequence** of actions. Each action can:

- **Inject** text into the agent's terminal
- Run a **gate** command (must pass before proceeding)
- Combine both (gate runs first, inject follows on success)

### Creating triggers

```sh
# Simple: inject commands when agent goes idle
pu trigger create post-task \
  --on agent_idle \
  --inject "/simplify" \
  --inject "/review" \
  --inject "/commit-push-pr"

# Quality gate: block commits unless tests pass
pu trigger create quality-gate \
  --on pre_commit \
  --gate "cargo test" \
  --gate "cargo clippy -- -D warnings"
```

### Trigger def format

For complex triggers, write YAML directly to `.pu/triggers/` or `~/.pu/triggers/`:

```yaml
name: gated-flow
description: Review code, gate on tests, then commit
on: agent_idle
sequence:
  - inject: "/simplify"
  - inject: "/review"
    gate:
      run: "cargo test"
      expect_exit: 0
    max_retries: 3
  - inject: "/commit-push-pr"
variables:
  BRANCH: main
```

### Sequence execution

For each action in order:

1. **Gate phase** (if present): Run the shell command, check exit code matches `expect_exit` (default: 0). Retry up to `max_retries` times on failure.
2. **Inject phase** (if present): Send the text to the agent's terminal with a newline.
3. Advance to the next action.

If a gate fails after all retries, the sequence stops with state `failed`. On each failure before exhausting retries, a failure message is injected into the agent's terminal showing the exit code and output.

### Gate timeouts

- Per-command: 60 seconds
- Total evaluation: 5 minutes

### Trigger state lifecycle

| State | Description |
|---|---|
| `active` | Sequence in progress |
| `gating` | Evaluating a gate command |
| `completed` | All actions executed successfully |
| `failed` | Stopped due to gate or inject failure |

### Managing triggers

```sh
pu trigger list
pu trigger show post-task
pu trigger delete post-task --scope local
pu trigger assign <agent-id> <trigger-name>  # bind trigger to existing agent
```

### Git hook integration

PurePoint automatically installs git hooks in agent worktrees. These hooks call `pu gate` to evaluate `pre_commit` and `pre_push` triggers:

```sh
# Manually evaluate (usually called by git hooks)
pu gate pre-commit
pu gate pre-push
```

### Variable substitution

Triggers support `{{VAR}}` placeholders in gate commands:

```yaml
variables:
  SCHEME: "MyApp"
sequence:
  - gate:
      run: "xcodebuild test -scheme {{SCHEME}} -quiet"
```

### Disabling triggers per agent

Spawn an agent with `--no-trigger` to skip auto-triggers:

```sh
pu spawn "quick fix" --no-trigger --name hotfix
```
