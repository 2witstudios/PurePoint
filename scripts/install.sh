#!/bin/sh
# Install the PurePoint CLI (pu) and daemon (pu-engine) into ~/.pu/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | sh
#
# Environment:
#   PU_VERSION      release to install, e.g. 1.4.0 (default: latest)
#   PU_INSTALL_DIR  install directory (default: ~/.pu/bin)
#   PU_ARCHIVE      install from a local purepoint-*.tar.gz instead of downloading
#
# Linux builds are static (musl) and run on any distribution. On macOS, prefer
# the PurePoint app, which bundles both binaries.
set -eu

REPO="2witstudios/PurePoint"
INSTALL_DIR="${PU_INSTALL_DIR:-$HOME/.pu/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }

detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$arch" in
        x86_64 | amd64) arch=x86_64 ;;
        aarch64 | arm64) arch=aarch64 ;;
        *) die "unsupported architecture: $arch" ;;
    esac
    case "$os" in
        Linux) printf '%s-unknown-linux-musl' "$arch" ;;
        *) die "no prebuilt $os binaries — on macOS install the PurePoint app, elsewhere build with cargo (see README)" ;;
    esac
}

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        die "need curl or wget"
    fi
}

latest_version() {
    url="https://api.github.com/repos/$REPO/releases/latest"
    fetch "$url" "$tmp/latest.json" || die "could not query $url"
    sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' "$tmp/latest.json" | head -n 1
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        die "need sha256sum or shasum to verify the download"
    fi
}

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT INT TERM

if [ -n "${PU_ARCHIVE:-}" ]; then
    archive="$PU_ARCHIVE"
    [ -f "$archive" ] || die "PU_ARCHIVE not found: $archive"
else
    target=$(detect_target)
    version="${PU_VERSION:-}"
    [ -n "$version" ] || version=$(latest_version)
    [ -n "$version" ] || die "could not determine the latest release"
    version="${version#v}"
    name="purepoint-$version-$target"
    base="https://github.com/$REPO/releases/download/v$version"
    archive="$tmp/$name.tar.gz"
    say "downloading $name"
    fetch "$base/$name.tar.gz" "$archive" || die "download failed: $base/$name.tar.gz"
    fetch "$base/$name.tar.gz.sha256" "$tmp/sum" || die "checksum download failed"
    expected=$(cut -d ' ' -f 1 "$tmp/sum")
    actual=$(sha256_of "$archive")
    [ "$expected" = "$actual" ] || die "checksum mismatch for $name.tar.gz"
fi

mkdir -p "$tmp/x"
tar -xzf "$archive" -C "$tmp/x"
pu=$(find "$tmp/x" -type f -name pu | head -n 1)
engine=$(find "$tmp/x" -type f -name pu-engine | head -n 1)
[ -n "$pu" ] && [ -n "$engine" ] || die "archive does not contain pu and pu-engine"

mkdir -p "$INSTALL_DIR"
# Replace by rename so a running daemon keeps its (unlinked) binary intact.
# The temp name includes $$ so concurrent installs cannot interleave cp/mv
# and publish a half-written binary.
for bin in "$pu" "$engine"; do
    dest="$INSTALL_DIR/$(basename "$bin")"
    staging="$dest.new.$$"
    cp "$bin" "$staging"
    chmod 755 "$staging"
    mv -f "$staging" "$dest"
done
say "installed pu and pu-engine to $INSTALL_DIR"

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        say ""
        say "add it to your PATH, e.g.:"
        say "  echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.profile"
        ;;
esac

say ""
say "next steps on this server:"
say "  $INSTALL_DIR/pu daemon install --now   # run the daemon as a user service"
say "  cd <repo> && $INSTALL_DIR/pu init        # register each project"
say "If the daemon is already running, restart it to pick up the new version (pu daemon stop && pu daemon start)."
