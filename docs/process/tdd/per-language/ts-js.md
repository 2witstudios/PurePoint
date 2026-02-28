# TDD — TS/JS

TypeScript/JavaScript TDD conventions for any TS/JS tooling in the PurePoint ecosystem.

## Test Framework

- **Vitest** as the test runner
- **Riteway** library for structured assertions
- Run: `npx vitest` or `npm test`

## Assert Format

```typescript
import { describe, test } from 'vitest';
import { assert } from 'riteway';

describe('createAccount', () => {
  test('new account creation', async () => {
    const result = await createAccount({ email: 'user@test.com' });
    
    assert({
      given: 'valid credentials',
      should: 'create an account and return user ID',
      actual: typeof result.id,
      expected: 'string'
    });
  });
});
```

## Test Utilities

- **Spies and stubs**: `vi.fn()` and `vi.spyOn()` (tinyspy under the hood)
- **Module mocking**: `vi.mock()` with `vi.importActual()` for partial mocks (ESM-friendly, avoid require)
- **Timers**: `vi.useFakeTimers()` and `vi.setSystemTime()`

## Conventions

- Colocate tests with source: `agent.ts` → `agent.test.ts` in same directory
- Use `describe` for unit under test name, `test` for category
- Avoid the `it` wrapper (conflicts with assert style)
- When testing state logic, always use selectors — never read directly from state objects
- Don't test types/shapes — redundant with TypeScript type checking
