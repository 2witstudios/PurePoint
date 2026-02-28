# Requirements Philosophy

## Why Formal Requirements

Requirements are the contract between what the user needs and what the code does. Without explicit requirements, developers guess, testers don't know what to verify, and reviewers can't assess completeness.

## Principles

### Behavior, Not Implementation
Requirements describe WHAT the system should do, not HOW. "Given a user with expired credentials, should prompt for re-authentication" — not "Given an expired JWT, should redirect to /login".

### Jobs, Not UI
Focus on the job the user wants to accomplish and the benefit they expect — not on specific UI elements or interactions. The UI is an implementation detail that may change; the job persists.

### Testable by Definition
Every requirement in "Given X, should Y" format is inherently testable. The "given" is the setup, the "should" is the assertion. If you can't test it, it's not a requirement — it's a wish.

### Linked to Pain
Every requirement traces back to a user story, which traces back to a pain point. If a requirement doesn't reduce pain, question whether it's needed.
