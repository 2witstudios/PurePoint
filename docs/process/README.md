# Process

How we work on PurePoint. Each process domain has up to three layers:

- **Philosophy** — WHY we do it this way. Principles and rationale.
- **Rules** — WHAT to do. Imperative instructions agents follow.
- **Rubric** — HOW to evaluate quality. Scoring criteria for review.

## Domains

| Domain | Philosophy | Rules | Rubric |
|---|---|---|---|
| TDD | Test isolation, 5 questions, assert format rationale | TDD process, assert format, constraints | Test quality scoring |
| Code Review | Simplicity, security, requirements fidelity | 9-step review process, OWASP checklist | Code quality scoring |
| Greenfield | Interface-first, single responsibility, conventions | Crate/module checklists, naming, dependencies | - |
| Product Discovery | Pain-driven priority, jobs not UI, continuous discovery | Discovery process, user story format, PRD structure | - |
| Task Planning | One thing at a time, requirements before code | Epic template, decomposition, execution protocol | - |
| Requirements | Behavior not implementation, testable by definition | Given/should format, ID conventions, writing rules | - |

## Per-Language TDD

TDD rules are supplemented with language-specific pages:
- **Rust** — cargo test, #[cfg(test)], trait-based DI, tempdir, tokio::test
- **Swift** — XCTest, SwiftTerm testing, gRPC client testing
- **TS/JS** — Vitest + Riteway, vi.fn/vi.mock, colocated tests
