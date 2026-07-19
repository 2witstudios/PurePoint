#!/bin/bash
# Launch a dev build of the PurePoint macOS app inside a sandboxed $HOME so it
# cannot touch the production install you're actively using.
#
# Why: every piece of PurePoint state hangs off $HOME — the daemon socket
# (~/.pu/daemon.sock), the daemon it spawns, ~/.pu/bin, CLIInstaller's targets,
# and UserDefaults (restored projects). A dev instance launched with your real
# HOME attaches to the production daemon, auto-opens your real projects, and —
# worst of all — sends the daemon a shutdown RPC when it quits. Overriding HOME
# isolates all of it: the dev instance spawns its own daemon on its own socket
# and tears down only that daemon on quit.
#
# Usage:
#   scripts/dev-app-sandbox.sh [path/to/purepoint-macos.app]
#
# Build first in Xcode with Cmd+B (Debug). NEVER Cmd+R — Xcode runs the app
# with your real HOME, which is exactly the unsafe case described above.
#
# Env:
#   PU_DEV_HOME   sandbox home directory (default: $HOME/.pu-dev-home)
set -euo pipefail

SANDBOX_HOME="${PU_DEV_HOME:-$HOME/.pu-dev-home}"

# Resolve the built app: explicit arg, else newest Debug product from
# DerivedData or the project-local build dir.
if [ $# -ge 1 ]; then
    APP="$1"
else
    REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
    APP="$(ls -td \
        "$HOME"/Library/Developer/Xcode/DerivedData/purepoint-macos-*/Build/Products/Debug/purepoint-macos.app \
        "$REPO_ROOT"/apps/purepoint-macos/build/Debug/purepoint-macos.app \
        2>/dev/null | head -1 || true)"
fi

BINARY="$APP/Contents/MacOS/purepoint-macos"
if [ -z "$APP" ] || [ ! -x "$BINARY" ]; then
    echo "error: no built app found. Build in Xcode first (Cmd+B, Debug)," >&2
    echo "or pass the .app path: $0 path/to/purepoint-macos.app" >&2
    exit 1
fi

mkdir -p "$SANDBOX_HOME"

# A scratch git repo so the dev app has a project to open.
SCRATCH="$SANDBOX_HOME/projects/scratch"
if [ ! -d "$SCRATCH/.git" ]; then
    mkdir -p "$SCRATCH"
    git -C "$SCRATCH" init -q
    echo "# PurePoint sandbox scratch project" > "$SCRATCH/README.md"
fi

echo "app:          $APP"
echo "sandbox HOME: $SANDBOX_HOME"
echo "test project: $SCRATCH"
echo

# Direct binary exec, not `open`: the dev build shares the production bundle
# id, so `open` would just focus the running production app. Direct exec also
# keeps the app's stdout (debug prints) in this terminal.
exec env HOME="$SANDBOX_HOME" "$BINARY"
