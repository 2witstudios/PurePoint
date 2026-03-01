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

| Domain | Philosophy | Rules | Rubric |
|---|---|---|---|
| TDD | Yes | Yes | Yes |
| Code Review | Yes | Yes | Yes |
| Greenfield | Yes | Yes | — |
| Product Discovery | Yes | Yes | — |
| Task Planning | Yes | Yes | — |
| Requirements | Yes | Yes | — |
| Spec Advancement | — | Yes | — |

TDD rules have per-language supplements: `per-language/{rust|swift|ts-js}.md`

### Architecture (ADR)
Technical decisions about how PurePoint is built. Each page follows the ADR template:
- **Context** — Why this decision matters
- **Open Questions** — Unresolved design questions (`? [DOMAIN-NNN]`)
- **Decisions** — Resolved questions (`! [DOMAIN-NNN]`)
- **Design Directions** — Candidate approaches and trade-offs
- **Research Notes** — Findings, benchmarks, prototypes

| Domain | ID Prefix | Maturity |
|---|---|---|
| Daemon Engine | DAEMON | SEED |
| IPC & API | IPC | SEED |
| Storage | STORE | SEED |
| Agent Execution | AGENT | SEED |
| Desktop App Integration | DESK | SEED |
| Distribution | DIST | SEED |
| Module Structure | MOD | SEED |

### Product (Domain Spec)
What PurePoint does. Each domain captures behavior, requirements, and interfaces:
- **Purpose** — What this domain is responsible for
- **Conceptual Model** — Key abstractions and relationships
- **Open Questions** — Unresolved design questions
- **Requirements** — Functional requirements (Given X, should Y)
- **Sum Sheet** — Concise prop-logic summary
- **Interfaces** — API surface, data structures, config
- **Edge Cases** — Known edge cases and how to handle them

| Domain | ID Prefix | Maturity |
|---|---|---|
| Data Model | DM | SEED |
| CLI | CLI | SEED |
| Daemon | DMN | SEED |
| Agent Lifecycle | AL | SEED |
| Worktree Management | WT | SEED |
| Orchestration | ORCH | SEED |
| Scheduling | SCHED | SEED |
| Output & Streaming | OUT | SEED |
| Memory System | MEM | SEED |
| Recovery & Resilience | REC | SEED |
| Desktop App | APP | SEED |
| Configuration | CFG | SEED |

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

### Minimum Content per Maturity Level

| Level | Required Sections |
|---|---|
| SEED | Purpose, Conceptual Model, at least 2 Open Questions |
| EXPLORING | All SEED content + Research Notes with findings for each open question |
| CONVERGING | All EXPLORING content + each question narrowed to 1-2 options with trade-offs |
| DECIDED | All CONVERGING content + all `?` converted to `!` with rationale |
| SPECIFIED | All DECIDED content + Requirements (Given/should), Interfaces, Edge Cases |

Use this checklist to verify a spec is ready to advance to the next level. Specs missing required sections for their current level should be backfilled before advancing.

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
2. Read `docs/product/{relevant domain}.md` for what to build — check the Dependencies field in the header for required architecture pages
3. Read the architecture pages listed in Dependencies
4. Read `docs/reference/` pages for existing implementation context
5. If the spec page maturity is SEED or EXPLORING, STOP — the spec needs research before implementation. See `docs/process/spec-advancement/rules.md` to advance it.

To find domain dependencies and cross-references:
6. Read `docs/product/cross-reference-matrix.md` — maps every domain to its architecture pages, API operations, stored data, CLI commands, and desktop views

Before doing TDD:
7. Read `docs/process/tdd/rules.md` for TDD process
8. Read `docs/process/tdd/per-language/{rust|swift|ts-js}.md` for language-specific conventions

Before code review:
9. Read `docs/process/code-review/rules.md` for review process
10. Read `docs/process/code-review/rubric.md` for quality scoring

Before advancing a spec:
11. Read `docs/process/spec-advancement/rules.md`

Before product discovery:
12. Read `docs/process/product-discovery/rules.md`

Before greenfield module creation:
13. Read `docs/process/greenfield/rules.md`

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

## Agent Communication Protocol (Parallel Execution)

When multiple agents work in parallel (swarms, worktrees), they must coordinate to avoid conflicts.

### Rules

1. **One agent per spec** — never assign two agents to advance or modify the same spec file concurrently
2. **Report, don't write** — during parallel execution, agents report findings to the conductor rather than writing directly to shared spec files. The conductor aggregates and applies changes.
3. **Structured findings** — agents report using this format:

```markdown
## Finding: {brief title}
- **Agent**: {agent ID or worktree name}
- **Spec**: {target spec file path}
- **Type**: research-note | new-question | decision-proposal | edge-case
- **Content**: {the actual finding}
```

4. **Divide by question, not by file** — when multiple agents research the same domain, the conductor assigns specific open questions to each agent, not entire files
5. **Dependency awareness** — before finalizing work, check whether specs you depend on are also being modified by another agent. If so, coordinate through the conductor before committing.

### Conductor Responsibilities

The conductor (human or orchestrating agent) must:
- Assign non-overlapping work units to each agent
- Aggregate findings and resolve conflicts before writing to specs
- Gate spec advancements on consistency with dependent specs
- Merge worktree branches in dependency order

## Future: Daemon Context Assembly

NOTE: The manual reading protocol above is temporary. PurePoint's daemon will handle context assembly automatically once implemented. The daemon will:
1. Classify incoming tasks to identify relevant domains
2. Pull specs from `docs/` + runtime memory from storage
3. Format and inject context per agent type
4. Agents receive context — they never navigate for it

See `docs/architecture/daemon-engine.md` [DAEMON-006], `docs/architecture/agent-execution.md` [AGENT-005, AGENT-006], and `docs/architecture/storage.md` [STORE-006] for the open questions on this design.
