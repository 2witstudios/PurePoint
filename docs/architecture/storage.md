# Storage

**Maturity: SEED**

## Context

PurePoint needs persistent, concurrent, queryable storage for project state — agents, worktrees, output history, events, and configuration. The storage layer must support concurrent reads from multiple clients (CLI, desktop app, daemon internals) while the daemon writes. It must also be portable per-project and simple to operate (no external database server).

## Open Questions

? [STORE-001] Should configuration live in the database, in files, or both?
Files are easier to edit manually and version-control. A database is easier to query and manage atomically. Hybrid approaches exist (files for user-editable content, database for runtime state).

? [STORE-002] What is the schema evolution strategy?
As PurePoint evolves, the storage schema will change. How do we handle schema changes without data loss? What about backwards compatibility if a user downgrades?

? [STORE-003] How long should output be retained, and how should it be stored?
Agents can produce megabytes of output per session. Should output be stored inline (simple but bloats storage), stored as files with references (more complex but keeps storage small), automatically pruned, or compressed?

? [STORE-004] What is the scope of global vs per-project storage?
Global storage needs to track known projects, global config, and cross-project state. How much goes in global vs per-project storage?

? [STORE-005] Should we support undo/history for state changes?
An event log provides a history of state transitions. Should this be used for undo operations, or purely for auditability?

? [STORE-006] How should the daemon index spec content for retrieval?
The daemon needs to find relevant specs for context assembly. Options: full-text search, tag-based lookup, directory-convention matching, semantic search, or a manifest file mapping domains to specs.

? [STORE-007] What storage technology should PurePoint use?
Options: embedded database (SQLite, DuckDB, etc.), flat files, external database, key-value store. Trade-offs: portability, concurrent access, query capability, operational simplicity.

## Design Directions

- Concurrent read access from multiple clients
- Graceful corruption handling (backup, rebuild)
- Transaction-based writes to prevent data loss on crash
- Per-project storage portability (no absolute paths stored)

## Related

- [STORE-006] connects to [DAEMON-006] (context classification) and [AGENT-005]/[AGENT-006] (context injection) — the daemon classifies tasks, storage indexes specs, and agent execution handles injection.
