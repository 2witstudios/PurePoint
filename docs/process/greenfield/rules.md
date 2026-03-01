# Greenfield Rules

Imperative instructions for creating new modules or components in PurePoint.

## Before You Start

1. **Read the Architecture page** for the relevant domain — understand constraints and decisions
2. **Read the Product page** for the relevant domain — understand what it needs to do
3. **Check Module Structure** (Architecture) — understand module boundaries and dependency rules

## New Module Checklist

1. Create the module in the appropriate location per the project's module structure
2. Define the public interface — what types and functions are exported
3. Write the first test before the first implementation
4. Verify the module compiles/builds independently
5. Wire the module into its parent and any dependents

## Conventions

- Follow existing patterns in the codebase for naming, error handling, and structure
- Core domain logic must not depend on transport or API layers
- No circular dependencies between modules
- Follow TDD — write the test first, then the implementation

## Constraints

- New modules must build independently
- Follow the project's established dependency direction (core → transport → client)
- Colocate tests with the code they test
- Document public interfaces
