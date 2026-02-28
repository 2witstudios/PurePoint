# Storage

**Maturity: SEED**

## Context

PurePoint needs persistent, concurrent, queryable storage for project state — agents, worktrees, output history, events, and configuration. The storage layer must support concurrent reads from multiple clients (CLI, desktop app, daemon internals) while the daemon writes. It must also be portable per-project and simple to operate (no external database server). A leading approach is per-project databases at `{project_root}/.pu/pu.db` and a global daemon database at `~/.pu/daemon.db`, using SQLite with WAL mode.

## Open Questions

? [STORE-001] Should configuration also live in SQLite, or stay as files?
One approach is YAML files for templates, swarms, prompts, and schedules. Should PurePoint use SQLite tables instead, keep files, or go hybrid (files for user-editable content, SQLite for runtime state)? Files are easier to edit manually and version-control. SQLite is easier to query and manage atomically.

? [STORE-002] What is the schema evolution strategy?
As PurePoint evolves, the SQLite schema will change. Options: embedded migrations (refinery or sqlx migrations), manual versioning with a schema_version table, or use SQLite's flexible ALTER TABLE. How do we handle schema changes without data loss? What about backwards compatibility if a user downgrades?

? [STORE-003] How long should output be retained, and how should it be stored?
The `output_chunks` table stores raw terminal output. Agents can produce megabytes of output per session. Should output be: stored in SQLite BLOBs (simple but bloats DB), stored as files with SQLite references (more complex but keeps DB small), automatically pruned after N days, or compressed? What about summary generation — store summaries in addition to or instead of raw output?

? [STORE-004] What is the scope of the global daemon database?
`~/.pu/daemon.db` needs to track: known projects, daemon config, global schedules(?), cross-project state. How much goes here vs in per-project DBs? Should the daemon DB be the authority on project registration, or should it discover projects by scanning for `.pu/` directories?

? [STORE-005] Should we support undo/history for state changes?
An event log (the `events` table) provides a history of state transitions. Should this be used for undo operations? If a user accidentally kills an agent or merges a worktree, can they undo? Or is the event log purely for auditability and the dashboard's activity feed?

? [STORE-006] How should the daemon index spec content for retrieval?
The daemon needs to find relevant specs for context assembly. Options: full-text search over the `docs/` directory, tag-based lookup (specs declare their domains via frontmatter), directory-convention-based matching (path = domain, e.g., `docs/product/cli.md` matches CLI tasks), embedded vector search for semantic similarity, or a simple manifest file that maps domains to spec paths.

## Design Directions

- WAL mode for concurrent read access
- Graceful DB corruption handling (backup, rebuild)
- Transaction-based writes to prevent data loss on daemon crash
- Per-project DB portability (no absolute paths stored)
- Compatibility with SQLite's single-writer model

## Related

- [STORE-006] connects to [DAEMON-006] (context classification) and [AGENT-005]/[AGENT-006] (context injection) — the daemon classifies tasks, storage indexes specs, and agent execution handles injection.
