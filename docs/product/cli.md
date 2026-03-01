# CLI

**Maturity: SEED** | ID Prefix: CLI | Dependencies: `architecture/ipc-api.md`

## Purpose

The `pu` command-line tool. A thin client that sends requests to the daemon and formats responses for terminal output. Zero domain logic — all state and operations live in the daemon.

## Conceptual Model

```
User types: pu {command} [args] [--flags]
  CLI parses args
  CLI connects to daemon
  CLI sends request
  Daemon processes and responds
  CLI formats response for terminal
  CLI exits with appropriate code
```

Key behaviors:
- Auto-starts daemon if not running
- Machine-readable output mode for conductor agents
- Ability to attach directly to an agent's terminal session

## Open Questions

? [CLI-001] How should the CLI handle daemon auto-start failures — retry, suggest manual start, or exit with an actionable error?

? [CLI-002] Should machine-readable output follow a standard format (JSON-RPC, JSON Lines) or a custom schema?
