# Skills Manager — Skills Browser & Editor

## User Story

As a user, I want to browse, create, and edit Claude skills from within PurePoint so that I can manage my agent capabilities without manually editing YAML files.

## Feature Description

A skills browser and editor with a two-column layout (skill list + editor). Supports YAML frontmatter editing via form fields, reference file management, import/create/delete operations, and personal and project scope separation.

## How ppg-cli Did It

Two-column skills view with a list on the left and an editor on the right. YAML frontmatter fields exposed as form inputs (name, description, triggers). Reference file management for attaching context files to skills. Import from file, create new, and delete with confirmation. Personal and project scope tabs.

What worked well: Form-based frontmatter editing was more approachable than raw YAML. Reference file management made it easy to attach context. Scope separation kept personal experiments separate from team skills.

## PurePoint Opportunity

- **Skills as first-class daemon resources**: Daemon indexes and validates skills, making them queryable via IPC and CLI.
- **Validation on save**: Daemon validates skill frontmatter schema, checks for missing required fields, and warns about common issues.
- **Skill dependency graph**: Visualize which skills reference other skills or files, helping users understand their skill ecosystem.
- **Test-run capability**: Spawn a test agent with a specific skill loaded to verify it works before committing.
- **CLI access**: `pu skill list`, `pu skill edit <name>` for terminal workflows.

## Priority

**P3** — Skills management is a power-user feature. Most users start with a few skills and grow over time.
