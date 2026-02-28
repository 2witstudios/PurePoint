# Requirements Rules

Imperative instructions for writing functional requirements.

## Format

```
type FunctionalRequirement = "Given {situation}, should {job to do}"
```

Every requirement follows this format. No exceptions.

## ID Convention

Requirements get IDs: `REQ-{DOMAIN}-{NNN}`

Example: `REQ-CLI-001: Given a user running pu spawn without a daemon, should auto-start the daemon and complete the spawn`

## Writing Rules

1. **Start with the situation** — "Given" describes the precondition or context
2. **State the behavior** — "Should" describes what the system does (the job)
3. **Be specific** — "Given a user" is too vague. "Given a user with 3 active worktrees" is specific.
4. **Be testable** — If you can't write a test for it, rewrite it
5. **One behavior per requirement** — If you use "and", split it into two requirements

## Constraints

- Focus on functional requirements to support the user journey
- Avoid describing specific UI elements or interactions
- Focus on the job the user wants to accomplish and the benefits expected
- Requirements must trace to a user story or pain point
- Keep requirements atomic — one requirement, one behavior
- Number requirements sequentially within each domain
