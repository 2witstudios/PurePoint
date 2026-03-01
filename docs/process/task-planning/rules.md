# Task Planning Rules

Imperative instructions for planning and executing task epics.

## Task Decomposition

1. **Decompose** — Break the request into atomic, sequential tasks
2. **Assess** — For each task, determine if specialized agent orchestration is needed
3. **Order** — Arrange by dependencies and logical flow
4. **Validate** — Ensure each task is: specific, actionable, independently testable, completable in one session, clear about inputs/outputs/success criteria
5. **Checkpoint** — Add approval gates between major phases

## Epic Template

```
# {Epic Name} Epic

**Status**: PLANNED
**Goal**: {brief goal}

## Overview

{Single paragraph starting with WHY — user benefit or problem being solved}

---

## {Task Name}

{Brief task description — 1 sentence max}

**Requirements**:
- Given {situation}, should {job to do}
- Given {situation}, should {job to do}

---
```

## Epic Constraints

- Overview starts with WHY (user benefit/problem being solved)
- No task numbering — use task names only
- Requirements use ONLY "Given X, should Y" format
- Include ONLY novel, meaningful, insightful requirements
- No extra sections, explanations, or text beyond the template

## Execution Protocol

1. Complete only the current task
2. Validate — verify the task meets its success criteria
3. Report — summarize what was accomplished
4. Await approval — get explicit user approval before proceeding

## On Completion

1. Update epic status to COMPLETED with date
2. Archive the epic file
3. Remove from active plan

## Execution Modes

### Supervised Mode (default)

Human-in-the-loop execution. Use when:
- Spec maturity is below SPECIFIED
- Working on novel or ambiguous requirements
- Single-agent sessions with direct human oversight

Rules:
- Complete only the current task before proceeding
- Get explicit user approval between tasks
- If a task reveals new information, pause and re-plan
- If blocked or uncertain, ask rather than assume

### Autonomous Mode

Agent operates without human checkpoints. Use when:
- Spec maturity is SPECIFIED (all requirements, interfaces, and edge cases defined)
- Operating within a swarm or parallel agent execution
- A conductor agent is coordinating the work

Rules:
- Follow the spec's Given/should requirements as success criteria
- Validate against rubrics (TDD rubric, code review rubric, design rubric) — these are the quality gates
- On completion, report results to the conductor or commit directly
- On failure or ambiguity, stop and report — don't guess

## Constraints

- Tasks should be independent — completing one shouldn't break others
- Avoid breaking changes unless explicitly requested (open/closed principle)
- Task quality is bounded by rubrics and success criteria, not line count
