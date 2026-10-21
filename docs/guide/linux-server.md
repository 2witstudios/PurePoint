# Run PurePoint on a Linux server

The PurePoint daemon (`pu-engine`) can run on any Linux machine you can reach over SSH: a cloud VM, a home server, a build box. It owns that machine's projects, worktrees and agents, and keeps running when you disconnect. You drive it from another computer with `pu --machine` or the PurePoint app's Machines section (see [Machines over SSH](../product/machines.md)), or with `pu` in an SSH session on the server.

## 1. Install

On the server:

```sh
curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | sh
```

This puts static `pu` and `pu-engine` binaries (x86_64 or aarch64) in `~/.pu/bin`. Set `PU_VERSION=1.2.3` to pin a release. Before installing, the script checks the release signature against the PurePoint key it embeds (using `minisign`, or OpenSSL 3, which most distributions ship), and, if the GitHub CLI is installed and signed in, the build provenance with `gh attestation verify`. If it can't verify the download it stops; set `PU_INSTALL_SKIP_VERIFY=1` to install anyway (it warns). A failed check always stops the install. Add `~/.pu/bin` to your `PATH` in `~/.profile`, or call it by full path.

To pair a phone or iPad directly with this server, also install phone access:

```sh
curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | PU_WITH_PHONE=1 sh
pu phone install --now
```

Phone access adds the Point Guard bridge, with its own copy of Node.js, in `~/.pu/pointguard/<version>/`. It needs glibc 2.28 or newer, so it doesn't run on Alpine or other musl systems. On those hosts the installer says so, and `pu` and `pu-engine` still install and work.

The payload is verified like `pu` itself: its SHA-256, then the release signature (with `minisign` or OpenSSL 3) or its build provenance (with a signed-in `gh`). Installation stops if a check fails or none can run, unless you set `PU_INSTALL_SKIP_VERIFY=1`. An existing version directory is never modified. Set `PU_PHONE_ARCHIVE` to install a local payload tarball.

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

## 5. Pair a phone or iPad (optional)

With phone access installed (step 1), the phone talks to this server directly over your tailnet. No Mac is involved. Both devices need [Tailscale](https://tailscale.com) connected.

```sh
pu phone install --now        # systemd user service pu-pointguard.service
pu phone login                # list providers; then: pu phone login <provider>
pu phone restart              # apply the sign-in
pu phone model                # list models; then: pu phone model <provider>/<model>
pu phone pair                 # show a one-time QR code in this terminal
```

In the PurePoint app, open **Connections → Pair a computer → Scan pairing code**. If the terminal can't show the QR (it's too narrow, or output isn't a terminal), or you pass `--text`, `pu phone pair` prints a `purepoint-pair:` code instead. Copy it to the phone and use **Paste code**. The code works once and expires within 2 minutes (`--ttl`, at most 300 s). Leaving `pu phone pair` early in any way (Ctrl-C, closing the SSH session) revokes it.

A new device can't do anything until you grant it access:

```sh
pu phone devices                                   # names, grants, chat level
pu phone grant iPhone --terminals view             # watch agents and read Point Guard chat
pu phone grant iPhone --terminals control --workspace on   # full control, including chat
pu phone rename iPhone "Work phone"
pu phone revoke "Work phone"                       # closes its connections now
```

Point Guard chat can run `pu` on this server, so sending chat needs the broadest grant (terminals Control plus workspace management). Machines this server reaches with `pu machine` appear on the phone under the same grants.

How it stays private:

- The phone listener binds only this host's Tailscale address, confirmed by both `tailscale ip -4` and the network interfaces. It never binds a public or LAN address. Without Tailscale it doesn't listen at all, and it starts by itself once Tailscale comes up.
- In userspace-networking mode (containers, some VMs), it listens on loopback and needs `tailscale serve --bg --tcp 8787 tcp://127.0.0.1:8787`. That is TCP passthrough, so the phone still checks the pinned certificate end to end. `pu phone status` prints the exact command when one is needed.
- Every connection must present the host certificate pinned in the QR code and a per-device credential. Every message is checked against that device's current grant.
- The owner channel is a Unix socket in a private `0700` directory under `~/.local/state/purepoint/point-guard/`, and it still needs a token. `pu` refuses to use it unless the directory, socket and token are private to you. There is no admin TCP port.
- The service runs with an explicit environment (`NODE_OPTIONS`, `LD_PRELOAD` and similar inherited variables are removed), `UMask=0077` and `NoNewPrivileges=yes`.

On hosts without systemd, run `pu phone run` under your own supervisor instead.

## Upgrading

Re-run the installer (with `PU_WITH_PHONE=1` if you use phone access), then restart the daemon so it uses the new binary. Restarting suspends running agents, and they resume when the daemon comes back:

```sh
pu daemon stop && pu daemon start
pu phone restart   # phone access: waits until Point Guard is idle, then moves to the new version
```

Client and server must speak the same protocol version. `pu daemon status` on the server shows the daemon's, and `pu machine status` on a client reports a mismatch as `incompatible`. Upgrade PurePoint on both sides together.

## Troubleshooting

- **`no usable pu-engine`**: `pu` looks beside itself, then in `~/.pu/bin`. Keep both binaries together (`install.sh`, `cargo install` and the macOS app all do). The error lists why each candidate was refused. To use another build, set `PU_ENGINE_PATH` to its path. The binary must be owned by you (or root) and not writable by group or others.
- **Agents can't find `claude`/`codex`**: the daemon takes agent environments from your login shell (`$SHELL -li`). Make sure your `PATH` is exported from `~/.profile` (or `~/.bash_profile`), not only from `~/.bashrc`.
- **`systemctl --user` fails with "Failed to connect to bus"**: you're in a session without a user manager (e.g. `su`). Log in over SSH directly as that user.
- **`pu phone` says the payload is for another pu version**: phone access runs only with the exact `pu` it was released with. Re-run the installer with `PU_WITH_PHONE=1`, then `pu phone restart`.
- **`pu phone pair` says there is no tailnet address**: run `tailscale up` (and the `tailscale serve` command from `pu phone status` in userspace mode). The listener binds on its own within a minute.
- **Point Guard stops when you log out**: enable lingering with `sudo loginctl enable-linger $USER`.
- **Logs**: `journalctl --user -u pu-pointguard`. They never contain credentials, pairing codes, prompts or terminal content.
