# CLAUDE.md Editor — CLAUDE.md & Rules Editor

## User Story

As a user, I want to view and edit my CLAUDE.md files and Claude rules from within PurePoint so that I can manage agent instructions without switching to an external editor.

## Feature Description

A tabbed editor for CLAUDE.md files at project and user scope, plus a browser for `.claude/rules/*.md` files. Includes a file switcher dropdown, monospace text editing, and save functionality. Provides a centralized place to manage all agent instruction files.

## How ppg-cli Did It

Tabbed editor with tabs for project CLAUDE.md, user-level CLAUDE.md, and a `.claude/rules/` browser. File switcher dropdown for selecting which rules file to edit. Monospace text area with save button. Changes written directly to the filesystem.

What worked well: Having all instruction files in one editor reduced context switching. The tabbed layout made scope (project vs user) explicit. Direct filesystem writes meant changes took effect immediately for new agent sessions.

## PurePoint Opportunity

- **Structured rules editor**: Beyond raw text editing, offer a structured view with sections, headers, and collapsible blocks for large CLAUDE.md files.
- **Preview mode**: Render CLAUDE.md as markdown to verify formatting before save.
- **Daemon-assisted context assembly**: Show which rules/instructions will actually be loaded for a given agent, resolving the layered config (project + user + rules directory).
- **Lint and validate**: Check for common issues (conflicting instructions, overly long files, broken references) before save.

## Priority

**P2** — Important for users who iterate on agent instructions frequently. External editors work but break flow.
