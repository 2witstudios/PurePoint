# Prompt Library — Prompt & Template Library

## User Story

As a user, I want a library of saved prompts and templates that I can browse, edit, and use when spawning agents so that I can reuse proven prompts and maintain consistency across agent tasks.

## Feature Description

A prompt browser and editor with support for template variables (`{{VAR}}`), project and global scopes, and markdown editing. Users can create, edit, categorize, and select prompts when launching agents. Templates support variable detection and binding at spawn time.

## How ppg-cli Did It

Two-column prompt browser with a list on the left and a markdown editor on the right. Template support with automatic `{{VAR}}` detection and extraction. Project-scoped prompts stored alongside the project, global prompts in user config. Prompts selectable when spawning new agents.

What worked well: The two-column layout was consistent with other editors (swarms, skills). Variable detection from templates automated parameterization. Having both project and global scopes covered team-shared and personal prompts.

## PurePoint Opportunity

- **Daemon-managed prompt resolution**: Prompts stored in `.pu/prompts/` with daemon indexing for fast search and retrieval.
- **Template rendering at spawn time**: Daemon resolves `{{VAR}}` bindings when spawning agents, supporting environment variables, project metadata, and user-provided values.
- **Version-controlled prompt history**: Since prompts live in `.pu/`, they're naturally version-controlled. Daemon can track which prompt version was used for each agent run.
- **CLI access**: `pu prompt list`, `pu prompt use <name>` for terminal-based workflows.

## Priority

**P2** — Valuable for power users and teams. Agents can be spawned with inline prompts in the interim.
