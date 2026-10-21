# Run PurePoint on a Linux server

The PurePoint daemon (`pu-engine`) can run on any Linux machine you can reach over SSH: a cloud VM, a home server, a build box. It owns that machine's projects, worktrees and agents, and keeps running when you disconnect. You drive it from another computer with `pu --machine` or the PurePoint app's Machines section (see [Machines over SSH](../product/machines.md)), or with `pu` in an SSH session on the server.

## 1. Install

On the server:

```sh
curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | sh
```

This puts static `pu` and `pu-engine` binaries (x86_64 or aarch64) in `~/.pu/bin`. Set `PU_VERSION=1.2.3` to pin a release. Add `~/.pu/bin` to your `PATH` in `~/.profile`, or call it by full path.

Also install `git` and the agent CLIs you use (`claude`, `codex`, …) as that same user, and sign in to them.

To build from source instead, run `cargo install --path crates/pu-cli` and `cargo install --path crates/pu-engine`.

## 2. Keep the daemon running

```sh
pu daemon install --now
sudo loginctl enable-linger "$USER"
```

`pu daemon install` writes a systemd user unit (`~/.config/systemd/user/pu-engine.service`) and enables it. It starts the daemon only with `--now`; without it, run `pu daemon start` or wait for the next boot or login. It also tells you if lingering is off. Without lingering, systemd stops your user services when your last SSH session ends and doesn't start them at boot.

| Command | What it does |
|---|---|
| `pu daemon status` | Daemon health, plus the service state |
| `pu daemon start` / `stop` | Start or stop the daemon; through systemd when the unit is installed |
| `pu daemon uninstall` | Disable and remove the unit |
| `journalctl --user -u pu-engine` | Daemon logs when it runs under systemd |

You can also skip systemd. Any `pu` command starts the daemon on demand, detached from your shell (in its own session, ignoring SIGHUP), so it survives you logging out. It won't restart after a crash or reboot, though. If logind is configured with `KillUserProcesses=yes`, only a lingering systemd unit survives logout.

## 3. Register projects

```sh
cd ~/src/my-repo && pu init
```

Each project is registered with the server's daemon, so every client of that daemon sees it.

## 4. Connect from your computer

On your Mac (or any machine with `pu` and SSH access to the server):

```sh
pu machine add server me@server.example.com   # or an ~/.ssh/config alias
pu machine status server
pu --machine server --project ~/src/my-repo status
```

The client forwards its connection over SSH to `~/.pu/daemon.sock` on the server, using your own `~/.ssh/config`, keys and agent. It never starts the remote daemon on its own; if the daemon is stopped, run `pu machine start server`. In the PurePoint app, add the same machine from the sidebar footer; its projects then appear under MACHINES.

## Upgrading

Re-run the installer, then restart the daemon so it uses the new binary. Restarting suspends running agents, and they resume when the daemon comes back:

```sh
pu daemon stop && pu daemon start
```

Client and server must speak the same protocol version. `pu daemon status` on the server shows the daemon's, and `pu machine status` on a client reports a mismatch as `incompatible`. Upgrade PurePoint on both sides together.

## Troubleshooting

- **`no usable pu-engine`**: `pu` looks beside itself, then in `~/.pu/bin`. Keep both binaries together (`install.sh` and `cargo install` both do). To use another build, set `PU_ENGINE_PATH` to its path. The binary must be owned by you (or root) and not writable by group or others.
- **Agents can't find `claude`/`codex`**: the daemon takes agent environments from your login shell (`$SHELL -li`). Make sure your `PATH` is exported from `~/.profile` (or `~/.bash_profile`), not only from `~/.bashrc`.
- **`systemctl --user` fails with "Failed to connect to bus"**: you're in a session without a user manager (e.g. `su`). Log in over SSH directly as that user.
