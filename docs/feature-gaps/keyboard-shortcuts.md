# Keyboard Shortcuts — Customizable Keyboard Shortcuts

## User Story

As a user, I want to customize keyboard shortcuts so that PurePoint fits my muscle memory and doesn't conflict with other tools I use.

## Feature Description

A keyboard shortcut editor where users can view all available actions, record custom key bindings, detect and resolve conflicts, and reset to defaults. Shortcuts persist across sessions and can be exported/imported.

## How ppg-cli Did It

Full keybinding manager with a list of all actions and their current bindings. Per-action key recording (click, press keys, confirm). Conflict detection that warns when a new binding overlaps an existing one. Reset-all and per-binding reset options. Persisted to UserDefaults.

What worked well: Conflict detection prevented broken shortcuts. Per-action recording was intuitive. Reset options provided a safety net for experimentation.

## PurePoint Opportunity

- **SwiftUI KeyboardShortcut customization**: Build on SwiftUI's keyboard shortcut system with user-configurable bindings.
- **User-facing shortcut editor**: Settings panel section with searchable action list, recording UI, and conflict detection.
- **Import/export keybinding profiles**: Share keybinding configurations across machines or with team members via JSON/TOML files.
- **Context-aware shortcuts**: Different bindings for different contexts (sidebar focused, terminal focused, editor focused).

## Priority

**P2** — Power users expect customizable shortcuts. The current hardcoded shortcuts work but can't be changed.
