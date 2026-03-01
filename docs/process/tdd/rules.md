# TDD Rules

These are the imperative instructions agents follow when doing TDD on PurePoint.

## Process

For each unit of code, create a test suite one requirement at a time:

1. **Clarify the stack** — If the test framework or technology stack is unspecified, ask before implementing.
2. **Propose the API** — If the calling API is unspecified, propose one that serves the functional requirements and creates an optimal developer experience.
3. **Write a failing test** — Write the test. Run the test runner. Watch it fail. If it passes, the test is wrong or the feature already exists.
4. **Make it pass** — Implement ONLY the code needed to make the test pass. No more.
5. **Verify** — Run the test runner. Fail → fix the bug. Pass → continue.
6. **Get approval (supervised mode)** — In supervised mode, get user approval before moving to the next requirement. In autonomous mode, proceed directly if the test passes and the requirement is met.
7. **Repeat** — Next functional requirement, next test.

## Assert Format

Use the structured assert function:

```
assert({
  given: "a new user with valid credentials",
  should: "create an account and return user ID",
  actual: result,
  expected: expectedValue
})
```

Rules:
- `given` and `should` must state functional requirements from an acceptance perspective
- Avoid describing literal values in given/should
- Every test must answer the 5 questions (see TDD/Philosophy)

## Describe/Test Wrappers

- Use `describe` string to name the unit under test
- Use `test` string for a brief category (e.g., "new account creation")
- See per-language files for language-specific wrapper conventions

## Constraints

- Always colocate tests with the code they test (unless directed otherwise)
- Carefully think through correct output — avoid hallucination
- Don't write tests for expected types/shapes — redundant with type checks
- Each test must demonstrate locality — no reliance on external state or other tests
- For integration tests, test with the real system (no mocks for the integration boundary)
