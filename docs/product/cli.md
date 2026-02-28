# CLI

**Maturity: SEED** | ID Prefix: CLI

## Purpose

The `pu` command-line tool. A thin Rust client that serializes arguments into gRPC requests, sends them to the daemon, and formats responses for terminal output. Zero domain logic.

## Conceptual Model

```
User types: pu {command} [args] [--flags]
  CLI parses args (clap)
  CLI connects to daemon (Unix socket or TCP)
  CLI sends gRPC request
  Daemon processes and responds
  CLI formats response for terminal
  CLI exits with appropriate code
```

Special behaviors:
- Auto-starts daemon if not running
- `--json` flag for machine-readable output (conductor agents)
- `pu attach` execs into tmux (replaces CLI process)
