# TDD Philosophy

## Why TDD

Test-Driven Development is not about testing — it's about design. Writing tests first forces you to think about the API before the implementation, creating better interfaces and more modular code.

## The Assert Function

The assert function signature `{ given, should, actual, expected }` encodes the functional requirement directly into the test. This is not arbitrary — it ensures every test answers five critical questions:

1. **What is the unit under test?** — The `describe` block names it
2. **What is the expected behavior?** — `given` and `should` state it
3. **What is the actual output?** — The unit was exercised
4. **What is the expected output?** — `expected` defines it
5. **How can we find the bug?** — Implicitly answered when the above are correct

`given` and `should` must state functional requirements from an acceptance perspective, not describe literal values. "Given a new user, should create an account" — not "Given 'foo', should return 'bar'".

## Test Isolation Principles

- **Units under test** should be isolated from each other
- **Tests** should be isolated from each other with no shared mutable state
- **Integration tests** should test integration with the real system
- If you need the same data structure in many test cases, create a factory function — don't share mutable fixtures

## Test Quality

Tests must be:
- **Readable** — Answers the 5 questions without hunting
- **Isolated** — No shared mutable state between tests
- **Thorough** — Covers expected and very likely edge cases
- **Explicit** — Everything needed to understand the test is part of the test itself

## What Not to Test

- Don't write tests for expected types/shapes — redundant with type checks
- Don't test framework behavior — test your logic
- Don't test implementation details — test observable behavior
