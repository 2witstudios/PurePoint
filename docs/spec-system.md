# PurePoint Spec System

This is the master document for the PurePoint knowledge system. Read this first before doing any work on PurePoint.

## Specs Live in `docs/`

All PurePoint knowledge lives in `docs/` as markdown files, git-versioned alongside the code. One source of truth.

The daemon will assemble context automatically in the future — for now, agents read relevant files directly.

## Page Taxonomy

### Process (Philosophy / Rules / Rubric)
How we work. Each process domain has three layers:
- **Philosophy** — WHY we do it this way. Principles and rationale.
- **Rules** — WHAT to do. Imperative instructions agents follow.
- **Rubric** — HOW to evaluate quality. Scoring criteria for review.

Current process domains: TDD, Code Review, Greenfield, Product Discovery, Task Planning, Requirements

### Architecture (ADR)
Technical decisions about how PurePoint is built. Each page follows the ADR template:
- Context, Open Questions, Decisions, Design Directions, Research Notes

Architecture domains: Daemon Engine, IPC & API, Storage, Agent Execution, Desktop App Integration, Distribution, Module Structure

### Product (Domain Spec)
What PurePoint does. Each domain captures behavior, requirements, and interfaces:
- Purpose, Conceptual Model, Open Questions, Requirements, Sum Sheet, Interfaces, Edge Cases

Product domains: Data Model, CLI, Daemon, Agent Lifecycle, Worktree Management, Orchestration, Scheduling, Output & Streaming, Memory System, Recovery & Resilience, Desktop App, Configuration

### Reference (Inventory)
Reference material for existing codebases and PurePoint CLI commands.

### Design
UI/UX philosophy, rules, and rubric for PurePoint.

## Maturity Levels

Every Architecture and Product page has a maturity level:

| Level | Meaning | Gate to next |
|---|---|---|
| SEED | Open questions planted, no research yet | First research note added |
| EXPLORING | Actively researching, collecting options | Key questions have candidate answers |
| CONVERGING | Down to 1-2 options per question | Team/author picks a direction |
| DECIDED | All questions resolved with ! decisions | Spec writing begins |
| SPECIFIED | Full requirements, sum sheets, interfaces | Ready for implementation |

## Conventions

### Open Questions
Format: `? [DOMAIN-NNN] Question text`
Example: `? [DAEMON-001] Should we use launchd or PID-file for process supervision?`

### Decisions
Format: `! [DOMAIN-NNN] Decision text — rationale`
Example: `! [DAEMON-001] Use PID-file — simpler, cross-platform, no plist maintenance`

When a question is answered, replace the `?` with `!` and append the rationale.

### Numbering
- Architecture: `[DAEMON-NNN]`, `[IPC-NNN]`, `[STORE-NNN]`, `[AGENT-NNN]`, `[DESK-NNN]`, `[DIST-NNN]`, `[MOD-NNN]`
- Product: `[DM-NNN]`, `[CLI-NNN]`, `[DMN-NNN]`, `[AL-NNN]`, `[WT-NNN]`, `[ORCH-NNN]`, `[SCHED-NNN]`, `[OUT-NNN]`, `[MEM-NNN]`, `[REC-NNN]`, `[APP-NNN]`, `[CFG-NNN]`

### Requirements Format
Functional requirements use: `Given {situation}, should {job to do}`
Each requirement gets an ID: `REQ-{DOMAIN}-{NNN}`

## Agent Reading Protocol

Before starting ANY work on PurePoint:
1. Read this page (Spec System) to learn conventions — first time only

Before implementing code:
2. Read `docs/product/{relevant domain}.md` for what to build
3. Read any `docs/architecture/` pages linked as dependencies
4. Read `docs/reference/` pages for existing implementation context
5. If the spec page maturity is SEED or EXPLORING, STOP — the spec needs research before implementation

Before doing TDD:
6. Read `docs/process/tdd/rules.md` for TDD process
7. Read `docs/process/tdd/per-language/{rust|swift|ts-js}.md` for language-specific conventions

Before code review:
8. Read `docs/process/code-review/rules.md` for review process
9. Read `docs/process/code-review/rubric.md` for quality scoring

Before product discovery:
10. Read `docs/process/product-discovery/rules.md`

Before greenfield module creation:
11. Read `docs/process/greenfield/rules.md`

## Agent Writing Protocol

### Write directly to spec files:
- Research findings for a specific open question — append to that page's Research Notes
- New open questions discovered during implementation — add to the relevant page
- Edge cases found during testing — add to Product/{domain} Edge Cases
- Decision rationale — convert ? to ! on the Architecture page
- Process improvements — write to relevant Process/ page directly

### Keep LOCAL (do not write to specs):
- Session-specific context (current task, in-progress work)
- Git state (branches, commits, worktrees)
- Build artifacts, dependencies, lock files
- Debugging notes that are transient

### Decision Tree
When an agent discovers new information during work:
1. Is it specific to a known open question? → Write to that page's Research Notes
2. Is it a new question about architecture/product? → Add as new ? [DOMAIN-NNN]
3. Is it a process improvement or convention? → Write to relevant Process/ page
4. Is it transient/session-specific? → Keep local

## Future: Daemon Context Assembly

NOTE: The manual reading protocol above is temporary. PurePoint's daemon will handle context assembly automatically once implemented. The daemon will:
1. Classify incoming tasks to identify relevant domains
2. Pull specs from `docs/` + runtime memory from SQLite
3. Format and inject context per agent type
4. Agents receive context — they never navigate for it

See `docs/architecture/daemon-engine.md` [DAEMON-006], `docs/architecture/agent-execution.md` [AGENT-005, AGENT-006], and `docs/architecture/storage.md` [STORE-006] for the open questions on this design.
