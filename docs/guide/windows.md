# PurePoint on Windows

PurePoint runs natively on Windows 11 (22H2 or later, x64 and arm64). You get the same daemon (`pu-engine`) and CLI (`pu`) as on macOS and Linux, plus a desktop app with the macOS app's features. Agents run in Windows terminals (ConPTY), and there is no WSL dependency. For the design and requirements, see [`docs/product/windows-app.md`](../product/windows-app.md).

## Install

**Desktop app.** Download `PurePoint_<version>_x64-setup.exe` from [Releases](https://github.com/2witstudios/purepoint/releases) and run it. It installs for your user only and needs no administrator rights. If WebView2 is missing, the installer installs it. On first launch the app:

- copies `pu.exe` and `pu-engine.exe` to `%USERPROFILE%\.pu\bin` and adds that folder to your user `PATH` (open a new terminal to pick it up);
- installs the PurePoint plugin for Claude Code;
- starts the daemon and supervises it while the app is open.

Builds that aren't code-signed yet make SmartScreen show "Windows protected your PC". Choose **More info**, then **Run anyway**.

**CLI only.** From PowerShell:

```powershell
irm https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.ps1 | iex
```

The script downloads the release archive and verifies its signature. It then removes the downloaded-from-the-internet mark from the binaries and installs them to `%USERPROFILE%\.pu\bin`. Set `PU_VERSION` to pin a release. The script doesn't change `PATH`: it prints the command to add the folder, or does it when you save the script and run it with `-AddToPath`.

**From source.** You need Rust 1.89 (MSVC; `rust-toolchain.toml` selects it), Node 22+, Git for Windows and the Visual Studio C++ build tools.

```powershell
git clone https://github.com/2witstudios/purepoint.git
cd purepoint
cargo build --release -p pu-cli -p pu-engine
cd apps\purepoint-windows
npm ci
npm run tauri dev          # run the app against your build
node scripts/build-release.mjs   # build the installer (see the app README)
```

## Keep the daemon running

Any `pu` command starts the daemon if it isn't running, detached from your terminal. The desktop app also starts it. To start it at every sign-in:

```powershell
pu daemon install --now     # adds an HKCU Run value "PurePoint Engine"; no admin needed
pu daemon status            # daemon health and the autostart state
pu daemon uninstall         # removes the Run value
```

Windows has no `launchd` or `systemd`. Autostart uses your user's Run key, and agents run in your own interactive session.

## Agents on Windows

- **Claude Code, Codex, OpenCode.** Install them as you normally would: the native installers, or npm (which gives `.cmd` shims such as `codex.cmd`). PurePoint finds them through `PATH` and `PATHEXT`. A forwarding shim is launched by running its target `.exe` directly, so Ctrl-C reaches the agent.
- **Shell.** `pu spawn --agent terminal` and Point Guard's home shell use `pwsh` if it's installed, otherwise Windows PowerShell, otherwise `cmd`.
- **Environment.** Agents get the daemon's environment merged with your current user and system `PATH` from the registry, so tools you installed after signing in are found without restarting. `NO_COLOR` is removed, because agents' TUIs need colour to work.
- **Stopping.** `pu kill` and `pu bench` stop the whole process tree, including grandchildren, through a Job Object.

## Remote machines

`pu machine add <name> <host>` and the app's **Machines** section reach other computers over SSH with Windows' built-in OpenSSH client (`ssh.exe`). Each daemon connection runs `pu daemon proxy` on the remote host. Linux, macOS and Windows hosts all work, and the remote host must have PurePoint installed. Your `~\.ssh\config`, keys and agent are used as they are. Set `PU_SSH` to point at a different `ssh.exe`.

## Phone pairing

With [Tailscale](https://tailscale.com) installed and connected:

```powershell
pu phone install --now
pu phone pair          # shows a one-time QR code
```

Point Guard listens only on your tailnet address. The commands are the same as in [the Linux server guide](linux-server.md#5-pair-a-phone-or-ipad-optional).

## Where things live

| What | Where |
|---|---|
| CLI and daemon | `%USERPROFILE%\.pu\bin\pu.exe`, `pu-engine.exe` |
| Daemon state, logs, registry of projects | `%USERPROFILE%\.pu\` (`daemon.log`, `daemon.sock.pid`, …) |
| Daemon IPC | a private named pipe `\\.\pipe\purepoint-<hash>-daemon` (only your user can open it) |
| Project data | `<project>\.pu\` (manifest, worktrees, `workspaces.json`), the same as on macOS |
| Point Guard state | `%LOCALAPPDATA%\PurePoint\point-guard\` |
| Secrets (pairing) | Windows Credential Manager, service `PurePoint.PointGuard.Pairing` |

## Troubleshooting

- **`pu` is not recognised.** Open a new terminal after the first app launch, or add `%USERPROFILE%\.pu\bin` to your user `PATH`.
- **"refusing … served by another user / pid …".** Another process is holding the daemon's pipe name. Run `pu daemon stop`, end any stray `pu-engine.exe` in Task Manager, then try again.
- **Running `pu` from an administrator terminal** works with the normal daemon. Clients refuse a daemon that is below Medium integrity, or that runs in an AppContainer or with a restricted token.
- **A worktree won't delete: "… is locked by another process".** An editor, a file watcher, antivirus or an agent that is still exiting has a file open. Close it and run `pu clean` again. PurePoint retries briefly, then names the locked file.
- **Slow builds inside worktrees.** Microsoft Defender scans every file Cargo or npm writes. Adding a Defender exclusion for your projects' `.pu\worktrees` folders (and `target\`) speeds up agent builds a lot. Only do this for folders you trust.
- **Long paths.** PurePoint passes `core.longpaths=true` to git for files inside worktrees. A worktree name long enough to push git's own folder path past 260 characters still fails, so keep worktree names short.
- **Logs.** The daemon writes `%USERPROFILE%\.pu\daemon.log`. `pu daemon status` shows where it is.
