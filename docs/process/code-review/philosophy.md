# Code Review Philosophy

## Why We Review

Code review is not about finding bugs — it's about maintaining the quality, consistency, and simplicity of the codebase. Every line of code is a liability. Every abstraction is a commitment.

## Core Principles

### Simplicity Above All
Simplicity is removing the obvious and adding the meaningful. Perfection is attained not when there is nothing more to add, but when there is nothing more to remove.

Look for: redundancies, forgotten files, things that should have been moved or deleted that were not. Dead code, unused exports, stray `.d.ts` files, abandoned utilities.

### Security is Non-Negotiable
Every review must explicitly consider the OWASP Top 10. This is not a checkbox — it's a deliberate scan. See Code Review Rules for the specific checklist.

For secret/token comparisons: use timing-safe comparison. For authentication: prefer opaque tokens over JWT.

### Requirements Fidelity
The completed work must satisfy the functional requirements. Compare finished code against the requirements, the plan, and the user stories. If something was requested but not built, flag it. If something was built but not requested, question it.

## Review Mindset

- **Don't make changes** — review only. Output serves as input for planning.
- **Avoid unfounded assumptions** — if unsure, note it and ask.
- **Show your work** — make the reasoning visible so the author can learn from it.
- **Be specific** — "this function is too complex" is useless. "This function handles both validation and persistence — split it" is actionable.
