#!/bin/sh
# Install the PurePoint CLI (pu) and daemon (pu-engine) into ~/.pu/bin.
#
#   curl -fsSL https://raw.githubusercontent.com/2witstudios/PurePoint/main/scripts/install.sh | sh
#
# Environment:
#   PU_VERSION      release to install, e.g. 1.4.0 (default: latest)
#   PU_INSTALL_DIR  install directory (default: ~/.pu/bin)
#   PU_ARCHIVE      install from a local purepoint-*.tar.gz instead of downloading
#                   (not verified: you vouch for the file)
#   PU_INSTALL_SKIP_VERIFY=1
#                   install a download even if its authenticity can't be
#                   verified (prints a warning)
#
# Downloads are verified before anything is installed. The release key below
# must have signed the archive (checked with minisign, or with OpenSSL 3 when
# minisign isn't installed), and when the GitHub CLI is installed and signed
# in, the archive's build provenance must verify with `gh attestation verify`.
# At least one of the two must succeed, and any check that runs must pass.
#
# Linux builds are static (musl) and run on any distribution. On macOS, prefer
# the PurePoint app, which bundles both binaries.
set -eu

REPO="2witstudios/PurePoint"
# Minisign public key for release archives (.github/scripts/minisign.sh).
RELEASE_PUBLIC_KEY="RWSEvitv5QIgchUQgebD93PuXamKznz5sMJMavmViuMEYSM8nVtcrRv6"
INSTALL_DIR="${PU_INSTALL_DIR:-$HOME/.pu/bin}"

say() { printf '%s\n' "$*"; }
die() { printf 'install.sh: %s\n' "$*" >&2; exit 1; }
warn() { printf 'install.sh: warning: %s\n' "$*" >&2; }

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

b64decode() {
    openssl base64 -d -A
}

# bytes <file> <offset> <count>
bytes() {
    dd if="$1" bs=1 skip="$2" count="$3" 2>/dev/null
}

hex() {
    od -An -v -tx1 | tr -d ' \n'
}

# Ed25519 verification with OpenSSL 3: ed25519_verify <raw-key> <message> <sig>
ed25519_verify() {
    # DER SubjectPublicKeyInfo prefix for an Ed25519 key.
    { printf '\060\052\060\005\006\003\053\145\160\003\041\000'; cat "$1"; } > "$1.der"
    openssl pkeyutl -verify -pubin -keyform DER -inkey "$1.der" -rawin \
        -in "$2" -sigfile "$3" >/dev/null 2>&1
}

# Checks a minisign signature (prehashed "ED" format, the minisign default)
# against RELEASE_PUBLIC_KEY with OpenSSL, including the trusted comment.
openssl_minisign_verify() {
    file=$1 sig=$2 work="$tmp/verify"
    mkdir -p "$work"
    printf '%s' "$RELEASE_PUBLIC_KEY" | b64decode > "$work/pub" 2>/dev/null || return 1
    [ "$(wc -c < "$work/pub" | tr -d ' ')" = 42 ] || return 1
    [ "$(bytes "$work/pub" 0 2)" = Ed ] || return 1
    bytes "$work/pub" 10 32 > "$work/key"

    [ "$(sed -n 1p "$sig" | cut -c 1-19)" = "untrusted comment: " ] || return 1
    sed -n 2p "$sig" | b64decode > "$work/sig-line" 2>/dev/null || return 1
    [ "$(wc -c < "$work/sig-line" | tr -d ' ')" = 74 ] || return 1
    [ "$(bytes "$work/sig-line" 0 2)" = ED ] || return 1
    [ "$(bytes "$work/sig-line" 2 8 | hex)" = "$(bytes "$work/pub" 2 8 | hex)" ] || return 1
    bytes "$work/sig-line" 10 64 > "$work/sig"

    trusted=$(sed -n 3p "$sig")
    case "$trusted" in "trusted comment: "*) ;; *) return 1 ;; esac
    sed -n 4p "$sig" | b64decode > "$work/global" 2>/dev/null || return 1
    [ "$(wc -c < "$work/global" | tr -d ' ')" = 64 ] || return 1

    openssl dgst -blake2b512 -binary "$file" > "$work/hash" || return 1
    ed25519_verify "$work/key" "$work/hash" "$work/sig" || return 1
    { cat "$work/sig"; printf '%s' "${trusted#trusted comment: }"; } > "$work/global-input"
    ed25519_verify "$work/key" "$work/global-input" "$work/global"
}

openssl_can_verify() {
    command -v openssl >/dev/null 2>&1 || return 1
    case "$(openssl version 2>/dev/null)" in
        "OpenSSL 0."* | "OpenSSL 1."* | LibreSSL*) return 1 ;;
        OpenSSL*) return 0 ;;
        *) return 1 ;;
    esac
}

# Prints "verified", "unavailable" (nothing to check with) or "failed".
verify_signature() {
    file=$1 sig_url=$2
    if ! fetch "$sig_url" "$tmp/archive.minisig" 2>/dev/null; then
        warn "the release has no signature ($sig_url)"
        echo unavailable
        return
    fi
    if command -v minisign >/dev/null 2>&1; then
        if minisign -Vm "$file" -x "$tmp/archive.minisig" -P "$RELEASE_PUBLIC_KEY" >/dev/null 2>&1; then
            echo verified
        else
            echo failed
        fi
    elif openssl_can_verify; then
        if openssl_minisign_verify "$file" "$tmp/archive.minisig"; then
            echo verified
        else
            echo failed
        fi
    else
        warn "install minisign or OpenSSL 3 to check the release signature"
        echo unavailable
    fi
}

# Prints "verified", "unavailable" or "failed".
verify_attestation() {
    file=$1
    if ! command -v gh >/dev/null 2>&1 || ! gh auth status >/dev/null 2>&1; then
        echo unavailable
        return
    fi
    if gh attestation verify "$file" --repo "$REPO" >/dev/null 2>&1; then
        echo verified
    else
        echo failed
    fi
}

verify_download() {
    file=$1 sig_url=$2
    signature=$(verify_signature "$file" "$sig_url")
    [ "$signature" != failed ] || die "signature check failed for $(basename "$file"); not installing"
    attestation=$(verify_attestation "$file")
    [ "$attestation" != failed ] || die "gh attestation verify failed for $(basename "$file"); not installing"
    if [ "$signature" = verified ] || [ "$attestation" = verified ]; then
        [ "$signature" = verified ] && say "verified release signature"
        [ "$attestation" = verified ] && say "verified build provenance (gh attestation)"
        return 0
    fi
    if [ "${PU_INSTALL_SKIP_VERIFY:-}" = 1 ]; then
        warn "PU_INSTALL_SKIP_VERIFY=1: installing $(basename "$file") without verifying who built it"
        return 0
    fi
    die "could not verify $(basename "$file"): install minisign or OpenSSL 3, or sign in with the GitHub CLI (gh auth login). To install anyway, set PU_INSTALL_SKIP_VERIFY=1."
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
    verify_download "$archive" "$base/$name.tar.gz.minisig"
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
