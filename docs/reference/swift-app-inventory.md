# Swift App Inventory

Source map of the macOS desktop app (`PPG CLI/PPG CLI/`).

## App Lifecycle

| File | Purpose |
|---|---|
| AppDelegate.swift | App lifecycle, window management, menu bar |
| main.swift | App entry point |

## Core Services

| File | Purpose |
|---|---|
| PPGService.swift | Bridge to CLI — shells out for mutations (spawn, kill, merge, etc.) |
| DashboardSession.swift | Dashboard session management, direct tmux calls, state persistence |
| ShellUtils.swift | Shell command execution utilities |
| AppSettingsManager.swift | User preferences and settings persistence |

## Views — Dashboard

| File | Purpose |
|---|---|
| HomeDashboardView.swift | Home dashboard with commit heatmap, agent status cards, recent commits |
| ContentTabViewController.swift | Tab-based content area (home, terminal, grid, swarms, prompts, schedules, config) |
| DashboardSplitViewController.swift | NSSplitViewController for sidebar + content layout |

## Views — Sidebar

| File | Purpose |
|---|---|
| SidebarViewController.swift | NSOutlineView sidebar tree, manifest watching, context menus |
| ProjectPickerViewController.swift | Multi-project selector (Cmd+1-9) |

## Views — Terminal

| File | Purpose |
|---|---|
| TerminalPane.swift | SwiftTerm terminal pane wrapper |
| ScrollableTerminalView.swift | Scrollable terminal container |
| PaneGridController.swift | Pane grid system (recursive binary split, up to 6 panes, draggable dividers) |

## Views — Editors

| File | Purpose |
|---|---|
| SwarmsView.swift | Swarm CRUD editor (.pu/swarms/*.yaml) |
| PromptsView.swift | Prompt/template editor with syntax highlighting |
| SchedulesView.swift | Schedules calendar view (day/week/month) |
| AgentConfigView.swift | Agent configuration editor |
| ClaudeMdEditorView.swift | CLAUDE.md file editor |
| SkillsView.swift | Skills browser and editor |
| PpgAgentsView.swift | Agent management view |

## Views — UI Components

| File | Purpose |
|---|---|
| CommandPalettePanel.swift | Command palette with fuzzy search |
| CommitHeatmapView.swift | Git commit activity heatmap visualization |
| SyntaxHighlighter.swift | Code syntax highlighting for editors |

## Configuration

| File | Purpose |
|---|---|
| SettingsViewController.swift | Settings UI (refresh interval, terminal font, shell path, appearance) |
| SetupViewController.swift | First-run setup flow |
| KeybindingManager.swift | Keyboard shortcut management |
| UpdaterManager.swift | Sparkle auto-update integration |

## Models

| File | Purpose |
|---|---|
| Models.swift | Data models (mirrors manifest.ts types) |
| AgentVariant.swift | Agent type definitions (claude, codex, opencode, terminal) |
| Theme.swift | Appearance theme definitions |
| CronParser.swift | Cron expression parsing |

## Total: 32 Swift files
