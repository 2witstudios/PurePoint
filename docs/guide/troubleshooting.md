# Troubleshooting

Common issues and their solutions.

## `pu` command not found

The `pu` binary lives at `~/.pu/bin/pu`. Add it to your PATH:

```sh
export PATH="$HOME/.pu/bin:$PATH"
```

Add this line to `~/.zshrc` or `~/.bashrc` and reload your shell.

If the binary is missing, open the PurePoint macOS app. It installs the CLI on launch.

## Daemon won't start

Check the daemon log:

```sh
cat ~/.pu/daemon.log
```

Common causes:
- **Socket already exists**: A stale `~/.pu/daemon.sock` from a crashed daemon. Remove it: `rm ~/.pu/daemon.sock`
- **PID file conflict**: A stale `~/.pu/daemon.pid`. Remove it: `rm ~/.pu/daemon.pid`
- **Port/socket permissions**: Ensure `~/.pu/` is writable

## Agent stuck in "waiting"

An agent shows `waiting` when:
- A shell prompt is detected (`$ `, `% `, `# `, `> `)
- The agent has been idle for more than 30 seconds
- The agent is suspended

Check if it's suspended:

```sh
pu status --json | grep -A 5 "ag-{id}"
```

If `suspended` is `true`, resume it:

```sh
pu play ag-{id}
```

If the agent is genuinely idle, send it more work:

```sh
pu send ag-{id} "continue working on the task"
```

## Agent shows "broken"

The agent process has exited. Check the exit code and logs:

```sh
pu logs ag-{id} --tail 5000
pu status --agent ag-{id} --json
```

Common causes:
- Agent CLI tool not installed or not in PATH
- API key not set (check your `.env` files)
- Agent hit a fatal error

## Worktree conflicts

If `pu spawn` fails with a branch conflict, the branch `pu/{name}` already exists. Either:
- Use a different name: `pu spawn "task" --name different-name`
- Clean the conflicting worktree: `pu clean --worktree wt-{id}`
- Delete the stale branch: `git branch -D pu/{name}`

## macOS app doesn't see agents

The macOS app watches `.pu/manifest.json` for changes. If agents exist but don't appear:
1. Check that the daemon is running: `pu health`
2. Ensure you're looking at the right project
3. Try spawning a new agent from the command palette (`Cmd+N`)

## Schedule didn't fire

Schedules are evaluated every 30 seconds by the daemon. Check:
1. Schedule is enabled: `pu schedule show <name>`
2. Daemon is running: `pu health`
3. `start_at` or `next_run` is in the past
4. Check daemon log for errors: `cat ~/.pu/daemon.log`

## Trigger gate timeout

Gate commands have a 60-second per-command and 5-minute total timeout. If your tests take longer:
- Optimize the test suite
- Run a subset of tests in the gate
- Consider moving slow checks to `pre_push` instead of `pre_commit`

## Terminal rendering issues

PurePoint uses PTY-based terminal emulation. If output looks wrong:
- Ensure your terminal size is set correctly (the app auto-sizes)
- Some TUI applications may not render correctly in all pane sizes
- Try resizing the pane or detaching/reattaching

## Getting help

- File issues at [github.com/2witstudios/purepoint/issues](https://github.com/2witstudios/purepoint/issues)
- Check daemon logs: `~/.pu/daemon.log`
- Use `--json` flag for detailed machine-readable output
- Use `pu health --json` to verify daemon state
