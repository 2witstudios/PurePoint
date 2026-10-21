#!/bin/bash
# A throwaway sshd for testing the iPad app's SSH machines against this Mac.
#
#   ipad-test-sshd.sh start <worktree> [--port N] [--sim <udid>|booted]
#   ipad-test-sshd.sh install-key <worktree> [--sim <udid>|booted]
#   ipad-test-sshd.sh stop <worktree>
#   ipad-test-sshd.sh status <worktree>
#
# - Runs the system's /usr/sbin/sshd binary unprivileged, as you, listening on 127.0.0.1 only,
#   with its own host key, authorized_keys and config in a temp dir. It never reads or writes
#   ~/.ssh and never touches the system sshd or its config.
# - `pu` resolves to <worktree>/target/debug/pu (build it with `cargo build -p pu-cli`): every
#   client command runs (ForceCommand) with HOME set to a shadow home whose .pu/bin/pu runs the
#   worktree build with your real HOME. The iPad's relay command,
#   /bin/sh -c 'PATH="$HOME/.pu/bin:$HOME/.cargo/bin:$PATH"; export PATH; exec pu daemon proxy',
#   is run unchanged, so its PATH lookup is what finds the worktree build. Daemon sockets and
#   state stay at your real ~/.pu.
# - Authorizes the simulator's device key, which a DEBUG iPad build writes on every launch to
#   <data container>/Documents/purepoint-debug/id_ed25519.pub (docs/product/ipad-app.md, Debug
#   launch arguments). Launch the app once before `start`, or run `install-key` after it.
#
# Prints the port and user to add as a machine: <user>@127.0.0.1:<port>.
set -euo pipefail

BUNDLE_ID="com.2witstudios.PurePoint.ipad"

usage() {
    sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
}

[ $# -ge 2 ] || usage
COMMAND="$1"
WORKTREE="$(cd "$2" && pwd)"
shift 2
PORT=""
SIM="booted"
while [ $# -gt 0 ]; do
    case "$1" in
        --port) PORT="$2"; shift 2 ;;
        --sim) SIM="$2"; shift 2 ;;
        *) usage ;;
    esac
done

# One state dir per worktree, under the per-user temp dir.
KEY="$(printf '%s' "$WORKTREE" | shasum | cut -c1-12)"
DIR="${TMPDIR:-/tmp}/pu-ipad-test-sshd-$KEY"
PU_BUILD="$WORKTREE/target/debug/pu"

running_pid() {
    [ -f "$DIR/sshd.pid" ] || return 1
    local pid
    pid="$(cat "$DIR/sshd.pid")"
    kill -0 "$pid" 2>/dev/null || return 1
    # Only ever signal the sshd this script started with this config.
    ps -o command= -p "$pid" | grep -q -- "-f $DIR/sshd_config" || return 1
    echo "$pid"
}

install_key() {
    local container pub
    if ! container="$(xcrun simctl get_app_container "$SIM" "$BUNDLE_ID" data 2>/dev/null)"; then
        echo "note: $BUNDLE_ID is not installed on simulator '$SIM'; no device key authorized yet." >&2
        return 0
    fi
    pub="$container/Documents/purepoint-debug/id_ed25519.pub"
    if [ ! -s "$pub" ]; then
        echo "note: $pub does not exist yet; launch the DEBUG app once, then run install-key." >&2
        return 0
    fi
    if ! grep -q '^ssh-ed25519 ' "$pub"; then
        echo "error: $pub is not an ssh-ed25519 public key" >&2
        return 1
    fi
    touch "$DIR/authorized_keys"
    grep -qxF "$(cat "$pub")" "$DIR/authorized_keys" || cat "$pub" >> "$DIR/authorized_keys"
    chmod 600 "$DIR/authorized_keys"
    echo "authorized: $(cut -d' ' -f1,3 "$pub") ($(ssh-keygen -lf "$pub" | awk '{print $2}'))"
}

free_port() {
    python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()'
}

case "$COMMAND" in
start)
    if pid="$(running_pid)"; then
        echo "already running (pid $pid) on port $(cat "$DIR/port")"
        install_key
        echo "machine: $(id -un)@127.0.0.1:$(cat "$DIR/port")"
        exit 0
    fi
    if [ ! -x "$PU_BUILD" ]; then
        echo "error: $PU_BUILD is missing; run 'cargo build -p pu-cli' in $WORKTREE first" >&2
        exit 1
    fi
    "$PU_BUILD" daemon proxy --help >/dev/null 2>&1 || {
        echo "error: $PU_BUILD has no 'daemon proxy'; rebuild it from this worktree" >&2
        exit 1
    }
    rm -rf "$DIR"
    mkdir -p "$DIR/home/.pu/bin"
    chmod 700 "$DIR"
    PORT="${PORT:-$(free_port)}"
    echo "$PORT" > "$DIR/port"
    ssh-keygen -q -t ed25519 -N '' -C pu-ipad-test-host -f "$DIR/host_ed25519"
    touch "$DIR/authorized_keys"
    chmod 600 "$DIR/authorized_keys"

    # The shadow home's `pu`: the worktree build, run with the real HOME.
    cat > "$DIR/home/.pu/bin/pu" <<EOF
#!/bin/sh
HOME='$HOME' exec '$PU_BUILD' "\$@"
EOF
    # Every client command runs exactly as sent, with the shadow HOME for its PATH lookup.
    cat > "$DIR/force-command.sh" <<EOF
#!/bin/sh
HOME='$DIR/home' exec /bin/sh -c "\$SSH_ORIGINAL_COMMAND"
EOF
    chmod 755 "$DIR/home/.pu/bin/pu" "$DIR/force-command.sh"

    cat > "$DIR/sshd_config" <<EOF
ListenAddress 127.0.0.1
Port $PORT
HostKey $DIR/host_ed25519
PidFile $DIR/sshd.pid
AuthorizedKeysFile $DIR/authorized_keys
AllowUsers $(id -un)
PubkeyAuthentication yes
PasswordAuthentication no
KbdInteractiveAuthentication no
UsePAM no
StrictModes no
PermitRootLogin no
AllowTcpForwarding no
AllowStreamLocalForwarding no
X11Forwarding no
PermitTTY no
PrintMotd no
ForceCommand $DIR/force-command.sh
LogLevel VERBOSE
EOF
    /usr/sbin/sshd -t -f "$DIR/sshd_config"
    nohup /usr/sbin/sshd -D -f "$DIR/sshd_config" -E "$DIR/sshd.log" >/dev/null 2>&1 &
    echo $! > "$DIR/sshd.pid"
    for _ in $(seq 1 50); do
        nc -z 127.0.0.1 "$PORT" 2>/dev/null && break
        sleep 0.1
    done
    nc -z 127.0.0.1 "$PORT" 2>/dev/null || { echo "error: sshd did not start; see $DIR/sshd.log" >&2; exit 1; }
    install_key
    echo "sshd: pid $(cat "$DIR/sshd.pid"), 127.0.0.1:$PORT, state $DIR"
    echo "host key: $(ssh-keygen -lf "$DIR/host_ed25519.pub" | awk '{print $2}')"
    echo "port: $PORT"
    echo "user: $(id -un)"
    echo "machine: $(id -un)@127.0.0.1:$PORT"
    ;;
install-key)
    [ -d "$DIR" ] || { echo "error: not started; run start first" >&2; exit 1; }
    install_key
    ;;
status)
    if pid="$(running_pid)"; then
        echo "running: pid $pid, machine $(id -un)@127.0.0.1:$(cat "$DIR/port"), state $DIR"
    else
        echo "not running"
    fi
    ;;
stop)
    if pid="$(running_pid)"; then
        kill "$pid"
        echo "stopped sshd (pid $pid)"
    else
        echo "not running"
    fi
    rm -rf "$DIR"
    ;;
*)
    usage
    ;;
esac
