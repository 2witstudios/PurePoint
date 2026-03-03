# Project Picker & Onboarding — Project Picker & Onboarding Flow

## User Story

As a user, I want a project picker with recent projects and a guided onboarding flow so that I can quickly open existing projects and get new ones set up correctly.

## Feature Description

A dedicated project picker screen shown at launch (or when no project is open) with recent project list, quick-open actions, and a first-run onboarding flow. Includes dependency validation (CLI version, tmux availability, daemon health) with re-check capability and guided project scaffolding.

## How ppg-cli Did It

Dedicated picker screen with recent projects list, double-click to open, and a setup/dependency check screen that validated CLI and tmux versions. Re-check button for retrying after installing missing dependencies. Picker shown on launch and when closing a project.

What worked well: The dependency check screen caught setup issues before they became confusing runtime errors. Recent projects list eliminated repeated file-picker navigation. The picker as launch screen set clear expectations.

## PurePoint Opportunity

- **Welcome screen with recent projects**: Native macOS recent-documents integration plus custom recent-projects tracking in `.pu/`.
- **Daemon health check**: Verify daemon is running, correct version, and responsive as part of project open.
- **Guided first-run experience**: Walk new users through `pu init`, explain workspace concepts, and validate their environment.
- **Project scaffolding**: `pu init` creates `.pu/` structure, initial manifest, and optionally CLAUDE.md template.

## Priority

**P2** — First impressions matter. The current Cmd+O picker works but lacks polish and safety checks.
